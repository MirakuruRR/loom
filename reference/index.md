---
title: Language basics
description: What a Loom source file is, and how Loom turns it into an asset.
sidebar:
  order: 1
---

A Loom source file describes one asset: a Blueprint, an enum, a struct or an interface. A
Blueprint's source gives its parent class, where it is saved, and its members. Loom compiles it to
the editor operations that build that Blueprint, and the editor plugin applies them, so what you
get is an ordinary Blueprint asset that the editor can open.

Every example in this reference is compiled by Loom's tests against the engine's own classes, so
each one compiles as shown. An example with no header line is compiled with
`blueprint Example : Actor at /Game/Example` in front of it.

## The header

A source starts with its header line, then any `use` and `implements` lines. Everything else comes
after them.

```loom-outline
blueprint BP_Door : Actor at /Game/Door/BP_Door
use /Game/Libs/Stock_1_0
implements BPI_Interactable
implements BPI_Lockable
// then variables, functions, events…
```

The header line names the Blueprint, its parent class and the asset path it is saved at. It starts
with `blueprint`, or `editor blueprint` for a Blueprint that only runs in the editor. `use` is
covered in [Using a folder](#using-a-folder), and `implements` in
[Implementing an interface](#implementing-an-interface).

An enum or a struct starts with `enum` or `struct` instead, and what it holds follows in braces; see
[Types](/reference/types/#declaring-an-enum). An interface starts with `interface`, covered below
in [Declaring an interface](#declaring-an-interface).

### The kind of Blueprint

The parent decides what kind of Blueprint a source makes, and what it can hold. These are the
common kinds; any class Blueprints can extend works as a parent.

| Kind | Parent you write | What it adds | Can't have |
| --- | --- | --- | --- |
| **Actor** `blueprint` [Members](/reference/members/) | `Actor`, or any actor class such as `Pawn` or `Character` | Components, and events such as BeginPlay | Widgets |
| **Widget** `blueprint` [Widgets](/reference/widgets/) | `UserWidget`, or a widget class | Placed widgets | Components |
| **Editor Blueprint** `editor blueprint` [Editor Blueprints](/reference/editor/) | A class that only runs in the editor, such as `EditorUtilityObject` | The editor's own classes and functions, and subsystems | What its parent can't have |
| **Function library** `blueprint` [Function libraries](/reference/libraries/) | `BlueprintFunctionLibrary` | Functions any Blueprint can call | Variables, dispatchers, events, components, widgets, `implements`, `self` |
| **Any other class** `blueprint` [Members](/reference/members/) | Any class Blueprints can extend, such as `Object` | Nothing more | Components, widgets |

The header has to match the parent. A parent that only runs in the editor needs `editor blueprint`,
and `editor blueprint` with a parent that runs in the game is refused. An editor utility widget is
both an editor Blueprint and a widget.

### Naming the parent

The parent can be written by name, or by path when a name is ambiguous. The asset's name must be
the Blueprint's name.

```loom
blueprint BP_Counter : /Script/Engine.Actor at /Game/Counter/BP_Counter
```

A path saved somewhere other than the Blueprint's name is refused:

```loom error="must be saved as"
blueprint BP_Counter : Actor at /Game/Counter/BP_Other
```

## Programs

Several files compiled together are one program. Each can use the others' Blueprints, their
variables, widgets and functions, whichever file declares them. Loom applies each Blueprint after
the ones it uses.

```loom program="shop"
blueprint BP_Till : Actor at /Game/Shop/BP_Till

var Total: int = 0

fn Ring(price: int) {
    Total = Total + price
}
```

```loom program="shop"
blueprint BP_Customer : Actor at /Game/Shop/BP_Customer

var Till: BP_Till

fn Buy() {
    Till.Ring(5)
}
```

## Words

A few words start a statement or a value, so they are never names: `break`, `continue`, `else`,
`for`, `if`, `let`, `match`, `return`, `while`, `true`, `false`, `none`, `self` and `super`.

```loom error="if is a reserved word"
var if: int
```

Every other word the language uses, such as `var`, `fn`, `out` or `in`, means something only where
a source expects it. Anywhere else it is a name like any other:

```loom
var in: int

fn Total(out: int) -> int {
    return out + in
}
```

A name in backticks is a name even when it is a reserved word or holds spaces, so a source can
name any member, such as a function a Blueprint made in the editor calls `Add Stock`. `self` and
`super` are the exception: they are always this object, so nothing can be declared with either.

```loom
var `if`: int

fn `Add Stock`(amount: int) {
    `if` += amount
}

fn Restock() {
    `Add Stock`(5)
}
```

## Names

A class, struct or enum written by its name, such as `BP_Till` or `Vector2D`, means:

1. the one of that name in the source's own folder, when there is one. Only that folder counts,
   not the folders inside it.
2. Otherwise, the one under the first folder the source [uses](#using-a-folder) that has one.
3. Otherwise, the only one of that name in the project.

Two of that name at any step is an error, which lists their paths. Writing a path instead of the
name always means exactly that one. So in a project with a `BP_Stock` in two folders, a source
elsewhere can't use the name alone:

```loom program="ambiguous"
blueprint BP_Stock : Actor at /Game/Stock/BP_Stock
```

```loom program="ambiguous"
blueprint BP_Stock : Actor at /Game/Old/BP_Stock
```

```loom program="ambiguous" error="BP_Stock is ambiguous; write its path"
blueprint BP_Shop : Actor at /Game/Shop/BP_Shop

var Shelf: BP_Stock
```

It can write the path, `var Shelf: /Game/Stock/BP_Stock.BP_Stock_C`, or `use` the folder.

## Using a folder

`use` names a folder a source takes short names from, such as a shared library's. It is how a
project holds two versions of a library, the released one and the next, with each source saying
which it means in one line, and moving to another version by changing that line:

```loom program="library"
blueprint BP_Stock : Actor at /Game/Libs/Stock_1_0/BP_Stock

var Count: int = 0
```

```loom program="library"
blueprint BP_Stock : Actor at /Game/Libs/Stock_2_0/BP_Stock

var Count: int = 0
var Reserved: int = 0
```

```loom program="library"
blueprint BP_Shop : Actor at /Game/Shop/BP_Shop
use /Game/Libs/Stock_1_0

var Shelf: BP_Stock
```

- `use` comes straight after the header, before the Blueprint's other items, and a source may use
  several folders. When more than one has a type of the name, the first written wins.
- A used folder holds the folders inside it too, such as a library's `UI/`.
- It covers classes, structs and enums alike.
- It applies only to the source that says it.
- What's in the folder needn't be built from sources in the program. A library installed in the
  project as built Blueprints is used the same way.

A folder with nothing under it is an error, so a mistyped folder or a version the project doesn't
have is caught rather than letting a name mean another version:

```loom error="nothing a source can use is under /Game/Libs/Stock_3_0"
blueprint BP_Shop : Actor at /Game/Shop/BP_Shop
use /Game/Libs/Stock_3_0
```

`use` takes a folder, not an asset:

```loom error="use takes a folder"
blueprint BP_Shop : Actor at /Game/Shop/BP_Shop
use /Game/Libs/Stock_1_0/BP_Stock.BP_Stock_C
```

## Implementing an interface

`implements` names an interface the Blueprint implements, such as one of the project's Blueprint
Interfaces, as the Blueprint's Class Settings list them. It comes after the header and any `use`,
one interface to a line, each named as any class is. The Blueprint's
[handlers](/reference/events/#an-interfaces-functions) implement the interface's functions.

```loom
blueprint BP_Curve : Actor at /Game/Curves/BP_Curve
implements MovieSceneEasingFunction
implements /Script/DataflowSimulation.DataflowSimulationActor

on OnEvaluate(t: float) -> float {
    return t * t
}
```

A Blueprint implements what its parent implements already, so it doesn't name those again.
Unreal would let it, but would replace the parent's functions that return a value with empty
ones.

```loom program="curves"
blueprint BP_Curve : Actor at /Game/Curves/BP_Curve
implements MovieSceneEasingFunction
```

```loom program="curves" error="already implements MovieSceneEasingFunction"
blueprint BP_Steep : BP_Curve at /Game/Curves/BP_Steep
implements MovieSceneEasingFunction
```

Only an interface can be implemented, once, and one that Blueprints can't implement is refused:

```loom error="Actor is not an interface" refusals="interface"
blueprint BP_Curve : Actor at /Game/Curves/BP_Curve
implements Actor
```

```loom error="MovieSceneEasingFunction is implemented twice" refusals="interface"
blueprint BP_Curve : Actor at /Game/Curves/BP_Curve
implements MovieSceneEasingFunction
implements MovieSceneEasingFunction
```

```loom error="an interface a Blueprint cannot implement" refusals="interface"
blueprint BP_Curve : Actor at /Game/Curves/BP_Curve
implements VisualLoggerDebugSnapshotInterface
```

## Declaring an interface

A Blueprint Interface has a source file of its own. The source is the interface alone: its name,
the asset path it is saved at, then its functions in braces, one on each line, each a signature
with no body. A `//` comment above a function becomes its tooltip.

```loom program="usable"
// Something a player can use.
interface BPI_Usable at /Game/Interfaces/BPI_Usable {
    fn Use(by: Actor)
    fn CanUse(by: Actor) -> bool
}
```

```loom program="usable"
blueprint BP_Door : Actor at /Game/Door/BP_Door
implements BPI_Usable

var Open: bool = false

on Use(by: Actor) {
    Open = !Open
}

on CanUse(by: Actor) -> bool {
    return true
}
```

A Blueprint implements it as it would any other interface. A function that returns nothing is
handled as an event, and one that returns a value or has an `out` parameter as a function, whose
handler sets each output by its name, as a function sets its own:

```loom program="counter"
interface BPI_Counter at /Game/Interfaces/BPI_Counter {
    fn Count(of: string, out found: bool) -> int
}
```

```loom program="counter"
blueprint BP_Shelf : Actor at /Game/Shelf/BP_Shelf
implements BPI_Counter

var Stock: map<string, int> = {"wood": 3}

on Count(of: string, out found: bool) -> int {
    found = Stock.Contains(of)
    return if found { Stock[of] } else { 0 }
}
```

An interface declares at least one function, each once, and none takes a `ref` parameter:

```loom error="Use is declared twice" refusals="declared interface"
interface BPI_Usable at /Game/Interfaces/BPI_Usable {
    fn Use(by: Actor)
    fn Use(by: Pawn)
}
```

```loom error="is a ref, which an interface's function cannot take yet" refusals="declared interface"
interface BPI_Usable at /Game/Interfaces/BPI_Usable {
    fn Use(count: ref int)
}
```

```loom error="BPI_Usable has no functions" refusals="declared interface"
interface BPI_Usable at /Game/Interfaces/BPI_Usable {
}
```

## Comments

`//` starts a comment that runs to the end of the line. A run of comments directly above a
variable, function or dispatcher is its description, which the editor shows as its tooltip. A `//`
line with nothing after it starts a new paragraph.

```loom
// How many times Bump has run.
var Count: int = 0

// Adds one to Count.
//
// Nothing else changes.
fn Bump() {
    Count = Count + 1
}
```
