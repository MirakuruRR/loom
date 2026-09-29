//! A cooked Blueprint to a loom source: its header, variables, defaults, components,
//! dispatchers, functions and event handlers.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use serde_json::Value;

use super::bytecode::{Lx, Reader};
use super::code::{Code, Flow};
use super::emit::{self, Emitter, Returns};
use super::flow;
use super::print;
use super::input::{str_of, Prop, PropTy, Ref};
use super::program::{own_name, Blueprint, Dir, Function, Program};
use crate::syntax::{Access, Expr, ExprKind, File, Interface, Item, ParamDecl, Purity, Signature, Span, Stmt, TypeName};

fn tn(name: String) -> TypeName {
    TypeName { name, span: Span::default() }
}

/// Names the compiler gives the locals that hold values between nodes, which are folded back
/// into what reads them when every value they hold is pure.
fn is_temporary(cooked: &str) -> bool {
    cooked.starts_with("CallFunc_")
}

pub fn lift(program: &Program, bp: &Blueprint) -> Result<File> {
    let package = &bp.package;
    let class = package
        .exports
        .iter()
        .find(|e| matches!(str_of(e, "Type"), Some("BlueprintGeneratedClass" | "WidgetBlueprintGeneratedClass")))
        .ok_or_else(|| refusal!(Unexpected, "a Blueprint package with no class"))?;
    let cdo_name = class.pointer("/ClassDefaultObject/ObjectName").and_then(Value::as_str).and_then(|n| n.split('\'').nth(1));
    let cdo = cdo_name.and_then(|n| package.export_named(n)).and_then(|c| c.get("Properties")).cloned().unwrap_or(Value::Null);
    let native = program.native_ancestor(&bp.parent);

    let mut items = Vec::new();

    // The interfaces the class implements that its parent does not already.
    let inherited = program.implemented(&bp.parent);
    for interface in bp.interfaces.iter().filter(|i| !inherited.contains(i)) {
        items.push(Item::Implements { interface: tn(program.class_name(interface)?), span: Span::default() });
    }

    // Components, from the construction script's nodes.
    let mut components = HashSet::new();
    let mut component_items = Vec::new();
    for node in package.exports.iter().filter(|e| str_of(e, "Type") == Some("SCS_Node")) {
        let props = node.get("Properties").ok_or_else(|| refusal!(Unexpected, "a component with nothing set"))?;
        let name = str_of(props, "InternalVariableName").ok_or_else(|| refusal!(Unexpected, "a component with no name"))?;
        let class = Ref::of(props.get("ComponentClass").ok_or_else(|| refusal!(Unexpected, "component {name} with no class"))?)?.path();
        let mut fields = Vec::new();
        if let Some(template) = props.get("ComponentTemplate").and_then(|t| Ref::of(t).ok()) {
            let template_name = template.name.rsplit([':', '.']).next().unwrap_or(&template.name);
            if let Some(t) = package.export_named(template_name).and_then(|t| t.get("Properties")).and_then(Value::as_object) {
                for (field, value) in t {
                    let ty = program.db.editable_property(&class, field).ok_or_else(|| refusal!(Unexpected, "component {name}'s {field}, which its class does not have"))?;
                    if let Some(lit) = program.literal(&PropTy::from_ty(&ty.ty)?, value).with_context(|| format!("component {name}'s {field}"))? {
                        fields.push((field.clone(), lit));
                    }
                }
            }
        }
        components.insert(name.to_string());
        component_items.push(Item::Component { name: own_name(name)?, class: tn(print::name(&program.class_name(&class)?)), fields, span: Span::default() });
    }

    // A widget Blueprint's tree, whose widgets are variables of it.
    let mut widget_items = Vec::new();
    if bp.widget {
        if let Some(canvas) = widget_tree(program, package, &mut widget_items).context("reading the widget tree")? {
            components.insert(canvas);
        }
        for w in &widget_items {
            if let Item::Widget { name, .. } = w {
                components.insert(name.clone());
            }
        }
    }

    // Variables and dispatchers, from the class's own properties.
    let mut own = HashSet::new();
    for p in class.get("ChildProperties").and_then(Value::as_array).into_iter().flatten() {
        let prop = Prop::of(p)?;
        own.insert(prop.name.clone());
        if prop.name == "UberGraphFrame" || components.contains(&prop.name) {
            continue;
        }
        match prop.ty {
            PropTy::MulticastDelegate => {
                let signature = format!("{}__DelegateSignature", prop.name);
                let f = package.export_named(&signature).ok_or_else(|| refusal!(Unexpected, "dispatcher {} with no signature", prop.name))?;
                let mut params = Vec::new();
                for p in f.get("ChildProperties").and_then(Value::as_array).into_iter().flatten() {
                    let p = Prop::of(p)?;
                    params.push(ParamDecl { name: own_name(&p.name)?, ty: tn(program.type_name(&p.ty)?), by_ref: false, out: false, default: None });
                }
                items.push(Item::Dispatcher { name: own_name(&prop.name)?, params, doc: None, span: Span::default() });
            }
            _ => {
                let ty = program.type_name(&prop.ty).with_context(|| format!("variable {}", prop.name))?;
                let default = match cdo.get(&prop.name) {
                    Some(v) => program.literal(&prop.ty, v).with_context(|| format!("variable {}'s default", prop.name))?,
                    None => None,
                };
                let editable = prop.has("Edit") && !prop.has("DisableEditOnInstance");
                items.push(Item::Var { name: own_name(&prop.name)?, ty: tn(ty), default, editable, private: false, doc: None, span: Span::default() });
            }
        }
    }

    // The parent's properties this class gives defaults of its own.
    if let Some(defaults) = cdo.as_object() {
        for (field, value) in defaults {
            if own.contains(field) {
                continue;
            }
            // Flags the widget compiler sets from whether the Blueprint implements a function,
            // which loom's compile sets the same way.
            let derived = match field.as_str() {
                "bHasScriptImplementedTick" => Some(bp.functions.contains_key("Tick")),
                "bHasScriptImplementedPaint" => Some(bp.functions.contains_key("OnPaint")),
                _ => None,
            };
            if let Some(expected) = derived {
                if value.as_bool() == Some(expected) {
                    continue;
                }
                unexpected!("{field} set to {value}, which does not match what the Blueprint implements");
            }
            let Some(p) = program.db.editable_property(&native, field) else {
                unexpected!("a default for {field}, which {} does not have", program.class_name(&native)?);
            };
            let ty = PropTy::from_ty(&p.ty)?;
            let mut value = value.clone();
            // The Blueprint compiler lets a Blueprint that handles Tick tick, so loom's does too.
            if field == "PrimaryActorTick" && bp.functions.contains_key("ReceiveTick") {
                if let Some(tick) = value.as_object_mut() {
                    if tick.get("bCanEverTick") == Some(&Value::Bool(true)) {
                        tick.remove("bCanEverTick");
                    }
                }
            }
            default_items(program, vec![field.clone()], &ty, &value, &mut items)?;
        }
    }
    items.extend(component_items);
    items.extend(widget_items);

    // Functions and event handlers.
    let ubergraph = package.exports.iter().find(|e| {
        str_of(e, "Type") == Some("Function") && str_of(e, "Name").is_some_and(|n| n.starts_with("ExecuteUbergraph_"))
    });
    let mut names: Vec<&String> = bp.functions.keys().collect();
    names.sort_by_key(|n| (bp.functions[*n].stub.is_none(), n.to_string()));
    let handlers: Vec<&String> = names.iter().copied().filter(|n| bp.functions[*n].stub.is_some()).collect();
    if !handlers.is_empty() && ubergraph.is_none() {
        unexpected!("event {} with no event graph", handlers[0]);
    }

    // The event graph's locals persist between events, so one that a handler reads before it
    // sets holds what another event left in it: it is the Blueprint's variable, in every
    // handler, and no handler may fold its value away or drop it as unread. Finding one can
    // show another, so the handlers are read again until no more are found.
    let mut shared: HashSet<String> = HashSet::new();
    let (read, graph_vars) = loop {
        let mut graph_vars: Vec<(String, PropTy)> = Vec::new();
        let mut read = Vec::new();
        if let Some(u) = ubergraph {
            let mut reader = Reader::new(program, bp, u)?;
            let code = reader.code(u).context("reading the event graph")?;
            for cooked in &handlers {
                let f = &bp.functions[*cooked];
                let stub = f.stub.as_ref().expect("a handler has an entry point");
                read.push(handler(program, &native, f, stub, &mut reader, &code, &shared, implements(program, &bp.class, &f.name), &mut graph_vars).with_context(|| format!("event {cooked}"))?);
            }
        }
        let found: HashSet<String> = graph_vars.iter().map(|(name, _)| name.clone()).collect();
        if found.is_subset(&shared) {
            break (read, graph_vars);
        }
        shared.extend(found);
    };
    for (mut item, locals) in read {
        let locals: Vec<_> = locals.into_iter().filter(|(name, ..)| !shared.contains(name)).collect();
        match &mut item {
            Item::On { body, .. } | Item::Event { body, .. } => emit::declare(body, &locals),
            _ => unreachable!("a handler is an on or an event"),
        }
        items.push(item);
    }
    for cooked in names.iter().filter(|n| bp.functions[**n].stub.is_none()) {
        let f = &bp.functions[*cooked];
        let export = package.export_named(cooked).ok_or_else(|| refusal!(Unexpected, "function {cooked}, which the package does not hold"))?;
        items.push(function(program, bp, f, export).with_context(|| format!("function {cooked}"))?);
    }

    // The event graph's variables that one event sets and another reads are the Blueprint's.
    let mut declared = HashSet::new();
    let mut vars = Vec::new();
    for (name, ty) in graph_vars {
        if declared.insert(name.clone()) {
            vars.push(Item::Var { name, ty: tn(program.type_name(&ty)?), default: None, editable: false, private: true, doc: None, span: Span::default() });
        }
    }
    let at = items.iter().position(|i| !matches!(i, Item::Var { .. })).unwrap_or(items.len());
    items.splice(at..at, vars);

    Ok(File { editor: false, name: bp.name.clone(), parent: program.class_name(&bp.parent)?, path: bp.path.clone(), items })
}

/// A widget Blueprint's tree as `widget` items: the root canvas's children placed on it, and each
/// panel's children in its block. Returns the root canvas's name, which loom makes itself.
fn widget_tree(program: &Program, package: &super::input::Package, items: &mut Vec<Item>) -> Result<Option<String>> {
    let tree = package.export("WidgetTree").ok_or_else(|| refusal!(Unexpected, "a widget Blueprint with no tree"))?;
    let Some(root) = tree.pointer("/Properties/RootWidget") else { return Ok(None) };
    let root = subobject(package, root)?;
    if str_of(root, "Type") == Some("CanvasPanel") {
        // loom's widget Blueprints start with a root canvas, which the placed widgets go on.
        for slot in slots(package, root)? {
            let (child, mut fields) = slot_of(program, package, slot)?;
            if fields.place.is_none() {
                // Placed at its offsets rounded, then given its exact layout, which the slot's
                // fields set after the place.
                let layout = slot.pointer("/Properties/LayoutData/Offsets");
                let n = |k: &str| layout.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(0.0).round().max(0.0) as i64;
                fields.place = Some([n("Left"), n("Top"), n("Right"), n("Bottom")]);
            }
            widget(program, package, child, None, fields, items)?;
        }
        return str_of(root, "Name").map(own_name).transpose();
    }
    widget(program, package, root, None, SlotFields::default(), items)?;
    Ok(None)
}

#[derive(Default)]
struct SlotFields {
    place: Option<[i64; 4]>,
    fields: Vec<(String, Expr)>,
}

fn subobject<'p>(package: &'p super::input::Package, r: &Value) -> Result<&'p Value> {
    let r = Ref::of(r)?;
    let name = r.name.rsplit(['.', ':']).next().unwrap_or(&r.name);
    package.export_named(name).ok_or_else(|| refusal!(Unexpected, "{}, which the package does not hold", r.name))
}

fn slots<'p>(package: &'p super::input::Package, panel: &Value) -> Result<Vec<&'p Value>> {
    panel.pointer("/Properties/Slots").and_then(Value::as_array).into_iter().flatten().map(|s| subobject(package, s)).collect()
}

/// The widget a slot holds, and what the slot sets: for a canvas, where the widget is.
fn slot_of<'p>(program: &Program, package: &'p super::input::Package, slot: &Value) -> Result<(&'p Value, SlotFields)> {
    let props = slot.get("Properties").and_then(Value::as_object).ok_or_else(|| refusal!(Unexpected, "a slot with nothing set"))?;
    let content = props.get("Content").ok_or_else(|| refusal!(Unexpected, "a slot holding nothing"))?;
    let child = subobject(package, content)?;
    let class = native_class(program, str_of(slot, "Class").unwrap_or_default())?;
    let mut out = SlotFields::default();
    for (field, value) in props {
        if matches!(field.as_str(), "Content" | "Parent") {
            continue;
        }
        // A canvas slot at whole offsets from a top-left anchor is written `at x y size w h`.
        if field == "LayoutData" && str_of(slot, "Type") == Some("CanvasPanelSlot") {
            if let Some(place) = whole_place(value) {
                out.place = Some(place);
                continue;
            }
        }
        let p = program.db.editable_property(&class, field).ok_or_else(|| refusal!(Unexpected, "{field}, which a {} does not have", str_of(slot, "Type").unwrap_or("slot")))?;
        if let Some(lit) = program.literal(&PropTy::from_ty(&p.ty)?, value)? {
            out.fields.push((field.clone(), lit));
        }
    }
    Ok((child, out))
}

/// A canvas slot's layout as `at x y size w h`, when it only sets whole offsets that are not
/// negative, as `at` writes them.
fn whole_place(layout: &Value) -> Option<[i64; 4]> {
    let layout = layout.as_object()?;
    if layout.keys().any(|k| k != "Offsets") {
        return None;
    }
    let o = layout.get("Offsets")?;
    let n = |k: &str| -> Option<i64> {
        let v = o.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        (v.fract() == 0.0 && v >= 0.0).then_some(v as i64)
    };
    Some([n("Left")?, n("Top")?, n("Right")?, n("Bottom")?])
}

/// The class a UScriptClass'Name' names, looked up among the engine's classes.
fn native_class(program: &Program, class: &str) -> Result<String> {
    let name = class.split('\'').nth(1).unwrap_or(class);
    program
        .db
        .class_in(name, false, &crate::types::TypeScope { folder: "", uses: &[] })?
        .ok_or_else(|| refusal!(Unexpected, "{name} where a class should be"))
}

fn widget(program: &Program, package: &super::input::Package, w: &Value, parent: Option<String>, slot: SlotFields, items: &mut Vec<Item>) -> Result<()> {
    let name = own_name(str_of(w, "Name").ok_or_else(|| refusal!(Unexpected, "a widget with no name"))?)?;
    let class = native_class(program, str_of(w, "Class").unwrap_or_default()).with_context(|| format!("widget {name}"))?;
    let mut fields = Vec::new();
    let empty = serde_json::Map::new();
    let props = w.get("Properties").and_then(Value::as_object).unwrap_or(&empty);
    for (field, value) in props {
        if matches!(field.as_str(), "Slot" | "Slots" | "bIsVariable") {
            continue;
        }
        let p = program.db.editable_property(&class, field).ok_or_else(|| refusal!(Unexpected, "widget {name}'s {field}, which its class does not have"))?;
        if let Some(lit) = program.literal(&PropTy::from_ty(&p.ty)?, value).with_context(|| format!("widget {name}'s {field}"))? {
            fields.push((field.clone(), lit));
        }
    }
    items.push(Item::Widget {
        name: name.clone(),
        class: tn(print::name(&program.class_name(&class)?)),
        fields,
        slot: slot.fields,
        place: slot.place,
        parent,
        span: Span::default(),
    });
    for s in slots(package, w)? {
        let (child, fields) = slot_of(program, package, s)?;
        widget(program, package, child, Some(name.clone()), fields, items)?;
    }
    Ok(())
}

/// `default path = value`, a struct's fields each on its own line, so only those the class sets
/// are written.
fn default_items(program: &Program, path: Vec<String>, ty: &PropTy, value: &Value, items: &mut Vec<Item>) -> Result<()> {
    if let (PropTy::Struct(s), Value::Object(fields)) = (ty, value) {
        for (field, v) in fields {
            let (name, fty) = program.struct_field(s, field)?;
            let mut p = path.clone();
            p.push(name);
            default_items(program, p, &fty, v, items)?;
        }
        return Ok(());
    }
    let value = match program.literal(ty, value).with_context(|| format!("default {}", path.join(".")))? {
        Some(v) => v,
        None => program.zero(ty)?,
    };
    items.push(Item::Default { path, value, span: Span::default() });
    Ok(())
}

fn params(program: &Program, f: &Function, skip_return: bool) -> Result<Vec<ParamDecl>> {
    let mut out = Vec::new();
    for p in &f.params {
        if p.cooked == "__WorldContext" || (skip_return && p.dir == Dir::Out && p.cooked == "ReturnValue") {
            continue;
        }
        out.push(ParamDecl {
            name: p.name.clone(),
            ty: tn(program.type_name(&p.ty).with_context(|| format!("parameter {}", p.cooked))?),
            by_ref: p.dir == Dir::Ref,
            out: p.dir == Dir::Out,
            default: None,
        });
    }
    Ok(out)
}

/// The type of the output called ReturnValue, which loom writes as the function's `-> Type`, as
/// it names the return pin of a function it builds. Any other output stays an `out` parameter.
fn single_return(f: &Function) -> Option<&PropTy> {
    f.params.iter().find(|p| p.dir == Dir::Out && p.cooked == "ReturnValue").map(|p| &p.ty)
}

/// Whether a function implements a function of an interface the class implements.
fn implements(program: &Program, class: &str, function: &str) -> bool {
    program.implemented(class).iter().any(|i| program.interface_declares(i, function))
}

/// A Blueprint Interface as an interface's source: each function's signature, in the order the
/// package holds them.
pub fn lift_interface(program: &Program, bp: &Blueprint) -> Result<Interface> {
    let mut functions = Vec::new();
    for export in bp.package.exports.iter().filter(|e| str_of(e, "Type") == Some("Function")) {
        let Some(f) = str_of(export, "Name").and_then(|n| bp.functions.get(n)) else { continue };
        if let Some(p) = f.params.iter().find(|p| p.dir == Dir::Ref) {
            cannot!("{}'s parameter {}, which is a ref, as an interface's function's cannot be yet", f.name, p.name);
        }
        let ret = single_return(f).map(|t| program.type_name(t)).transpose()?.map(tn);
        let params = params(program, f, ret.is_some()).with_context(|| format!("function {}", f.name))?;
        functions.push(Signature { name: f.name.clone(), params, ret, doc: None, span: Span::default() });
    }
    Ok(Interface { name: bp.name.clone(), path: bp.path.clone(), functions, doc: None, span: Span::default() })
}

fn function(program: &Program, bp: &Blueprint, f: &Function, export: &Value) -> Result<Item> {
    let mut reader = Reader::new(program, bp, export)?;
    let ret = single_return(f).cloned();
    let result = ret.as_ref().map(|_| "result".to_string());
    for p in &f.params {
        let name = if p.cooked == "ReturnValue" && ret.is_some() { "result".to_string() } else { p.name.clone() };
        reader.names.insert(p.cooked.clone(), name);
    }
    let code = reader.code(export)?;
    let flows = flow::structure(&code, None)?;
    let returns = match &result {
        Some(r) => Returns::Value(r.clone()),
        None => Returns::Plain,
    };
    let mut body = emit_body(program, &reader, &code, &flows, HashMap::new(), returns, &HashSet::new())?;
    let mut locals = locals_of(program, &reader, f)?;
    if let (Some(r), Some(ty)) = (&result, &ret) {
        locals.push((r.clone(), tn(program.type_name(ty)?), program.zero(ty)?));
    }
    emit::declare(&mut body, &locals);
    let params = params(program, f, ret.is_some())?;
    let ret = ret.map(|t| program.type_name(&t)).transpose()?.map(tn);
    // A function that overrides the parent's, or implements an interface's, is its handler.
    if f.overrides || implements(program, &bp.class, &f.name) {
        return Ok(Item::On { component: None, event: f.name.clone(), params, ret, body, span: Span::default() });
    }
    let access = match f.access {
        Some("private") => Access::Private,
        Some("protected") => Access::Protected,
        _ => Access::Public,
    };
    let pure = if f.pure { Purity::Pure } else { Purity::Impure };
    Ok(Item::Fn { name: f.name.clone(), params, ret, body, doc: None, access, pure, span: Span::default() })
}

/// An event's or custom event's handler: the event graph from the event's entry point.
fn handler(
    program: &Program,
    native: &str,
    f: &Function,
    stub: &super::program::Stub,
    reader: &mut Reader,
    code: &Code<Vec<Lx>, Expr>,
    shared: &HashSet<String>,
    interface_fn: bool,
    graph_vars: &mut Vec<(String, PropTy)>,
) -> Result<(Item, Vec<(String, TypeName, Expr)>)> {
    let flows = flow::structure(code, Some(stub.entry))?;
    // The graph's copies of the event's parameters are the parameters.
    let mut renames = HashMap::new();
    for (copy, param) in &stub.copies {
        let name = f.params.iter().find(|p| &p.cooked == param).map(|p| p.name.clone()).unwrap_or_else(|| param.clone());
        renames.insert(reader.local_name(copy), name);
    }
    let body = emit_body(program, reader, code, &flows, renames.clone(), Returns::Plain, shared)?;
    // A graph variable the handler reads before it sets holds what another event, or an earlier
    // run of this one, left in it, so it is the Blueprint's variable, not the handler's.
    let mut locals = Vec::new();
    let param_names: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    for (name, ty, zero) in locals_of(program, reader, f)? {
        // The graph's copies of this event's parameters are the parameters themselves.
        if renames.contains_key(&name) || param_names.contains(&name) {
            continue;
        }
        if emit::read_first(&body, &name) {
            let cooked = reader.locals.iter().find(|(c, _)| reader.local_name(c) == name).map(|(_, p)| p.ty.clone());
            if let Some(t) = cooked {
                graph_vars.push((name, t));
            }
        } else {
            locals.push((name, ty, zero));
        }
    }
    let params = params(program, f, false)?;
    let is_event = f.overrides || program.db.event(native, &f.name).is_ok() || interface_fn;
    if is_event {
        return Ok((Item::On { component: None, event: f.name.clone(), params, ret: None, body, span: Span::default() }, locals));
    }
    // Anything else with an entry point in the event graph is a custom event, which may wait.
    Ok((Item::Event { name: f.name.clone(), params, body, doc: None, span: Span::default() }, locals))
}

fn emit_body(
    _program: &Program,
    reader: &Reader,
    code: &Code<Vec<Lx>, Expr>,
    flows: &[Flow<Vec<Lx>, Expr>],
    renames: HashMap<String, String>,
    returns: Returns,
    shared: &HashSet<String>,
) -> Result<Vec<Stmt>> {
    let inline = inlinable(reader, code).difference(shared).cloned().collect();
    let mut emitter = Emitter::new(inline, renames, returns);
    let mut body = emitter.body(flows)?;
    // A call's value that nothing reads is not kept: the call stands on its own.
    // Parameters are left alone: an out parameter is the function's output, which the body only
    // sets.
    let locals: HashSet<String> = reader.locals.iter().filter(|(_, p)| !p.has("Parm")).map(|(c, _)| reader.local_name(c)).filter(|n| !shared.contains(n)).collect();
    emit::drop_unread(&mut body, &locals);
    // The macros' loops, whose counters are this body's alone, are loom's `for`.
    super::loops::recover(&mut body, &locals);
    Ok(body)
}

/// The compiler's temporaries that only ever hold a pure value, which are read where they are
/// used rather than declared.
fn inlinable(reader: &Reader, code: &Code<Vec<Lx>, Expr>) -> HashSet<String> {
    let mut impure = HashSet::new();
    let mut assigned = HashSet::new();
    for (_, insn) in &code.insns {
        let stmts = match insn {
            super::code::Insn::Stmt(s) => s,
            super::code::Insn::Latent { call, .. } => call,
            _ => continue,
        };
        for lx in stmts {
            if let Lx::Assign { target, pure, .. } = lx {
                if let ExprKind::Name(n) = &target.kind {
                    assigned.insert(n.clone());
                    if !pure {
                        impure.insert(n.clone());
                    }
                }
            }
        }
    }
    reader
        .locals
        .keys()
        .filter(|c| is_temporary(c))
        .map(|c| reader.local_name(c))
        .filter(|n| assigned.contains(n) && !impure.contains(n))
        .collect()
}

/// The function's locals that are not its parameters, each with its type and zero, for those
/// the body uses to be declared.
fn locals_of(program: &Program, reader: &Reader, f: &Function) -> Result<Vec<(String, TypeName, Expr)>> {
    let params: HashSet<&str> = f.params.iter().map(|p| p.cooked.as_str()).collect();
    let mut out = Vec::new();
    let mut cooked: Vec<&String> = reader.locals.keys().collect();
    cooked.sort();
    for c in cooked {
        let prop = &reader.locals[c];
        if params.contains(c.as_str()) || prop.has("Parm") || c == "__WorldContext" {
            continue;
        }
        if matches!(prop.ty, PropTy::Delegate | PropTy::MulticastDelegate) {
            continue;
        }
        let ty = program.type_name(&prop.ty).with_context(|| format!("local {c}"));
        let zero = program.zero(&prop.ty);
        match (ty, zero) {
            (Ok(ty), Ok(zero)) => out.push((reader.local_name(c), tn(ty), zero)),
            // A local of a type loom cannot declare is an error only if the body uses it, which
            // declaring finds out.
            (Err(err), _) | (_, Err(err)) => {
                out.push((reader.local_name(c), tn(format!("<{err}>")), Expr { kind: ExprKind::Null, span: Span::default() }))
            }
        }
    }
    Ok(out)
}
