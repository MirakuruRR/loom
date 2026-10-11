---
title: Expressions
description: Values, operators, calls and members.
sidebar:
  order: 5
---

## Literals

```loom
fn F() {
    let a = 42
    let b = 0xFF
    let c = 2.5
    let d = true
    let e = false
    let f = "a line\nand another"
    let g: Actor = none
}
```

A string may hold `\n`, `\"` and `\\`.

`self` is the object the function runs on.

```loom
fn Me() -> Actor {
    return self
}
```

A path, such as `/Script/Engine.Actor`, is a literal only where a soft reference, a class or an
object is expected.

## Operators

The [built-ins](/reference/builtins/#operators) list every operator, from loosest to tightest,
and which types each takes. Bitwise operators bind tighter than comparisons, so `flags & 32 != 0`
reads as it is meant, and parentheses group as usual.

```loom
fn F(a: int, b: int, ok: bool) -> bool {
    let sum = a + b * 2 - (a % 3) / 1
    let flags = a & 32 | b
    return !ok || sum >= 10 && sum <= 20 && flags != 0 && a < b || a > b || a == -b
}
```

Both sides of an operator must have the same type. Convert one with `float()` or `int()`. A
whole number written out beside a float is taken as a float, and beside an `int64` as an `int64`.

```loom error="mixes types"
fn F(count: int) {
    let x = count + 2.5
}
```

```loom
fn F(speed: float) -> bool {
    return speed > 0 && speed * 2 < 10
}
```

A struct has the operators its own table gives it, which the engine's library functions carry out:
a `Vector` has `+`, `-`, `*`, `/`, `==` and `!=`, and `* float`. Whichever side the struct is
written on, it is the function's first argument, so `2.0 * Reach` is `Reach * 2.0`. An operator the
struct has not is refused with the list of the ones it has.

```loom
var Where: Vector
var Fwd: Vector
var Reach: float
var Facing: Rotator

fn Ahead() -> Vector {
    let at = Where + Fwd * Reach
    let aimed = Fwd * 2.0 + Where
    return -at
}

fn Same(a: Rotator, b: Rotator) -> bool {
    return a == b && a * 1.5 != b
}
```

A class or a soft reference is compared with `none` to ask whether it is set, which the engine's own
validity check answers.

```loom
var Kind: class<Actor>
var Icon: soft<Texture2D>

fn Ready() -> bool {
    return Kind != none && Icon == none
}
```

`&&` and `||` cannot short-circuit, since both sides are computed first, so neither side may
contain a call that does something, or an `if` expression. Split it into `if`s.

## If expressions

`if condition { a } else { b }` is `a` when the condition holds, and `b` when it doesn't. It is
written as an [`if` statement](/reference/statements/#if) is, with a value on each side, and it
always has an `else`. Only the side chosen is computed, so either side may call something that
does something, or read an item that is only there when the condition holds.

```loom
fn Label(n: int, items: array<int>) -> string {
    let first = if items.Length() > 0 { items[0] } else { 0 }
    return if n == 0 { "none" } else if n == 1 { string(first) } else { "many" }
}
```

Both sides give the same type. A whole number written out beside a float is taken as a float, and
`none` beside an object is that object's type.

```loom error="one side is int and the other string"
fn F(ok: bool) {
    let x = if ok { 1 } else { "one" }
}
```

## Calls

A function of this Blueprint, or of its parent, is called by name. A static function is called on
its class. A function of an object is called on the object.

```loom
fn Twice(x: float) -> float {
    return x * 2.0
}

fn F(other: Actor) -> float {
    let a = Twice(1.5)
    let b = KismetMathLibrary.Sin(a)
    other.K2_DestroyActor()
    return b
}
```

Some calls return several outputs, such as a function with [`out` parameters](/reference/members/#outputs).
`let` gives such a call a name, and each output is a member of it.

```loom
fn F(target: TextureRenderTarget2D) {
    let draw = KismetRenderingLibrary.BeginDrawCanvasToRenderTarget(target)
    KismetRenderingLibrary.EndDrawCanvasToRenderTarget(draw.Context)
}
```

`format(pattern, value, ...)` fills a [`text`](/reference/builtins/#format)'s places with values:
`{0}` is the first value after the pattern, `{1}` the second, and so on. Text and strings go in as
they are, numbers as numbers, and anything else that can be shown as text is converted.

```loom
var Count: int
var Stock: map<string, int>

fn Report(name: string) -> text {
    return format("{0} has {1} of {2}", name, Count, Stock.Length())
}
```

## Calls through an interface

`cast(thing, Interface)` is the object as that interface, or `none` when it doesn't implement it.
A function of the interface called on it is sent as the editor's message call is: to an object
that doesn't implement the interface it does nothing, and what it returns is its type's zero.
`implements(thing, Interface)` says whether the object implements it.

```loom
fn Ease(thing: Object, t: float) -> float {
    if !implements(thing, MovieSceneEasingFunction) {
        return t
    }
    return cast(thing, MovieSceneEasingFunction).OnEvaluate(t)
}
```

An object whose class implements the interface, such as `self` in a Blueprint that implements it,
takes the interface's functions as its own, with no cast. Such a call is a message too.

```loom
blueprint BP_Curve : Actor at /Game/Curves/BP_Curve
implements MovieSceneEasingFunction

on OnEvaluate(t: float) -> float {
    return t * t
}

fn Half() -> float {
    return OnEvaluate(0.5) + self.OnEvaluate(0.25)
}
```

Any other object needs the cast:

```loom error="Object has no function OnEvaluate"
fn Ease(thing: Object) -> float {
    return thing.OnEvaluate(0.5)
}
```

## Match expressions

A [`match`](/reference/statements/#match) gives a value when each arm holds one. It always has an
`else` arm, and as with `if`, only the arm chosen is computed, and the arms give the same type.

```loom
fn Speed(group: ETickingGroup) -> float {
    return match group {
        ETickingGroup.TG_PrePhysics, ETickingGroup.TG_StartPhysics { 300.0 }
        ETickingGroup.TG_PostPhysics { 600.0 }
        else { 0.0 }
    }
}
```

## Members and items

`object.Name` reads a variable of an object, or a field of a struct. A struct the engine reads with a
function of its own takes the names that function gives its outputs, which are not always the
struct's own field names: a Transform is read as `Location`, `Rotation` and `Scale`, so
`transform.Location` is right and `transform.Translation` is refused with the names to use instead.
`array[i]` reads an item of an array, and `map[key]` the value a map holds for a key, or the value
type's zero when it holds none.

```loom
var Where: Vector2D
var Items: array<int> = [1, 2]
var Stock: map<string, int> = {"wood": 3}
var Placed: Transform

fn F() -> float {
    return Where.X + float(Items[0] + Stock["wood"])
}

fn At() -> Vector {
    return Placed.Location
}
```

## Structs

`Struct(field: value, ...)` makes a struct with those fields. A field left out is its type's zero,
and one the struct's own make function gives a default to keeps that default: a `Transform` left
without its Scale has a scale of 1, not of 0.

```loom
var At: Vector
var Facing: Rotator

fn F() -> Vector2D {
    return Vector2D(X: 1.0, Y: 2.0)
}

fn Turned(yaw: float) -> Rotator {
    return Rotator(Yaw: yaw)
}

fn Placed() -> Transform {
    return Transform(Location: At, Rotation: Facing)
}
```

A struct the engine makes with a function of its own — Vector, Rotator, Transform and the like — is
written with that function's parameters, which are not always named as the struct's own fields:
`Transform(Location: ..., Rotation: ...)` calls `MakeTransform`. A name it does not take is refused
with the list of the ones it does.

## Enums

`Enum.Value` is one of an enum's values.

```loom
fn Late() {
    SetTickGroup(ETickingGroup.TG_PostUpdateWork)
}
```

## Arrays and maps

`[a, b, c]` fills an array variable or gives one its default. `[]` empties one. `{key: value}` is
a map's default, and `{}` an empty map.

```loom
var Items: array<int>

fn F() {
    Items = [1, 2, 3]
    Items = []
}
```

An empty `[]` or `{}` needs a variable to fill, which is emptied in place. Blueprint has no node
that makes an empty array, set or map, so a local `set` or `map`, a struct's field, a call's
argument and a return cannot be given one: empty a variable and give that instead.

```loom error="is not something Blueprint can make"
fn F() {
    let seen: set<int> = []
}
```
