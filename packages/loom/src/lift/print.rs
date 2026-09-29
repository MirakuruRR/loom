//! A syntax tree back to source text, laid out as the reference writes it.

use std::fmt::Write;

use crate::syntax::{
    written_name, Access, BinaryOp, Enum, Expr, ExprKind, File, Interface, Item, ParamDecl, Purity, Source, Stmt, Struct, UnaryOp, BINARY_OPERATORS,
    SELF_WORDS,
};

const INDENT: &str = "    ";

/// A function's name as a source writes it: in backticks when it is a built-in function's, which
/// a bare call would mean instead.
pub fn function_name(n: &str) -> String {
    if crate::builtins::function(n).is_some() { format!("`{n}`") } else { name(n) }
}

/// A name as a source writes it, in backticks where it needs them. A name no source can write
/// is written in backticks all the same, so the check that the source parses back finds it.
pub fn name(n: &str) -> String {
    written_name(n).unwrap_or_else(|| format!("`{n}`"))
}

pub fn source(source: &Source) -> String {
    match source {
        Source::Blueprint(file) => blueprint(file),
        Source::Enum(e) => enumeration(e),
        Source::Struct(s) => structure(s),
        Source::Interface(i) => interface(i),
    }
}

fn interface(i: &Interface) -> String {
    let mut out = String::new();
    doc(&mut out, &i.doc, "");
    let _ = writeln!(out, "interface {} at {} {{", name(&i.name), i.path);
    for f in &i.functions {
        doc(&mut out, &f.doc, INDENT);
        let _ = write!(out, "{INDENT}fn {}({})", function_name(&f.name), param_list(&f.params));
        if let Some(t) = &f.ret {
            let _ = write!(out, " -> {}", t.name);
        }
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

fn doc(out: &mut String, doc: &Option<String>, indent: &str) {
    if let Some(doc) = doc {
        for line in doc.lines() {
            if line.is_empty() {
                let _ = writeln!(out, "{indent}//");
            } else {
                let _ = writeln!(out, "{indent}// {line}");
            }
        }
    }
}

fn enumeration(e: &Enum) -> String {
    let mut out = String::new();
    doc(&mut out, &e.doc, "");
    let _ = writeln!(out, "enum {} at {} {{", name(&e.name), e.path);
    for v in &e.values {
        doc(&mut out, &v.doc, INDENT);
        match &v.label {
            Some(label) => {
                let _ = writeln!(out, "{INDENT}{} {}", name(&v.name), string(label));
            }
            None => {
                let _ = writeln!(out, "{INDENT}{}", name(&v.name));
            }
        }
    }
    out.push_str("}\n");
    out
}

fn structure(s: &Struct) -> String {
    let mut out = String::new();
    doc(&mut out, &s.doc, "");
    let _ = writeln!(out, "struct {} at {} {{", name(&s.name), s.path);
    for f in &s.fields {
        doc(&mut out, &f.doc, INDENT);
        let _ = write!(out, "{INDENT}{}: {}", name(&f.name), f.ty.name);
        if let Some(d) = &f.default {
            let _ = write!(out, " = {}", expr(d));
        }
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

fn blueprint(file: &File) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}blueprint {} : {} at {}", if file.editor { "editor " } else { "" }, name(&file.name), name(&file.parent), file.path);
    // The header's own lines, then a blank line between groups of items of one kind, and between
    // every function or handler.
    let mut last: Option<&'static str> = None;
    for item in &file.items {
        let kind = match item {
            Item::Use { .. } | Item::Implements { .. } => "header",
            Item::Category { .. } => "category",
            Item::Var { .. } => "var",
            Item::Default { .. } => "default",
            Item::Dispatcher { .. } => "dispatcher",
            Item::Component { .. } => "component",
            Item::Widget { .. } => "widget",
            Item::Fn { .. } | Item::Event { .. } | Item::On { .. } => "body",
        };
        let blank = match (last, kind) {
            (None, "header") | (Some("header"), "header") => false,
            (Some(prev), k) => prev != k || k == "body",
            (None, _) => true,
        };
        if blank {
            out.push('\n');
        }
        last = Some(kind);
        match item {
            Item::Use { folder, .. } => {
                let _ = writeln!(out, "use {folder}");
            }
            Item::Implements { interface, .. } => {
                let _ = writeln!(out, "implements {}", interface.name);
            }
            Item::Category { name, .. } => {
                let _ = writeln!(out, "category {}", string(name));
            }
            Item::Var { name: n, ty, default, editable, private, doc: d, .. } => {
                doc(&mut out, d, "");
                let modifier = if *editable { "editable " } else if *private { "private " } else { "" };
                let _ = write!(out, "{modifier}var {}: {}", name(n), ty.name);
                if let Some(value) = default {
                    let _ = write!(out, " = {}", expr(value));
                }
                out.push('\n');
            }
            Item::Default { path, value, .. } => {
                let _ = writeln!(out, "default {} = {}", path.iter().map(|p| name(p)).collect::<Vec<_>>().join("."), expr(value));
            }
            Item::Dispatcher { name: n, params, doc: d, .. } => {
                doc(&mut out, d, "");
                let _ = writeln!(out, "dispatcher {}({})", name(n), param_list(params));
            }
            Item::Component { name: n, class, fields, .. } => {
                let _ = writeln!(out, "component {}: {}{}", name(n), class.name, fields_list(fields));
            }
            Item::Widget { .. } => {
                // Widgets are written as a tree, from the ones with no parent.
                if let Item::Widget { parent: None, .. } = item {
                    widget(&mut out, file, item, "");
                }
            }
            Item::Fn { name: n, params, ret, body, doc: d, access, pure, .. } => {
                doc(&mut out, d, "");
                let access = match access {
                    Access::Public => "",
                    Access::Protected => "protected ",
                    Access::Private => "private ",
                };
                let pure = match pure {
                    Purity::Impure => "",
                    Purity::Pure => "pure ",
                    Purity::PureMut => "pure mut ",
                };
                let _ = write!(out, "{access}{pure}fn {}({})", function_name(n), param_list(params));
                if let Some(ret) = ret {
                    let _ = write!(out, " -> {}", ret.name);
                }
                out.push_str(" {\n");
                block(&mut out, body, INDENT);
                out.push_str("}\n");
            }
            Item::Event { name: n, params, body, doc: d, .. } => {
                doc(&mut out, d, "");
                let _ = write!(out, "event {}", function_name(n));
                if !params.is_empty() {
                    let _ = write!(out, "({})", param_list(params));
                }
                out.push_str(" {\n");
                block(&mut out, body, INDENT);
                out.push_str("}\n");
            }
            Item::On { component, event, params, ret, body, .. } => {
                let _ = write!(out, "on ");
                if let Some(c) = component {
                    let _ = write!(out, "{}.", name(c));
                }
                let _ = write!(out, "{}", name(event));
                if !params.is_empty() {
                    let _ = write!(out, "({})", param_list(params));
                }
                if let Some(ret) = ret {
                    let _ = write!(out, " -> {}", ret.name);
                }
                out.push_str(" {\n");
                block(&mut out, body, INDENT);
                out.push_str("}\n");
            }
        }
    }
    out
}

fn widget(out: &mut String, file: &File, item: &Item, indent: &str) {
    let Item::Widget { name: n, class, fields, slot, place, .. } = item else { return };
    let _ = write!(out, "{indent}widget {}: {}{}", name(n), class.name, fields_list(fields));
    if let Some([x, y, w, h]) = place {
        let _ = write!(out, " at {x} {y} size {w} {h}");
    }
    if !slot.is_empty() {
        let _ = write!(out, " slot{}", fields_list(slot));
    }
    let children: Vec<&Item> = file.items.iter().filter(|i| matches!(i, Item::Widget { parent: Some(p), .. } if p == n)).collect();
    if children.is_empty() {
        out.push('\n');
        return;
    }
    out.push_str(" {\n");
    let inner = format!("{indent}{INDENT}");
    for child in children {
        widget(out, file, child, &inner);
    }
    let _ = writeln!(out, "{indent}}}");
}

fn fields_list(fields: &[(String, Expr)]) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = fields.iter().map(|(n, v)| format!("{}: {}", name(n), expr(v))).collect();
    format!("({})", parts.join(", "))
}

fn param_list(params: &[ParamDecl]) -> String {
    params
        .iter()
        .map(|p| {
            let mut s = format!("{}{}: {}{}", if p.out { "out " } else { "" }, name(&p.name), if p.by_ref { "ref " } else { "" }, p.ty.name);
            if let Some(d) = &p.default {
                let _ = write!(s, " = {}", expr(d));
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn block(out: &mut String, body: &[Stmt], indent: &str) {
    for s in body {
        stmt(out, s, indent);
    }
}

fn stmt(out: &mut String, s: &Stmt, indent: &str) {
    let inner = format!("{indent}{INDENT}");
    match s {
        Stmt::Let { name: n, ty, value, .. } => {
            let _ = match ty {
                Some(ty) => writeln!(out, "{indent}let {}: {} = {}", name(n), ty.name, expr(value)),
                None => writeln!(out, "{indent}let {} = {}", name(n), expr(value)),
            };
        }
        Stmt::Assign { target, value, .. } => {
            let _ = writeln!(out, "{indent}{} = {}", expr(target), expr(value));
        }
        Stmt::Compound { target, op, value, .. } => {
            let _ = writeln!(out, "{indent}{} {}= {}", expr(target), op.symbol(), expr(value));
        }
        Stmt::Return { value, .. } => {
            let _ = match value {
                Some(v) => writeln!(out, "{indent}return {}", expr(v)),
                None => writeln!(out, "{indent}return"),
            };
        }
        Stmt::Break { label, .. } => {
            let _ = writeln!(out, "{indent}break{}", label.as_ref().map(|l| format!(" {}", name(l))).unwrap_or_default());
        }
        Stmt::Continue { label, .. } => {
            let _ = writeln!(out, "{indent}continue{}", label.as_ref().map(|l| format!(" {}", name(l))).unwrap_or_default());
        }
        Stmt::Labelled { label, looped, .. } => {
            // The label, then the loop's own line after it.
            let mut inner = String::new();
            stmt(&mut inner, looped, indent);
            let _ = write!(out, "{indent}{}: {}", name(label), inner.trim_start());
        }
        Stmt::Expr(e) => {
            let _ = writeln!(out, "{indent}{}", expr(e));
        }
        Stmt::While { cond, body, .. } => {
            let _ = writeln!(out, "{indent}while {} {{", expr(cond));
            block(out, body, &inner);
            let _ = writeln!(out, "{indent}}}");
        }
        Stmt::For { names, items, body, .. } => {
            let _ = writeln!(out, "{indent}for {} in {} {{", names.iter().map(|n| name(n)).collect::<Vec<_>>().join(", "), expr(items));
            block(out, body, &inner);
            let _ = writeln!(out, "{indent}}}");
        }
        Stmt::Count { name: n, first, last, body, .. } => {
            let _ = writeln!(out, "{indent}for {} in {}..{} {{", name(n), expr(first), expr(last));
            block(out, body, &inner);
            let _ = writeln!(out, "{indent}}}");
        }
        Stmt::If { cond, then, otherwise, .. } => {
            let _ = write!(out, "{indent}if {} {{\n", expr(cond));
            block(out, then, &inner);
            let mut rest = otherwise;
            loop {
                if rest.is_empty() {
                    let _ = writeln!(out, "{indent}}}");
                    break;
                }
                // An else holding only another if is written `else if`.
                if let [Stmt::If { cond, then, otherwise, .. }] = rest.as_slice() {
                    let _ = writeln!(out, "{indent}}} else if {} {{", expr(cond));
                    block(out, then, &inner);
                    rest = otherwise;
                    continue;
                }
                let _ = writeln!(out, "{indent}}} else {{");
                block(out, rest, &inner);
                let _ = writeln!(out, "{indent}}}");
                break;
            }
        }
        Stmt::Match { value, arms, otherwise, .. } => {
            let _ = writeln!(out, "{indent}match {} {{", expr(value));
            for (values, body) in arms {
                let vs: Vec<String> = values.iter().map(expr).collect();
                let _ = writeln!(out, "{inner}{} {{", vs.join(", "));
                block(out, body, &format!("{inner}{INDENT}"));
                let _ = writeln!(out, "{inner}}}");
            }
            if let Some(body) = otherwise {
                let _ = writeln!(out, "{inner}else {{");
                block(out, body, &format!("{inner}{INDENT}"));
                let _ = writeln!(out, "{inner}}}");
            }
            let _ = writeln!(out, "{indent}}}");
        }
    }
}

/// How tightly an expression binds: an operand that binds looser than its operator is wrapped in
/// parentheses.
fn precedence(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Binary(op, _, _) => binary_precedence(*op),
        ExprKind::Unary(..) => 9,
        // An if or match expression reads to the end, so it is wrapped wherever it is an operand.
        ExprKind::If(..) | ExprKind::Match(..) => 0,
        _ => 10,
    }
}

fn binary_precedence(op: BinaryOp) -> u8 {
    BINARY_OPERATORS.iter().find(|(_, o, _, _)| *o == op).map(|(_, _, p, _)| *p).expect("every operator is in BINARY_OPERATORS")
}

fn operand(e: &Expr, min: u8) -> String {
    if precedence(e) < min {
        format!("({})", expr(e))
    } else {
        expr(e)
    }
}

pub fn expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(f) => float(*f),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Str(s) => string(s),
        ExprKind::Null => "none".to_string(),
        // self and super are this object, never a name.
        ExprKind::Name(n) if SELF_WORDS.contains(&n.as_str()) => n.clone(),
        ExprKind::Name(n) => name(n),
        ExprKind::Path(p) => p.clone(),
        ExprKind::Unary(op, inner) => {
            let sym = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            format!("{sym}{}", operand(inner, 9))
        }
        ExprKind::Binary(op, l, r) => {
            let p = binary_precedence(*op);
            // Left to right: an operand of the same precedence on the right is wrapped.
            format!("{} {} {}", operand(l, p), op.symbol(), operand(r, p + 1))
        }
        ExprKind::Call(n, args, quoted) => {
            let called = if *quoted { function_name(n) } else { name(n) };
            format!("{called}({})", args.iter().map(expr).collect::<Vec<_>>().join(", "))
        }
        ExprKind::Make(n, fields) => format!("{}{}", name(n), if fields.is_empty() { "()".to_string() } else { fields_list(fields) }),
        ExprKind::Index(a, i) => format!("{}[{}]", operand(a, 10), expr(i)),
        ExprKind::EmptyArray => "[]".to_string(),
        ExprKind::ArrayLiteral(items) => format!("[{}]", items.iter().map(expr).collect::<Vec<_>>().join(", ")),
        ExprKind::MapLiteral(entries) => {
            let parts: Vec<String> = entries.iter().map(|(k, v)| format!("{}: {}", expr(k), expr(v))).collect();
            format!("{{{}}}", parts.join(", "))
        }
        ExprKind::Member(obj, n) => format!("{}.{}", operand(obj, 10), name(n)),
        ExprKind::Method(obj, n, args) => {
            format!("{}.{}({})", operand(obj, 10), name(n), args.iter().map(expr).collect::<Vec<_>>().join(", "))
        }
        ExprKind::If(c, a, b) => {
            let mut s = format!("if {} {{ {} }} else ", expr(c), expr(a));
            if let ExprKind::If(..) = b.kind {
                s.push_str(&expr(b));
            } else {
                let _ = write!(s, "{{ {} }}", expr(b));
            }
            s
        }
        ExprKind::Match(v, arms, otherwise) => {
            let mut s = format!("match {} {{ ", expr(v));
            for (values, value) in arms {
                let vs: Vec<String> = values.iter().map(expr).collect();
                let _ = write!(s, "{} {{ {} }} ", vs.join(", "), expr(value));
            }
            let _ = write!(s, "else {{ {} }} }}", expr(otherwise));
            s
        }
    }
}

/// A float as loom reads one back: always with a point, so it is never taken for a whole number.
fn float(f: f64) -> String {
    let s = format!("{f:?}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{s}.0")
    }
}

pub fn string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax;

    /// Each source reads back as the same tree after printing.
    fn round_trip(text: &str) {
        let tree = syntax::parse_source(text).unwrap();
        let printed = source(&tree);
        let again = syntax::parse_source(&printed).unwrap_or_else(|e| panic!("{e}\n{printed}"));
        assert_eq!(strip(&source(&again)), strip(&printed), "{printed}");
    }

    fn strip(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn prints_what_it_parses() {
        round_trip(
            "blueprint BP_A : Actor at /Game/A/BP_A\n\nvar Count: int = 0\nvar Names: array<string> = [\"a\", \"b\"]\n\n\
             fn F(x: int, ys: ref array<int>) -> int {\n    let a = (x + 1) * 2 - -x\n    if a < 3 && !(x == 2) {\n        return 1\n    } else if a > 4 {\n        return 2\n    }\n    return a\n}\n\n\
             on ReceiveBeginPlay {\n    Count = Count + 1\n    KismetSystemLibrary.PrintString(\"hi\\n\")\n}\n",
        );
    }

    #[test]
    fn wraps_operands_that_bind_looser() {
        let tree = syntax::parse_source("blueprint BP_A : Actor at /Game/A/BP_A\n\nfn F(a: int, b: int, c: int) -> int {\n    return a - (b - c) * (a + b)\n}\n").unwrap();
        let printed = source(&tree);
        assert!(printed.contains("return a - (b - c) * (a + b)"), "{printed}");
    }
}
