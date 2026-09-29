---
title: Members
description: The variables, functions, dispatchers and components a Blueprint holds, and their settings.
sidebar:
  order: 3
---

## Variables

`var Name: Type` declares a variable of the Blueprint. `= value` gives its default, which must be a
literal.

```loom
var Count: int
var Speed: float = 1.5
var Names: array<string> = ["a", "b"]
var Stock: map<string, int> = {"wood": 3, "stone": 4}
var Empty: map<string, int> = {}
var Tint: LinearColor = "#ff8800"
var Start: Vector2D = Vector2D(X: 1.0, Y: 2.0)
```

A colour is written as a `"#rrggbb"` or `"#rrggbbaa"` string, as a colour picker shows it.

`private var` is a variable only this Blueprint can read or set, as the editor's Private box
makes it. Other Blueprints, its children included, can't reach it.

```loom
private var Seed: int = 7
```

`editable` shows a variable in the editor's Details panel, where each instance can set it.

```loom
// Who this spawner is, in the level.
editable var Id: string = ""
```

## Functions

`fn Name(params) -> Type { ... }` declares a function. Leave out `-> Type` for one that returns
nothing.

```loom
fn Square(x: int) -> int {
    return x * x
}

fn Reset() {
}
```

A parameter marked `ref` is the caller's own variable, not a copy of its value, so the function can
change it. Its argument must be a variable.

```loom
var Items: array<int>

fn Push(xs: ref array<int>, n: int) {
    xs.Add(n)
}

fn Fill() {
    Push(Items, 1)
}
```

### Outputs

A parameter marked `out` is an output of the function, as an output added in the Details panel
is: a pin on the function's return node beside its return value, so a function can give back
several values. A call does not pass it. The body assigns it as it would a local, and may read it
back, and it starts at its type's zero on every call. Every `return`, and the end of the body,
gives back each output as it is then.

A call to a function with outputs gives them as its outputs, read by name after `let`, as a call to
an [engine function with several outputs](/reference/expressions/#calls) is. A function that
returns a value too gives it as `ReturnValue`.

```loom
fn Lookup(key: string, out found: bool, out value: int) {
    found = false
    value = 0
    if key == "a" {
        found = true
        value = 1
    }
}

fn Use() -> int {
    let r = Lookup("a")
    if r.found {
        return r.value
    }
    return 0
}

pure fn Divide(n: int, by: int, out remainder: int) -> int {
    remainder = n % by
    return n / by
}

fn Halves(n: int) -> int {
    let d = Divide(n, 2)
    return d.ReturnValue + d.remainder
}
```

An `out` parameter may come anywhere in the list, and a call passes the other parameters in their
order. Called as a statement, the function's outputs are dropped. A `pure fn` may have outputs,
and assigning them changes nothing outside it.

An argument for an output is refused, and so is an output that is a `ref` or has a default, since a
call neither passes nor gives it:

```loom error="which a call does not pass"
fn Lookup(key: string, out found: bool) {
    found = key == "a"
}

fn Use() {
    Lookup("a", true)
}
```

```loom error="is an output, which the function gives, so it cannot be ref"
fn Take(out xs: ref array<int>) {
}
```

```loom error="is an output, which a call does not pass, so it has no default"
fn Count(out n: int = 1) {
}
```

The outputs are read one by one, so a call with outputs cannot go where one value is:

```loom error="pick one by name"
fn Lookup(key: string, out found: bool, out value: int) {
    found = key == "a"
}

fn Use() -> bool {
    return Lookup("a")
}
```

Only a function has outputs, and an [interface's function](/reference/#declaring-an-interface),
whose handler sets them. An event's handler gives back its `-> Type` alone, and a dispatcher gives
nothing back:

```loom error="cannot be out"
on ReceiveTick(out dt: float) {
}
```

```loom error="cannot be out"
dispatcher Scored(out points: int)
```

### Defaults

A parameter can have a default, `= value`, which a call that leaves it out gives it, as a
function's input set in the Details panel does. The value is written out, as a variable's default
is. Only the last parameters can be left out, so every parameter after one with a default has one
too, and a `ref` parameter has none.

```loom
fn Hit(amount: int, times: int = 1, label: string = "hit") -> int {
    return amount * times
}

fn Combo() -> int {
    return Hit(5) + Hit(5, 3)
}
```

```loom error="comes after a parameter with a default"
fn Hit(times: int = 1, amount: int) {
}
```

### Pure functions

`pure fn` makes a pure function, as the Details panel's Pure does: its node has no exec pins, and
it runs each time its value is read. Since it runs whenever it's read, it changes nothing outside
itself. It may change its own locals, and call other pure functions.

```loom
var Width: int = 4
var Height: int = 3

pure fn Area() -> int {
    return Width * Height
}

fn Report() -> string {
    return "area " + string(Area())
}
```

A pure function that assigns a variable, changes a member's array, or calls a function that isn't
pure is refused:

```loom error="this assigns what is outside it"
var Reads: int = 0

pure fn Area() -> int {
    Reads += 1
    return 12
}
```

`pure mut fn` allows it, as Blueprint does. It makes the change again each time the value is read.

```loom
var Reads: int = 0

pure mut fn Area() -> int {
    Reads += 1
    return 12
}
```

### Private and protected

`private` before `fn` lets only this Blueprint call the function, and `protected` lets this
Blueprint and the Blueprints that derive from it call it, as the Details panel's Access Specifier
does. Either comes before `pure`: `private pure fn`.

```loom program="access"
blueprint BP_Lock : Actor at /Game/Locks/BP_Lock

var Code: int = 1234

private pure fn Matches(guess: int) -> bool {
    return guess == Code
}

protected fn Reset() {
    Code = 0
}

fn Try(guess: int) -> bool {
    return Matches(guess)
}
```

```loom program="access"
blueprint BP_TimeLock : BP_Lock at /Game/Locks/BP_TimeLock

fn Expire() {
    Reset()
}
```

Another Blueprint's call to a private function is refused:

```loom program="calls"
blueprint BP_Lock : Actor at /Game/Locks/BP_Lock

private pure fn Matches(guess: int) -> bool {
    return guess == 1234
}
```

```loom program="calls" error="Matches is private to BP_Lock"
blueprint BP_Door : Actor at /Game/Locks/BP_Door

fn Open(lock: BP_Lock) -> bool {
    return lock.Matches(1)
}
```

## Categories

`category "Name"` lists every function of the Blueprint under that category in the editor's
palette. `|` nests one category in another.

```loom
category "Tools|Counting"

fn Count() -> int {
    return 3
}
```

## Dispatchers

`dispatcher Name(params)` declares an event dispatcher. Functions are
[bound to it](/reference/events/#binding-a-dispatcher), and `Name.broadcast(args)` calls everything
bound to it. It can pass a [delegate](/reference/types/#delegates), which a function bound to it
takes as a parameter.

```loom
dispatcher Scored(points: int)

fn Win() {
    Scored.broadcast(100)
}
```

## Components

`component Name: Class(field: value, ...)` builds the Blueprint with a component, with its fields
set as the Details panel sets them. Only an actor's Blueprint has components.

```loom
component Sound: AudioComponent(bAutoActivate: false)
component Root: SceneComponent

fn Play() {
    Sound.Play(0.0)
}
```

## Class defaults

`default Name = value` gives a property the parent class declares this Blueprint's own default, as
the Class Defaults panel does. The name can go on into a struct's fields. A property only the
Details panel shows, which code cannot reach, can be given a default too.

```loom
default InitialLifeSpan = 5.0
default Tags = ["pickup", "small"]
default PrimaryActorTick.bTickEvenWhenPaused = true
```

```loom error="only in the Details panel"
fn F(other: Actor) -> bool {
    return other.PrimaryActorTick.bTickEvenWhenPaused
}
```

`default Name = Class.Name` copies another class's default for the same property, whole, with any
fields no literal can reach.

```loom program="pickups"
blueprint BP_Pickup : Actor at /Game/Pickups/BP_Pickup

default Tags = ["pickup"]
```

```loom program="pickups"
blueprint BP_Coin : Actor at /Game/Pickups/BP_Coin

default Tags = BP_Pickup.Tags
```

## Names

A name that starts with `Loom_` is kept for the ones loom makes itself, such as the functions that
handle events and the variables that hold values between nodes. A variable, function, parameter,
dispatcher, component, widget or local may not start with it.

```loom error="loom keeps for names it makes itself"
var Loom_Count: int
```
