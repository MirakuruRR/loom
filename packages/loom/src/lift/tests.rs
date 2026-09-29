//! The lifter end to end: packages written by hand, as CUE4Parse writes cooked ones, lifted
//! against the test types, and the printed source checked, then compiled as any other.

use serde_json::{json, Value};

use super::input::Package;
use super::{lift, Lifted};
use crate::lower::tests::{compile_together, db};

/// The cooked folder the test assets live in, which `/Game/Shop` names in a source.
const FOLDER: &str = "Game/Content/Shop";

fn obj(kind: &str, name: &str, path: &str) -> Value {
    json!({"ObjectName": format!("{kind}'{name}'"), "ObjectPath": path})
}

fn engine_class(name: &str) -> Value {
    obj("Class", name, "/Script/Engine")
}

/// A property as a class or a function declares it. Extra keys, such as an object property's
/// class, are given as `extra`.
fn prop(ty: &str, name: &str, flags: &str, extra: Value) -> Value {
    let mut p = json!({"Type": ty, "Name": name, "PropertyFlags": flags});
    if let Some(more) = extra.as_object() {
        for (k, v) in more {
            p[k] = v.clone();
        }
    }
    p
}

fn int_prop(name: &str, flags: &str) -> Value {
    prop("IntProperty", name, flags, json!({}))
}

fn local(p: Value) -> Value {
    json!({"Token": "EX_LocalVariable", "Variable": {"Property": p}})
}

fn out_local(p: Value) -> Value {
    json!({"Token": "EX_LocalOutVariable", "Variable": {"Property": p}})
}

/// One of the Blueprint's own variables.
fn member(bp: &str, p: Value) -> Value {
    let owner = obj("BlueprintGeneratedClass", &format!("{bp}_C"), &format!("{FOLDER}/{bp}.0"));
    json!({"Token": "EX_InstanceVariable", "Variable": {"Owner": owner, "Property": p}})
}

fn int(n: i64) -> Value {
    json!({"Token": "EX_IntConst", "Value": n})
}

fn math(function: &str, args: Vec<Value>) -> Value {
    json!({"Token": "EX_CallMath", "Function": obj("Function", &format!("KismetMathLibrary:{function}"), "/Script/Engine"), "Parameters": args})
}

fn let_(at: usize, target: Value, value: Value) -> Value {
    json!({"Token": "EX_Let", "StatementIndex": at, "Variable": target, "Expression": value})
}

fn jump_if_not(at: usize, cond: Value, to: usize) -> Value {
    json!({"Token": "EX_JumpIfNot", "StatementIndex": at, "CodeOffset": to, "BooleanExpression": cond})
}

fn jump(at: usize, to: usize) -> Value {
    json!({"Token": "EX_Jump", "StatementIndex": at, "CodeOffset": to})
}

/// A function's end: its return, then the end of its script.
fn ends(at: usize) -> Vec<Value> {
    vec![json!({"Token": "EX_Return", "StatementIndex": at}), json!({"Token": "EX_EndOfScript", "StatementIndex": at + 2})]
}

fn function(name: &str, flags: &str, locals: Vec<Value>, mut code: Vec<Value>, ends_at: usize) -> Value {
    code.extend(ends(ends_at));
    json!({"Type": "Function", "Name": name, "FunctionFlags": flags, "ChildProperties": locals, "ScriptBytecode": code})
}

/// A Blueprint's package: its class with its variables, its default object with their values,
/// and its functions.
fn blueprint(name: &str, vars: Vec<Value>, defaults: Value, functions: Vec<Value>) -> Package {
    let class_name = format!("{name}_C");
    let cdo_name = format!("Default__{class_name}");
    let mut exports = vec![
        json!({
            "Type": "BlueprintGeneratedClass",
            "Name": class_name,
            "Package": format!("{FOLDER}/{name}"),
            "SuperStruct": engine_class("Actor"),
            "ChildProperties": vars,
            "ClassDefaultObject": obj(&class_name, &cdo_name, &format!("{FOLDER}/{name}.1")),
        }),
        json!({"Type": class_name, "Name": cdo_name, "Properties": defaults}),
    ];
    exports.extend(functions);
    package(exports)
}

fn package(exports: Vec<Value>) -> Package {
    Package::parse(&Value::Array(exports).to_string()).unwrap()
}

/// The source an asset lifted to, or a panic saying why it did not.
#[track_caller]
fn source_of<'l>(lifted: &'l [Lifted], path: &str) -> &'l str {
    let asset = lifted.iter().find(|l| l.path == path).unwrap_or_else(|| panic!("{path} was not lifted"));
    match &asset.source {
        Ok(text) => text,
        Err(e) => panic!("{path} failed: {e:#}"),
    }
}

/// Lifted sources must compile together against the same types they were lifted against.
#[track_caller]
fn compiles(sources: &[&str]) {
    compile_together(sources).unwrap_or_else(|e| panic!("{e}\n{}", sources.join("\n")));
}

#[test]
fn a_variable_with_a_default_and_a_function_that_branches_on_its_way_to_a_value() {
    // Price(Amount) returns twice the amount over ten, else the amount.
    let amount = int_prop("Amount", "BlueprintVisible | BlueprintReadOnly | Parm");
    let ret = int_prop("ReturnValue", "Parm | OutParm | ReturnParm");
    let over = prop("BoolProperty", "CallFunc_Greater_IntInt_ReturnValue", "", json!({}));
    let doubled = int_prop("CallFunc_Multiply_IntInt_ReturnValue", "");
    let code = vec![
        let_(0, local(over.clone()), math("Greater_IntInt", vec![local(amount.clone()), int(10)])),
        jump_if_not(10, local(over.clone()), 40),
        let_(20, local(doubled.clone()), math("Multiply_IntInt", vec![local(amount.clone()), int(2)])),
        let_(25, out_local(ret.clone()), local(doubled.clone())),
        jump(30, 50),
        let_(40, out_local(ret.clone()), local(amount.clone())),
    ];
    let price = function("Price", "FUNC_Public | FUNC_BlueprintCallable", vec![amount, ret, over, doubled], code, 50);
    let stock = int_prop("Stock", "Edit | BlueprintVisible");
    let bp = blueprint("BP_Shop", vec![stock], json!({"Stock": 5}), vec![price]);

    let lifted = lift(&db(), vec![bp]).unwrap();
    let source = source_of(&lifted, "/Game/Shop/BP_Shop");
    assert_eq!(
        source,
        "blueprint BP_Shop : Actor at /Game/Shop/BP_Shop\n\n\
         editable var Stock: int = 5\n\n\
         fn Price(Amount: int) -> int {\n    \
             let result: int = 0\n    \
             if Amount > 10 {\n        \
                 result = Amount * 2\n    \
             } else {\n        \
                 result = Amount\n    \
             }\n    \
             return result\n\
         }\n"
    );
    compiles(&[source]);
}

#[test]
fn out_parameters_keep_their_assignments() {
    // Split(Total, out Half, out Rest): each output is set once, and the second reads the first.
    let total = int_prop("Total", "BlueprintVisible | BlueprintReadOnly | Parm");
    let half = int_prop("Half", "Parm | OutParm");
    let rest = int_prop("Rest", "Parm | OutParm");
    let quotient = int_prop("CallFunc_Divide_IntInt_ReturnValue", "");
    let difference = int_prop("CallFunc_Subtract_IntInt_ReturnValue", "");
    let code = vec![
        let_(0, local(quotient.clone()), math("Divide_IntInt", vec![local(total.clone()), int(2)])),
        let_(10, out_local(half.clone()), local(quotient.clone())),
        let_(20, local(difference.clone()), math("Subtract_IntInt", vec![local(total.clone()), out_local(half.clone())])),
        let_(30, out_local(rest.clone()), local(difference.clone())),
    ];
    let split = function("Split", "FUNC_Public | FUNC_BlueprintCallable", vec![total, half, rest, quotient, difference], code, 40);
    let bp = blueprint("BP_Shop", vec![], json!({}), vec![split]);

    let lifted = lift(&db(), vec![bp]).unwrap();
    let source = source_of(&lifted, "/Game/Shop/BP_Shop");
    assert_eq!(
        source,
        "blueprint BP_Shop : Actor at /Game/Shop/BP_Shop\n\n\
         fn Split(Total: int, out Half: int, out Rest: int) {\n    \
             Half = Total / 2\n    \
             Rest = Total - Half\n\
         }\n"
    );
    compiles(&[source]);
}

#[test]
fn an_event_handler_in_the_event_graph_inlines_a_native_calls_temporary() {
    // ReceiveBeginPlay jumps into the event graph, which passes Rate through Sin and then
    // destroys the actor. The compiler's local for Sin's value is read where it is used.
    let rate = prop("DoubleProperty", "Rate", "BlueprintVisible", json!({}));
    let entry = int_prop("EntryPoint", "BlueprintVisible | BlueprintReadOnly | Parm");
    let sine = prop("DoubleProperty", "CallFunc_Sin_ReturnValue", "", json!({}));
    let graph_code = vec![
        json!({"Token": "EX_ComputedJump", "StatementIndex": 0, "CodeOffsetExpression": local(entry.clone())}),
        let_(10, local(sine.clone()), math("Sin", vec![member("BP_Shop", rate.clone())])),
        let_(30, member("BP_Shop", rate.clone()), local(sine.clone())),
        json!({"Token": "EX_VirtualFunction", "StatementIndex": 50, "Function": "K2_DestroyActor", "Parameters": []}),
    ];
    let graph = function("ExecuteUbergraph_BP_Shop", "FUNC_Final | FUNC_UbergraphFunction", vec![entry, sine], graph_code, 60);
    // The event's stub only calls the graph at the event's entry point.
    let call_graph = json!({
        "Token": "EX_LocalFinalFunction",
        "StatementIndex": 0,
        "Function": obj("Function", "BP_Shop_C:ExecuteUbergraph_BP_Shop", &format!("{FOLDER}/BP_Shop.2")),
        "Parameters": [int(10)],
    });
    let mut begin_play = function("ReceiveBeginPlay", "FUNC_Event | FUNC_Protected | FUNC_BlueprintEvent", vec![], vec![call_graph], 15);
    begin_play["SuperStruct"] = obj("Function", "Actor:ReceiveBeginPlay", "/Script/Engine");
    let bp = blueprint("BP_Shop", vec![rate], json!({"Rate": 0.5}), vec![graph, begin_play]);

    let lifted = lift(&db(), vec![bp]).unwrap();
    let source = source_of(&lifted, "/Game/Shop/BP_Shop");
    assert_eq!(
        source,
        "blueprint BP_Shop : Actor at /Game/Shop/BP_Shop\n\n\
         var Rate: float = 0.5\n\n\
         on ReceiveBeginPlay {\n    \
             Rate = KismetMathLibrary.Sin(Rate)\n    \
             K2_DestroyActor()\n\
         }\n"
    );
    compiles(&[source]);
}

#[test]
fn a_custom_event_is_lifted_as_an_event_and_may_wait() {
    // Pulse jumps into the event graph, which waits a second and then destroys the actor.
    let entry = int_prop("EntryPoint", "BlueprintVisible | BlueprintReadOnly | Parm");
    let latent = json!({
        "Token": "EX_StructConst",
        "Struct": obj("ScriptStruct", "LatentActionInfo", "/Script/Engine"),
        "Properties": [
            {"Token": "EX_SkipOffsetConst", "Value": 40},
            {"Token": "EX_IntConst", "Value": 1},
            {"Token": "EX_NameConst", "Value": "ExecuteUbergraph_BP_Shop"},
            {"Token": "EX_Self"},
        ],
    });
    let delay = json!({
        "Token": "EX_CallMath",
        "StatementIndex": 10,
        "Function": obj("Function", "KismetSystemLibrary:Delay", "/Script/Engine"),
        "Parameters": [{"Token": "EX_Self"}, {"Token": "EX_DoubleConst", "Value": 1.0}, latent],
    });
    let graph_code = vec![
        json!({"Token": "EX_ComputedJump", "StatementIndex": 0, "CodeOffsetExpression": local(entry.clone())}),
        delay,
        json!({"Token": "EX_Return", "StatementIndex": 30, "ReturnExpression": {"Token": "EX_Nothing"}}),
        json!({"Token": "EX_VirtualFunction", "StatementIndex": 40, "Function": "K2_DestroyActor", "Parameters": []}),
    ];
    let graph = function("ExecuteUbergraph_BP_Shop", "FUNC_Final | FUNC_UbergraphFunction", vec![entry], graph_code, 60);
    let call_graph = json!({
        "Token": "EX_LocalFinalFunction",
        "StatementIndex": 0,
        "Function": obj("Function", "BP_Shop_C:ExecuteUbergraph_BP_Shop", &format!("{FOLDER}/BP_Shop.2")),
        "Parameters": [int(10)],
    });
    let pulse = function("Pulse", "FUNC_Public | FUNC_BlueprintCallable | FUNC_BlueprintEvent | FUNC_Event", vec![], vec![call_graph], 15);
    let bp = blueprint("BP_Shop", vec![], json!({}), vec![graph, pulse]);

    let lifted = lift(&db(), vec![bp]).unwrap();
    let source = source_of(&lifted, "/Game/Shop/BP_Shop");
    assert_eq!(
        source,
        "blueprint BP_Shop : Actor at /Game/Shop/BP_Shop\n\n\
         event Pulse {\n    \
             KismetSystemLibrary.Delay(1.0)\n    \
             K2_DestroyActor()\n\
         }\n"
    );
    compiles(&[source]);
}

#[test]
fn a_local_one_handler_reads_first_is_the_blueprints_variable_in_every_handler() {
    // ReceiveBeginPlay sets the graph's local, and Pulse reads it before setting it, so both use
    // the Blueprint's variable: a local in ReceiveBeginPlay would leave Pulse reading nothing.
    let rate = prop("DoubleProperty", "Rate", "BlueprintVisible", json!({}));
    let entry = int_prop("EntryPoint", "BlueprintVisible | BlueprintReadOnly | Parm");
    let held = prop("DoubleProperty", "CallFunc_Sin_ReturnValue", "", json!({}));
    let graph_code = vec![
        json!({"Token": "EX_ComputedJump", "StatementIndex": 0, "CodeOffsetExpression": local(entry.clone())}),
        let_(10, local(held.clone()), math("Sin", vec![member("BP_Shop", rate.clone())])),
        jump(20, 60),
        let_(30, member("BP_Shop", rate.clone()), local(held.clone())),
        jump(40, 60),
    ];
    let graph = function("ExecuteUbergraph_BP_Shop", "FUNC_Final | FUNC_UbergraphFunction", vec![entry, held], graph_code, 60);
    let stub = |entry: i64| {
        json!({
            "Token": "EX_LocalFinalFunction",
            "StatementIndex": 0,
            "Function": obj("Function", "BP_Shop_C:ExecuteUbergraph_BP_Shop", &format!("{FOLDER}/BP_Shop.2")),
            "Parameters": [int(entry)],
        })
    };
    let mut begin_play = function("ReceiveBeginPlay", "FUNC_Event | FUNC_Protected | FUNC_BlueprintEvent", vec![], vec![stub(10)], 15);
    begin_play["SuperStruct"] = obj("Function", "Actor:ReceiveBeginPlay", "/Script/Engine");
    let pulse = function("Pulse", "FUNC_Public | FUNC_BlueprintCallable | FUNC_BlueprintEvent | FUNC_Event", vec![], vec![stub(30)], 15);
    let bp = blueprint("BP_Shop", vec![rate], json!({}), vec![graph, begin_play, pulse]);

    let lifted = lift(&db(), vec![bp]).unwrap();
    let source = source_of(&lifted, "/Game/Shop/BP_Shop");
    assert_eq!(
        source,
        "blueprint BP_Shop : Actor at /Game/Shop/BP_Shop\n\n\
         var Rate: float\n\
         private var sin: float\n\n\
         event Pulse {\n    \
             Rate = sin\n\
         }\n\n\
         on ReceiveBeginPlay {\n    \
             sin = KismetMathLibrary.Sin(Rate)\n\
         }\n"
    );
    compiles(&[source]);
}

/// BPI_Counter, a Blueprint Interface with one function: Count(of, out found) -> int.
fn counter_interface() -> Package {
    let params = vec![
        prop("StrProperty", "of", "Parm", json!({})),
        prop("BoolProperty", "found", "Parm | OutParm", json!({})),
        int_prop("ReturnValue", "Parm | OutParm | ReturnParm"),
    ];
    let count = function("Count", "FUNC_Public | FUNC_Event | FUNC_BlueprintEvent", params, vec![], 0);
    let mut bpi = blueprint("BPI_Counter", vec![], json!({}), vec![count]);
    bpi.exports[0]["SuperStruct"] = obj("Class", "Interface", "/Script/CoreUObject");
    bpi
}

#[test]
fn an_interface_its_implementation_and_a_call_through_it_are_lifted() {
    // BP_Shop implements BPI_Counter's Count, and Ask calls it on any object through the
    // interface, reading its output and its value.
    let of = prop("StrProperty", "of", "Parm", json!({}));
    let found = prop("BoolProperty", "found", "Parm | OutParm", json!({}));
    let ret = int_prop("ReturnValue", "Parm | OutParm | ReturnParm");
    let code = vec![let_(0, out_local(found.clone()), json!({"Token": "EX_True"})), let_(10, out_local(ret.clone()), int(1))];
    let count = function("Count", "FUNC_Public | FUNC_Event | FUNC_BlueprintEvent", vec![of, found, ret], code, 20);

    let thing = prop("ObjectProperty", "thing", "Parm", json!({"PropertyClass": obj("Class", "Object", "/Script/CoreUObject")}));
    let seen = prop("BoolProperty", "Seen", "BlueprintVisible", json!({}));
    let got = prop("BoolProperty", "CallFunc_Count_found", "", json!({}));
    let counted = int_prop("CallFunc_Count_ReturnValue", "");
    let interface = obj("BlueprintGeneratedClass", "BPI_Counter_C", &format!("{FOLDER}/BPI_Counter.0"));
    let call = json!({
        "Token": "EX_Let",
        "StatementIndex": 0,
        "Variable": local(counted.clone()),
        "Expression": {
            "Token": "EX_Context",
            "ObjectExpression": {"Token": "EX_InterfaceContext", "InterfaceValue": {"Token": "EX_ObjToInterfaceCast", "InterfaceClass": interface.clone(), "Target": local(thing.clone())}},
            "ContextExpression": {
                "Token": "EX_FinalFunction",
                "Function": obj("Function", "BPI_Counter_C:Count", &format!("{FOLDER}/BPI_Counter.2")),
                "Parameters": [{"Token": "EX_StringConst", "Value": "wood"}, local(got.clone())],
            },
        },
    });
    let code = vec![call, let_(30, member("BP_Shop", seen.clone()), local(got.clone()))];
    let ask = function("Ask", "FUNC_Public | FUNC_BlueprintCallable", vec![thing, got, counted], code, 40);

    let mut bp = blueprint("BP_Shop", vec![seen], json!({}), vec![count, ask]);
    bp.exports[0]["Interfaces"] = json!([{"Class": interface, "PointerOffset": 0, "bImplementedByK2": true}]);

    let lifted = lift(&db(), vec![counter_interface(), bp]).unwrap();
    let bpi = source_of(&lifted, "/Game/Shop/BPI_Counter");
    assert_eq!(bpi, "interface BPI_Counter at /Game/Shop/BPI_Counter {\n    fn Count(of: string, out found: bool) -> int\n}\n");
    let shop = source_of(&lifted, "/Game/Shop/BP_Shop");
    assert_eq!(
        shop,
        "blueprint BP_Shop : Actor at /Game/Shop/BP_Shop\n\
         implements BPI_Counter\n\n\
         var Seen: bool\n\n\
         fn Ask(thing: Object) {\n    \
             let count2 = cast(thing, BPI_Counter).Count(\"wood\")\n    \
             Seen = count2.found\n\
         }\n\n\
         on Count(of: string, out found: bool) -> int {\n    \
             found = true\n    \
             let result: int = 1\n    \
             return result\n\
         }\n"
    );
    compiles(&[bpi, shop]);
}

#[test]
fn an_interfaces_event_implemented_in_the_event_graph_is_its_handler() {
    let use_ = function("Use", "FUNC_Public | FUNC_Event | FUNC_BlueprintEvent", vec![], vec![], 0);
    let mut bpi = blueprint("BPI_Usable", vec![], json!({}), vec![use_]);
    bpi.exports[0]["SuperStruct"] = obj("Class", "Interface", "/Script/CoreUObject");

    let entry = int_prop("EntryPoint", "BlueprintVisible | BlueprintReadOnly | Parm");
    let graph_code = vec![
        json!({"Token": "EX_ComputedJump", "StatementIndex": 0, "CodeOffsetExpression": local(entry.clone())}),
        json!({"Token": "EX_VirtualFunction", "StatementIndex": 10, "Function": "K2_DestroyActor", "Parameters": []}),
    ];
    let graph = function("ExecuteUbergraph_BP_Shop", "FUNC_Final | FUNC_UbergraphFunction", vec![entry], graph_code, 20);
    let call_graph = json!({
        "Token": "EX_LocalFinalFunction",
        "StatementIndex": 0,
        "Function": obj("Function", "BP_Shop_C:ExecuteUbergraph_BP_Shop", &format!("{FOLDER}/BP_Shop.2")),
        "Parameters": [int(10)],
    });
    let stub = function("Use", "FUNC_Public | FUNC_Event | FUNC_BlueprintEvent", vec![], vec![call_graph], 15);
    let mut bp = blueprint("BP_Shop", vec![], json!({}), vec![graph, stub]);
    let interface = obj("BlueprintGeneratedClass", "BPI_Usable_C", &format!("{FOLDER}/BPI_Usable.0"));
    bp.exports[0]["Interfaces"] = json!([{"Class": interface, "PointerOffset": 0, "bImplementedByK2": true}]);

    let lifted = lift(&db(), vec![bpi, bp]).unwrap();
    let bpi = source_of(&lifted, "/Game/Shop/BPI_Usable");
    let shop = source_of(&lifted, "/Game/Shop/BP_Shop");
    assert_eq!(shop, "blueprint BP_Shop : Actor at /Game/Shop/BP_Shop\nimplements BPI_Usable\n\non Use {\n    K2_DestroyActor()\n}\n");
    compiles(&[bpi, shop]);
}

/// A user-defined struct's field, whose cooked name carries its index and a guid.
fn field(ty: &str, name: &str, index: u32) -> Value {
    prop(ty, &format!("{name}_{index}_0123456789ABCDEF0123456789ABCDEF"), "Edit | BlueprintVisible", json!({}))
}

fn stock_struct() -> Package {
    let count = field("IntProperty", "Count", 2);
    let label = field("StrProperty", "Label", 5);
    package(vec![json!({
        "Type": "UserDefinedStruct",
        "Name": "S_Stock",
        "Package": format!("{FOLDER}/S_Stock"),
        "ChildProperties": [count, label],
        "DefaultProperties": {count["Name"].as_str().unwrap(): 3, label["Name"].as_str().unwrap(): ""},
    })])
}

fn season_enum() -> Package {
    package(vec![json!({
        "Type": "UserDefinedEnum",
        "Name": "E_Season",
        "Package": format!("{FOLDER}/E_Season"),
        "Properties": {"DisplayNameMap": [
            {"Key": "NewEnumerator0", "Value": {"SourceString": "Spring"}},
            {"Key": "NewEnumerator1", "Value": {"SourceString": "Summer"}},
        ]},
        "Names": {"E_Season::NewEnumerator0": 0, "E_Season::NewEnumerator1": 1, "E_Season::E_Season_MAX": 2},
    })])
}

#[test]
fn a_struct_and_an_enum_lift_and_a_blueprint_uses_them() {
    let season = prop("ByteProperty", "Season", "BlueprintVisible", json!({"Enum": obj("UserDefinedEnum", "E_Season", &format!("{FOLDER}/E_Season.0"))}));
    let stock = prop("StructProperty", "Stock", "BlueprintVisible", json!({"Struct": obj("UserDefinedStruct", "S_Stock", &format!("{FOLDER}/S_Stock.0"))}));
    let defaults = json!({"Season": "E_Season::NewEnumerator1", "Stock": {"Count_2_0123456789ABCDEF0123456789ABCDEF": 7, "Label_5_0123456789ABCDEF0123456789ABCDEF": ""}});
    let bp = blueprint("BP_Shop", vec![season, stock], defaults, vec![]);

    let lifted = lift(&db(), vec![bp, stock_struct(), season_enum()]).unwrap();
    let structure = source_of(&lifted, "/Game/Shop/S_Stock");
    let enumeration = source_of(&lifted, "/Game/Shop/E_Season");
    let shop = source_of(&lifted, "/Game/Shop/BP_Shop");
    assert_eq!(structure, "struct S_Stock at /Game/Shop/S_Stock {\n    Count: int = 3\n    Label: string\n}\n");
    assert_eq!(enumeration, "enum E_Season at /Game/Shop/E_Season {\n    Spring\n    Summer\n}\n");
    assert_eq!(
        shop,
        "blueprint BP_Shop : Actor at /Game/Shop/BP_Shop\n\n\
         var Season: E_Season = E_Season.Summer\n\
         var Stock: S_Stock = S_Stock(Count: 7)\n"
    );
    compiles(&[shop, structure, enumeration]);
}

#[test]
fn a_native_type_the_project_lacks_fails_only_its_own_asset() {
    // The Blueprint holds a widget, from a module the test types do not have.
    let owner = prop("ObjectProperty", "Owner", "BlueprintVisible", json!({"PropertyClass": obj("Class", "Widget", "/Script/UMG")}));
    let bp = blueprint("BP_Shop", vec![owner], json!({}), vec![]);

    let lifted = lift(&db(), vec![bp, season_enum()]).unwrap();
    let failed = lifted.iter().find(|l| l.path == "/Game/Shop/BP_Shop").unwrap();
    let err = format!("{:#}", failed.source.as_ref().unwrap_err());
    assert!(err.contains("lifting /Game/Shop/BP_Shop"), "{err}");
    assert!(err.contains("variable Owner"), "{err}");
    assert!(err.contains("/Script/UMG.Widget"), "{err}");
    let enumeration = source_of(&lifted, "/Game/Shop/E_Season");
    compiles(&[enumeration]);
}

/// What lifting a Blueprint gave, which must be a failure naming a type the project lacks.
#[track_caller]
fn fails_for_lacking(bp: Package, path: &str) {
    let lifted = lift(&db(), vec![bp]).unwrap();
    let err = format!("{:#}", lifted[0].source.as_ref().unwrap_err());
    assert!(err.contains(&format!("the project lacks it: {path}, which the project's types do not have")), "{err}");
    let refused = crate::lift::Refused::of(lifted[0].source.as_ref().unwrap_err()).expect("a refusal");
    assert_eq!(refused.kind, crate::lift::Refusal::Missing);
}

#[test]
fn a_native_type_the_project_lacks_fails_wherever_it_is_used() {
    let widget = || obj("Class", "Widget", "/Script/UMG");

    // As the parent.
    let mut bp = blueprint("BP_Shop", vec![], json!({}), vec![]);
    bp.exports[0]["SuperStruct"] = widget();
    fails_for_lacking(bp, "/Script/UMG.Widget");

    // As the class of a cast.
    let seen = prop("ObjectProperty", "Seen", "BlueprintVisible", json!({"PropertyClass": engine_class("Actor")}));
    let cast = json!({"Token": "EX_DynamicCast", "ClassPtr": widget(), "Target": member("BP_Shop", seen.clone())});
    let code = vec![let_(0, member("BP_Shop", seen.clone()), cast)];
    let bp = blueprint("BP_Shop", vec![seen], json!({}), vec![function("Look", "FUNC_Public | FUNC_BlueprintCallable", vec![], code, 20)]);
    fails_for_lacking(bp, "/Script/UMG.Widget");

    // As a struct value passed straight to a call, with no variable of its type.
    let pair = json!({"Token": "EX_StructConst", "Struct": obj("ScriptStruct", "Pair", "/Script/Missing"), "Properties": []});
    let rate = prop("DoubleProperty", "Rate", "BlueprintVisible", json!({}));
    let code = vec![let_(0, member("BP_Shop", rate.clone()), math("Sin", vec![pair]))];
    let bp = blueprint("BP_Shop", vec![rate], json!({}), vec![function("Go", "FUNC_Public | FUNC_BlueprintCallable", vec![], code, 20)]);
    fails_for_lacking(bp, "/Script/Missing.Pair");

    // As the class a function is called on.
    let call = json!({"Token": "EX_CallMath", "StatementIndex": 0, "Function": obj("Function", "MissingLibrary:Go", "/Script/Missing"), "Parameters": []});
    let bp = blueprint("BP_Shop", vec![], json!({}), vec![function("Go", "FUNC_Public | FUNC_BlueprintCallable", vec![], vec![call], 20)]);
    fails_for_lacking(bp, "/Script/Missing.MissingLibrary");
}

#[test]
fn names_keep_their_spelling_in_backticks_where_they_need_them() {
    // A variable called if, a function with spaces in its name, and a parameter called in: each
    // is written as the asset has it, since other assets find them by those names.
    let reserved = int_prop("if", "BlueprintVisible");
    let amount = int_prop("in", "BlueprintVisible | BlueprintReadOnly | Parm");
    let sum = int_prop("CallFunc_Add_IntInt_ReturnValue", "");
    let code = vec![
        let_(0, local(sum.clone()), math("Add_IntInt", vec![member("BP_Shop", reserved.clone()), local(amount.clone())])),
        let_(10, member("BP_Shop", reserved.clone()), local(sum.clone())),
    ];
    let scale = function("Scale Stock", "FUNC_Public | FUNC_BlueprintCallable", vec![amount, sum], code, 20);
    let bp = blueprint("BP_Shop", vec![reserved], json!({"if": 3}), vec![scale]);

    let lifted = lift(&db(), vec![bp]).unwrap();
    let source = source_of(&lifted, "/Game/Shop/BP_Shop");
    assert_eq!(
        source,
        "blueprint BP_Shop : Actor at /Game/Shop/BP_Shop\n\n\
         var `if`: int = 3\n\n\
         fn `Scale Stock`(in: int) {\n    \
             `if` = `if` + in\n\
         }\n"
    );
    compiles(&[source]);
}

#[test]
fn a_name_no_source_can_write_fails_its_asset() {
    let odd = int_prop("a`b", "BlueprintVisible");
    let lifted = lift(&db(), vec![blueprint("BP_Shop", vec![odd], json!({}), vec![])]).unwrap();
    let err = format!("{:#}", lifted[0].source.as_ref().unwrap_err());
    assert!(err.contains("loom cannot express: the name \"a`b\""), "{err}");
    let refused = crate::lift::Refused::of(lifted[0].source.as_ref().unwrap_err()).expect("a refusal");
    assert_eq!(refused.kind, crate::lift::Refusal::Inexpressible);
}
