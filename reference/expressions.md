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
whole number written out beside a float is taken as a float.

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

`object.Name` reads a variable of an object, or a field of a struct. `array[i]` reads an item of an
array, and `map[key]` the value a map holds for a key, or the value type's zero when it holds none.

```loom
var Where: Vector2D
var Items: array<int> = [1, 2]
var Stock: map<string, int> = {"wood": 3}

fn F() -> float {
    return Where.X + float(Items[0] + Stock["wood"])
}
```

## Structs

`Struct(field: value, ...)` makes a struct with those fields, and the rest zero.

```loom
fn F() -> Vector2D {
    return Vector2D(X: 1.0, Y: 2.0)
}
```

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
