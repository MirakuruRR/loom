---
title: Statements
description: What a function's body is made of.
sidebar:
  order: 4
---

A function's body is statements, one per line.

## let

`let name = value` declares a local. `let name: Type = value` gives its type, which a value such as
`none` or `[]` needs.

```loom
fn F() {
    let count = 3
    let target: Actor = none
    let names: array<string> = []
}
```

## Assignment

A variable, a member of an object or struct, or an item of an array or map can be assigned.

```loom
var Count: int
var Items: array<int>
var Stock: map<string, int>
var Where: Vector2D

fn F() {
    Count = 1
    Where.X = 2.0
    Items.Add(0)
    Items[0] = 5
    Stock["wood"] = 3
}
```

A parameter cannot be assigned:

```loom error="cannot be assigned"
fn F(n: int) {
    n = 1
}
```

### Compound assignment

`+=`, `-=`, `*=`, `/=`, `%=`, `&=` and `|=` apply their operator to what is assigned and the value,
and assign the result: `Count += 2` is `Count = Count + 2`. What picks out the target, such as an
index or a call that gives the object, is computed once.

```loom
var Count: int
var Items: array<int> = [1, 2, 3]
var Label: string

fn F(i: int) {
    Count += 2
    Items[i] *= 10
    Label += "!"
}
```

## if

```loom
fn Sign(x: int) -> int {
    if x < 0 {
        return -1
    } else if x == 0 {
        return 0
    } else {
        return 1
    }
}
```

A condition must be a `bool`:

```loom error="not bool"
fn F(n: int) {
    if n {
    }
}
```

## match

`match` compares a value with the values each arm lists, and runs the first arm that lists it. An
arm lists one or more values, separated by commas, then its block. `else` runs when no arm lists
the value, and comes last. Without it, a value no arm lists does nothing, as in Blueprint's switch
nodes.

```loom
var Speed: float = 0.0

fn Move(group: ETickingGroup) {
    match group {
        ETickingGroup.TG_PrePhysics, ETickingGroup.TG_StartPhysics {
            Speed = 300.0
        }
        ETickingGroup.TG_PostPhysics {
            Speed = 600.0
        }
        else {
            Speed = 0.0
        }
    }
}
```

The value is an `int`, a `string`, a `name` or an enum value, and is computed once. Each arm lists
values written out, a number, a string or `Enum.Value`, and each value once:

```loom error="listed by an earlier arm"
fn F(n: int) {
    match n {
        1 {
        }
        2, 1 {
        }
    }
}
```

## while

```loom
fn Sum(n: int) -> int {
    let total = 0
    let i = 0
    while i <= n {
        total = total + i
        i = i + 1
    }
    return total
}
```

## for

`for item in array` runs once for each item of an array or a set. `for item, i in array` also names
the item's index, counting from 0. `for key, value in map` runs once for each entry of a map.

```loom
var Names: array<string> = ["a", "b"]
var Stock: map<string, int> = {"wood": 3}

fn Count() -> int {
    let total = 0
    for name in Names {
        total = total + 1
    }
    for name, i in Names {
        total = total + i
    }
    for item, count in Stock {
        total = total + count
    }
    return total
}
```

`for i in first..last` counts from `first` to `last`, both included, as Blueprint's For Loop does,
so `0..2` runs three times. Both are whole numbers. `first` is computed once, before the first
pass, and `last` again before every pass, so a count up to `Items.Length() - 1` follows the array
as it grows or shrinks. A count whose first number is past its last doesn't run. `i` is read-only
inside the loop.

```loom
fn Triangle(n: int) -> int {
    let total = 0
    for i in 1..n {
        total = total + i
    }
    return total
}
```

## break and continue

`break` leaves a `while` or a `for`, and `continue` goes on to its next pass. In a loop inside
another, both apply to the inner one.

```loom
fn FirstNegative(xs: array<int>) -> int {
    let found = 0
    for x in xs {
        if x < 0 {
            found = x
            break
        }
    }
    return found
}

fn SumOdd(n: int) -> int {
    let total = 0
    let i = 0
    while i < n {
        i = i + 1
        if i % 2 == 0 {
            continue
        }
        total = total + i
    }
    return total
}
```

Either one outside a loop is refused:

```loom error="only allowed inside a loop"
fn F() {
    break
}
```

### Labelled loops

A loop can be given a label, a name and a colon before its `for` or `while`. A `break` or
`continue` that names the label leaves that loop or goes on to its next pass, however many loops
inside it the jump is in. Without a label, each still applies to the innermost loop.

```loom
fn FindEmpty(grid: array<string>, size: int) -> int {
    let found = -1
    rows: for row in 0..size - 1 {
        for column in 0..size - 1 {
            if grid[row * size + column] == "" {
                found = row * size + column
                break rows
            }
            if column > row {
                continue rows
            }
        }
    }
    return found
}
```

A label must name a loop the jump is inside:

```loom error="no loop around this break is labelled rows"
fn F(n: int) {
    while n > 0 {
        break rows
    }
}
```

## return

`return` leaves the function, with a value when the function returns one. Code after a `return`,
`break` or `continue` that every path reaches can never run, and is refused.

```loom error="never run"
fn F() -> int {
    return 1
    return 2
}
```

## Calls

A call that does something can stand on its own line. A pure call, one that only computes a value,
does nothing on its own and is refused there.

```loom
fn F() {
    KismetSystemLibrary.PrintString("hi")
}
```

```loom error="pure"
fn F() {
    KismetMathLibrary.Add_IntInt(1, 2)
}
```
