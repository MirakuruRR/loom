---
title: Function libraries
description: Blueprints of functions that any Blueprint can call.
sidebar:
  order: 9
---

A function library is a Blueprint whose parent is `BlueprintFunctionLibrary`, Unreal's own kind of
Blueprint for shared helpers. It holds only functions. They belong to the class, not to an object,
so they have no `self`, and any Blueprint calls them by the library's name, with no object.

## Calling a library's functions

Write the library, a dot and the function. The library is found by name like any class, so `use`
and the usual naming rules apply.

```loom program="calling"
blueprint BPL_Math : BlueprintFunctionLibrary at /Game/Lib/BPL_Math

// The value, kept between lo and hi.
pure fn Clamp(x: int, lo: int, hi: int) -> int {
    if x < lo {
        return lo
    }
    if x > hi {
        return hi
    }
    return x
}
```

```loom program="calling"
blueprint BP_Meter : Actor at /Game/Meter/BP_Meter

var Level: int = 0

fn Raise(by: int) {
    Level = BPL_Math.Clamp(Level + by, 0, 10)
}
```

Most library functions only compute a value, so they're usually [pure](/reference/members/#pure-functions), with no exec
pins, like the math nodes in Unreal's own libraries.

A library has no variables of its own, so a `ref` parameter is how it changes one of the caller's.
It works as in any [function](/reference/members/#functions).

## What a library can't have

Anything but functions is refused:

```loom error="a function library has no variables" refusals="library"
blueprint BPL_Math : BlueprintFunctionLibrary at /Game/Lib/BPL_Math

var Count: int = 0
```

```loom error="a function library has no dispatchers" refusals="library"
blueprint BPL_Math : BlueprintFunctionLibrary at /Game/Lib/BPL_Math

dispatcher Changed(value: int)
```

```loom error="a function library has no events" refusals="library"
blueprint BPL_Math : BlueprintFunctionLibrary at /Game/Lib/BPL_Math

on ReceiveBeginPlay {
}
```

```loom error="only an actor's Blueprint has components" refusals="library"
blueprint BPL_Math : BlueprintFunctionLibrary at /Game/Lib/BPL_Math

component Sound: AudioComponent
```

```loom error="only a widget Blueprint, whose parent is a UserWidget, has widgets" refusals="library"
blueprint BPL_Math : BlueprintFunctionLibrary at /Game/Lib/BPL_Math

widget Title: TextBlock
```

```loom error="a function library implements no interfaces" refusals="library"
blueprint BPL_Math : BlueprintFunctionLibrary at /Game/Lib/BPL_Math

implements MovieSceneEasingFunction
```

```loom error="a function library has no self" refusals="library"
blueprint BPL_Math : BlueprintFunctionLibrary at /Game/Lib/BPL_Math

fn Owner() -> Object {
    return self
}
```

A library's parent is `BlueprintFunctionLibrary`, not another library. Unreal won't make a child of
one, whether it's a Blueprint's or one of the engine's own, such as `KismetMathLibrary`:

```loom program="calling" error="a function library can't derive from another library, and BPL_Math is one"
blueprint BPL_Geometry : BPL_Math at /Game/Lib/BPL_Geometry
```
