---
title: Types
description: The types values can have, and how to write them.
sidebar:
  order: 2
---

## Plain types

`bool`, `int`, `int64`, `float`, `string`, `name` and `text` are the Blueprint types of the same
names. A `float` is a double, as Blueprint floats are. The [built-ins](/reference/builtins/) list
each one with what converts to it.

```loom
var Alive: bool = true
var Lives: int = 3
var Seed: int64 = 0
var Speed: float = 2.5
var Title: string = "Counter"
var Tag: name = "wood"
var Label: text = "Hello"
```

## Classes, structs and enums

A class, a struct or an enum is written by its name, or by its path when the name is ambiguous.
[Names](/reference/#names) says which one a name means, and how `use` picks among several.

```loom
var Target: Actor
var Where: Vector2D
var Group: ETickingGroup = ETickingGroup.TG_PostPhysics
var Other: /Script/Engine.Actor
```

A variable of a class holds an object of that class, or `none`.

An enum's value is held as a number from 0 to 255. `int(value)` gives the number, and
`Enum(number)` the value it stands for, whether the enum names that number or not.

```loom
var Group: ETickingGroup = ETickingGroup.TG_PostPhysics

fn Next() {
    Group = ETickingGroup(int(Group) + 1)
}
```

## Declaring an enum

An enum is an asset of its own, so it has a source file of its own, saved as the enum's name
beside the asset. The source is the enum alone: its name, the asset path it is saved at, then its
values in braces, one on each line. Every source in the project can use it as it would any other.

```loom program="tool"
// What a worker holds.
enum E_Tool at /Game/Types/E_Tool {
    Hammer
    // Cuts wood.
    Saw "Hand Saw"
    Chisel
}
```

```loom program="tool"
blueprint BP_Worker : Actor at /Game/Workers/BP_Worker

editable var Holding: E_Tool = E_Tool.Hammer

fn Cuts() -> bool {
    return Holding == E_Tool.Saw
}
```

A value is written by its name, as `E_Tool.Saw`, and the editor shows its name too. A string after
the name is what the editor shows instead, such as in a dropdown in the Details panel. A `//`
comment above the enum or a value becomes its tooltip.

An enum has at least one value and at most 255, each listed once, and no two are shown alike:

```loom error="Saw is listed twice" refusals="enum"
enum E_Tool at /Game/Types/E_Tool {
    Saw
    Saw
}
```

```loom error="Blade and Saw would both be shown as" refusals="enum"
enum E_Tool at /Game/Types/E_Tool {
    Saw
    Blade "Saw"
}
```

```loom error="E_Tool has no values" refusals="enum"
enum E_Tool at /Game/Types/E_Tool {
}
```

## Declaring a struct

A struct is an asset of its own too, with a source file of its own. The source is the struct
alone: its name, the asset path it is saved at, then its fields in braces, one on each line, each
a name and a type, as a variable's. A field starts out zero, or at the value after `=`.

```loom program="cell"
// A place on the grid.
struct S_Cell at /Game/Types/S_Cell {
    X: int
    // How far up.
    Y: int = 1
    Visited: bool
    Next: array<Vector2D>
}
```

```loom program="cell"
blueprint BP_Grid : Actor at /Game/Grid/BP_Grid

var Start: S_Cell = S_Cell(X: 2)

fn Up() -> S_Cell {
    return S_Cell(X: Start.X, Y: Start.Y + 1)
}
```

A struct is made, read and changed as [any other](/reference/expressions/#structs), and a `//`
comment above the struct or a field becomes its tooltip. A struct has at least one field, each
declared once, and none of its fields is the struct itself:

```loom error="X is declared twice" refusals="struct"
struct S_Cell at /Game/Types/S_Cell {
    X: int
    X: float
}
```

```loom error="which would hold itself" refusals="struct"
struct S_Cell at /Game/Types/S_Cell {
    Next: S_Cell
}
```

```loom error="S_Cell has no fields" refusals="struct"
struct S_Cell at /Game/Types/S_Cell {
}
```

## Interfaces

A variable of an interface holds an object that implements it, or `none`. A value of the
interface comes from [a cast](/reference/expressions/#calls-through-an-interface) to it.

```loom
var Easing: MovieSceneEasingFunction

fn Pick(thing: Object) -> bool {
    Easing = cast(thing, MovieSceneEasingFunction)
    return Easing != none
}
```

## Delegates

A delegate is a function and the object to call it on, which the engine calls later, as Set Timer
by Event does. Its type is named after its signature, what it passes the function:
`TimerDynamicDelegate` for Set Timer by Event's, which C++ calls `FTimerDynamicDelegate`. A function
named where one is expected is a [delegate to it](/reference/events/#functions-as-delegates).

A function takes and returns one as Blueprint allows, to pass it on:

```loom
var Ticks: int = 0

fn Count() {
    Ticks += 1
}

fn Every(seconds: float, handler: TimerDynamicDelegate) {
    KismetSystemLibrary.K2_SetTimerDelegate(handler, seconds, true)
}

fn Counter() -> TimerDynamicDelegate {
    return Count
}

on ReceiveBeginPlay {
    Every(0.5, Count)
    Every(2.0, Counter())
}
```

A function of an [interface](/reference/#declaring-an-interface) takes and returns one too, and a
[dispatcher](/reference/members/#dispatchers) passes one as it passes anything else. Blueprint
keeps no delegate anywhere else, and has no way to call one:

```loom error="Blueprint has no delegate variables" refusals="delegate"
blueprint BP_Timer : Actor at /Game/Timers/BP_Timer

var Handler: TimerDynamicDelegate
```

```loom error="Blueprint has no delegate variables" refusals="delegate"
blueprint BP_Timer : Actor at /Game/Timers/BP_Timer

fn Count() {
}

fn Start() {
    let handler: TimerDynamicDelegate = Count
}
```

```loom error="Blueprint cannot call a delegate" refusals="delegate"
blueprint BP_Timer : Actor at /Game/Timers/BP_Timer

fn Run(handler: TimerDynamicDelegate) {
    handler()
}
```

Nor can a struct's field be a delegate:

```loom error="the struct editor refuses a delegate field"
struct S_Timer at /Game/Types/S_Timer {
    Handler: TimerDynamicDelegate
}
```

## Containers

`array<T>`, `set<T>` and `map<K, V>` are Blueprint arrays, sets and maps. Blueprints have no
nested containers, so none of them can hold another.

```loom
var Scores: array<int>
var Seen: set<name>
var Stock: map<string, int>
```

```loom error="no nested containers"
var Grid: array<array<int>>
```

## Classes as values

`class<T>` holds a class that is `T` or a subclass of it.

```loom
var Kind: class<Actor> = /Script/Engine.Actor
```

## Soft references

`soft<T>` refers to an asset by its path without loading it, and `soft<class<T>>` does the same for
a class. `load()` loads what it refers to and waits for it, and `valid()` says whether it refers to
anything.

```loom
var Engine: soft<class<Actor>> = /Script/Engine.Actor

fn Spawnable() -> bool {
    return Engine.valid()
}
```
