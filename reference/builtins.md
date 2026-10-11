---
title: Built-ins
description: The plain types, conversions, functions, methods and operators Loom gives.
sidebar:
  order: 10
tableOfContents:
  maxHeadingLevel: 2
---

<!-- Written by the compiler's tests from packages/loom/src/builtins.rs. Change that, not this. -->

What Loom gives without a class to look in. Each is written from the compiler's own tables, and each example below compiles. The examples use these variables:

```loom prelude
var Alive: bool = true
var Lives: int = 3
var Seed: int64 = 0
var Speed: float = 2.5
var Title: string = "Counter"
var Tag: name = "wood"
var Label: text = "Hello"
var Target: Actor
var Kind: class<Actor>
var Group: ETickingGroup
var Icon: soft<Texture2D>
var Spawns: soft<class<Actor>>
var Items: array<int>
var Seen: set<name>
var Stock: map<string, int>
```

## Plain types

| Type | Is |
| --- | --- |
| `bool` | `true` or `false`. |
| `int` | A 32-bit whole number. |
| `int64` | A 64-bit whole number. |
| `float` | A number with a fraction, held as a double, as Blueprint floats are. |
| `string` | Text to work with. |
| `name` | An identifier, such as a tag, compared quickly. |
| `text` | Text to show a player. |

## Functions

### float

`float(value) -> float`

The value as a `float`. See [Conversions](#conversions) for what it converts.

```loom
fn F() -> float {
    return float(Lives) * Speed
}
```

### int

`int(value) -> int`

The value as an `int`. A `float` loses its fraction, a `string` that does not start with a number gives 0, and an enum gives the number that stands for its value.

```loom
fn F() -> int {
    return int(Speed)
}
```

### string

`string(value) -> string`

The value as a `string`.

```loom
fn F() -> string {
    return "lives: " + string(Lives)
}
```

### name

`name(value) -> name`

The value as a `name`, such as a key made up from strings.

```loom
fn F(id: string) -> name {
    return name("option." + id)
}
```

### text

`text(value) -> text`

The value as `text`, to show a player.

```loom
fn F() -> text {
    return text(Title)
}
```

### format

`format(pattern, value, ...) -> text`

The `text` pattern with each `{0}`, `{1}` and so on replaced by the value of that place, as the engine's own Format does. The pattern comes first, then one value per place: text and strings go in as they are, numbers as numbers, and anything else that can be shown as text is converted.

```loom
fn F() -> text {
    return format("{0} of {1}", Lives, float(Lives))
}
```

### cast

`cast(value, Class or Interface) -> Class or Interface`

The object if it is of the class, and `none` otherwise. With a `class<T>`, the class if it is the class or a subclass of it, and `none` otherwise.

With an interface, the object as that interface, or `none` if it doesn't implement it. Call the interface's functions on the result, as [Calls through an interface](/reference/expressions/#calls-through-an-interface) shows.

```loom
fn F(thing: Object) -> Actor {
    return cast(thing, Actor)
}
```

### implements

`implements(value, Interface) -> bool`

Whether the object implements the interface, itself or through its parents. `false` for `none`.

```loom
fn F() -> bool {
    return implements(Target, MovieSceneEasingFunction)
}
```

### create_widget

`create_widget(Class) -> Class`

A new widget of a widget Blueprint's class.

```loom
blueprint WBP_Panel : UserWidget at /Game/UI/WBP_Panel

fn Another() {
    let panel = create_widget(WBP_Panel)
    panel.AddToViewport(0)
}
```

### construct

`construct(Class, outer) -> Class`

A new object of the class, held by `outer`, which it lives as long as. Not for an actor, which is spawned, or a widget Blueprint, which `create_widget` makes.

```loom
fn F(owner: Object) -> HorizontalBox {
    return construct(HorizontalBox, owner)
}
```

### include

`include("file.json")`

A variable's default, read from a JSON file when the Blueprint is compiled: an object as a map, an array as an array. The path is relative to the source file. Only a variable's default can be included.

```loom
var Strings: map<name, string> = include("strings.json")
```

### subsystem

`subsystem(Class) -> Class`

The engine's subsystem of the class, or in an editor Blueprint, the editor's.

```loom
fn F() -> bool {
    return subsystem(DataDrivenCVarEngineSubsystem) != none
}
```

## Conversions

What each conversion takes, and the Blueprint function it calls.

| Conversion | Of | Calls |
| --- | --- | --- |
| `float()` | `float` | nothing: it is the value itself |
| `float()` | `int` | `KismetMathLibrary.Conv_IntToDouble` |
| `float()` | `string` | `KismetStringLibrary.Conv_StringToDouble` |
| `int()` | `int` | nothing: it is the value itself |
| `int()` | `float` | `KismetMathLibrary.FTrunc` |
| `int()` | a byte | `KismetMathLibrary.Conv_ByteToInt` |
| `int()` | an enum | `KismetMathLibrary.Conv_ByteToInt` |
| `int()` | `string` | `KismetStringLibrary.Conv_StringToInt` |
| `string()` | `string` | nothing: it is the value itself |
| `string()` | `int` | `KismetStringLibrary.Conv_IntToString` |
| `string()` | `float` | `KismetStringLibrary.Conv_DoubleToString` |
| `string()` | `bool` | `KismetStringLibrary.Conv_BoolToString` |
| `string()` | `name` | `KismetStringLibrary.Conv_NameToString` |
| `string()` | `text` | `KismetTextLibrary.Conv_TextToString` |
| `string()` | an object | `KismetStringLibrary.Conv_ObjectToString` |
| `string()` | `soft<T>` | `KismetSystemLibrary.Conv_SoftObjectReferenceToString` |
| `string()` | `soft<class<T>>` | `KismetSystemLibrary.Conv_SoftClassReferenceToString` |
| `string()` | `class<T>` | `KismetSystemLibrary.Conv_ClassToSoftClassReference` (internal), then `KismetSystemLibrary.Conv_SoftClassReferenceToString` |
| `name()` | `name` | nothing: it is the value itself |
| `name()` | `string` | `KismetStringLibrary.Conv_StringToName` |
| `text()` | `text` | nothing: it is the value itself |
| `text()` | `string` | `KismetTextLibrary.Conv_StringToText` |

```loom
fn Examples() {
    let shown = string(Lives)
    let rounded = int(Speed)
    let exact = float(Lives)
    let heading = text(Title)
}
```

## Array methods

Items are also read and set with `array[index]`, and walked with `for`. `T` is what the array holds. Each method is a function of `KismetArrayLibrary`, called by its name without `Array_`. One that gives outputs gives them as a call to any function does.

| Method | Gives | Does | Calls |
| --- | --- | --- | --- |
| `Add(NewItem: T)` | `int` | Adds the item at the end, and gives its index. | `KismetArrayLibrary.Array_Add` |
| `AddUnique(NewItem: T)` | `int` | Adds the item at the end unless it's there already, and gives its index, or -1. | `KismetArrayLibrary.Array_AddUnique` |
| `Append(SourceArray: array<T>)` | nothing | Adds every item of the other array at the end, in order. | `KismetArrayLibrary.Array_Append` |
| `Clear()` | nothing | Removes every item. | `KismetArrayLibrary.Array_Clear` |
| `Contains(ItemToFind: T)` | `bool` | Whether the item is in it. | `KismetArrayLibrary.Array_Contains`, pure |
| `Find(ItemToFind: T)` | `int` | The index of the item's first place, or -1. | `KismetArrayLibrary.Array_Find`, pure |
| `Identical(ArrayB: array<T>)` | `bool` | Whether the other array holds the same items in the same order. | `KismetArrayLibrary.Array_Identical`, pure |
| `Insert(NewItem: T, Index: int)` | nothing | Puts the item at the index, moving the ones after it along. | `KismetArrayLibrary.Array_Insert` |
| `IsEmpty()` | `bool` | Whether it holds nothing. | `KismetArrayLibrary.Array_IsEmpty`, pure |
| `IsNotEmpty()` | `bool` | Whether it holds anything. | `KismetArrayLibrary.Array_IsNotEmpty`, pure |
| `IsValidIndex(IndexToTest: int)` | `bool` | Whether the index is one of its items'. | `KismetArrayLibrary.Array_IsValidIndex`, pure |
| `LastIndex()` | `int` | The index of its last item, or -1 if it's empty. | `KismetArrayLibrary.Array_LastIndex`, pure |
| `Length()` | `int` | How many items it holds. | `KismetArrayLibrary.Array_Length`, pure |
| `Random()` | the outputs `OutItem: T, OutIndex: int` | A random item, and its index. | `KismetArrayLibrary.Array_Random`, pure |
| `RandomFromStream(RandomStream: RandomStream)` | the outputs `OutItem: T, OutIndex: int` | A random item, and its index, drawn from the stream. | `KismetArrayLibrary.Array_RandomFromStream`, pure |
| `Remove(IndexToRemove: int)` | nothing | Removes the item at the index, moving the ones after it back. | `KismetArrayLibrary.Array_Remove` |
| `RemoveItem(Item: T)` | `bool` | Removes every copy of the item, and gives whether there was one. | `KismetArrayLibrary.Array_RemoveItem` |
| `Resize(Size: int)` | nothing | Makes it that long, adding zeroed items or dropping the last ones. | `KismetArrayLibrary.Array_Resize` |
| `Reverse()` | nothing | Reverses its order. | `KismetArrayLibrary.Array_Reverse` |
| `Set(Index: int, Item: T, bSizeToFit: bool)` | nothing | Sets the item at the index; with bSizeToFit, grows it to reach an index past its end. | `KismetArrayLibrary.Array_Set` |
| `Shuffle()` | nothing | Puts its items in a random order. | `KismetArrayLibrary.Array_Shuffle` |
| `ShuffleFromStream(RandomStream: RandomStream)` | nothing | Puts its items in a random order, drawn from the stream. | `KismetArrayLibrary.Array_ShuffleFromStream` |
| `Swap(FirstIndex: int, SecondIndex: int)` | nothing | Swaps the items at the two indices. | `KismetArrayLibrary.Array_Swap` |

```loom
fn Examples() {
    Items.Add(4)
    let count = Items.Length()
    let has_four = Items.Contains(4)
    Items.RemoveItem(4)
}
```

## Set methods

A set is walked with `for`. `T` is what the set holds. Each method is a function of `BlueprintSetLibrary`, called by its name without `Set_`. One that gives outputs gives them as a call to any function does.

| Method | Gives | Does | Calls |
| --- | --- | --- | --- |
| `Add(NewItem: T)` | nothing | Adds the item, unless it's there already. | `BlueprintSetLibrary.Set_Add` |
| `AddItems(NewItems: array<T>)` | nothing | Adds each item of the array that isn't there already. | `BlueprintSetLibrary.Set_AddItems` |
| `Clear()` | nothing | Removes every item. | `BlueprintSetLibrary.Set_Clear` |
| `Contains(ItemToFind: T)` | `bool` | Whether the item is in it. | `BlueprintSetLibrary.Set_Contains`, pure |
| `Difference(B: set<T>)` | the outputs `Result: set<T>` | The items of this set that aren't in the other. | `BlueprintSetLibrary.Set_Difference` |
| `GetItemByIndex(Index: int)` | the outputs `Item: T` | The item at the index, in the set's own order. | `BlueprintSetLibrary.Set_GetItemByIndex`, pure |
| `GetLastIndex()` | `int` | The index of its last item, or -1 if it's empty. | `BlueprintSetLibrary.Set_GetLastIndex`, pure |
| `Intersection(B: set<T>)` | the outputs `Result: set<T>` | The items in both sets. | `BlueprintSetLibrary.Set_Intersection` |
| `IsEmpty()` | `bool` | Whether it holds nothing. | `BlueprintSetLibrary.Set_IsEmpty`, pure |
| `IsNotEmpty()` | `bool` | Whether it holds anything. | `BlueprintSetLibrary.Set_IsNotEmpty`, pure |
| `Length()` | `int` | How many items it holds. | `BlueprintSetLibrary.Set_Length`, pure |
| `Remove(Item: T)` | `bool` | Removes the item, and gives whether it was there. | `BlueprintSetLibrary.Set_Remove` |
| `RemoveItems(Items: array<T>)` | nothing | Removes each item of the array. | `BlueprintSetLibrary.Set_RemoveItems` |
| `ToArray()` | the outputs `Result: array<T>` | Its items, as an array. | `BlueprintSetLibrary.Set_ToArray` |
| `Union(B: set<T>)` | the outputs `Result: set<T>` | The items in either set. | `BlueprintSetLibrary.Set_Union` |

```loom
fn Examples() {
    Seen.Add("wood")
    let seen_wood = Seen.Contains("wood")
    let kinds = Seen.Length()
}
```

## Map methods

A value is also read and set with `map[key]`, and a map walked with `for key, value in map`. `K` and `V` are the map's keys' and values' types. Each method is a function of `BlueprintMapLibrary`, called by its name without `Map_`. One that gives outputs gives them as a call to any function does.

| Method | Gives | Does | Calls |
| --- | --- | --- | --- |
| `Add(Key: K, Value: V)` | nothing | Sets the key's value, adding the key if it's new. | `BlueprintMapLibrary.Map_Add` |
| `Clear()` | nothing | Removes every key. | `BlueprintMapLibrary.Map_Clear` |
| `Contains(Key: K)` | `bool` | Whether the key is in it. | `BlueprintMapLibrary.Map_Contains`, pure |
| `Find(Key: K)` | the outputs `Value: V, ReturnValue: bool` | The key's value, and whether the key was there. | `BlueprintMapLibrary.Map_Find`, pure |
| `GetKeyValueByIndex(Index: int)` | the outputs `Key: K, Value: V` | The key and value at the index, in the map's own order. | `BlueprintMapLibrary.Map_GetKeyValueByIndex`, pure |
| `GetLastIndex()` | `int` | The index of its last key, or -1 if it's empty. | `BlueprintMapLibrary.Map_GetLastIndex`, pure |
| `IsEmpty()` | `bool` | Whether it holds nothing. | `BlueprintMapLibrary.Map_IsEmpty`, pure |
| `IsNotEmpty()` | `bool` | Whether it holds anything. | `BlueprintMapLibrary.Map_IsNotEmpty`, pure |
| `Keys()` | the outputs `Keys: array<K>` | Its keys, as an array. | `BlueprintMapLibrary.Map_Keys` |
| `Length()` | `int` | How many keys it holds. | `BlueprintMapLibrary.Map_Length`, pure |
| `Remove(Key: K)` | `bool` | Removes the key and its value, and gives whether it was there. | `BlueprintMapLibrary.Map_Remove` |
| `Values()` | the outputs `Values: array<V>` | Its values, as an array. | `BlueprintMapLibrary.Map_Values` |

```loom
fn Examples() {
    Stock.Add("wood", 3)
    let wood = Stock.Find("wood")
    let has_wood = wood.ReturnValue
    let wood_count = wood.Value
}
```

## Soft reference methods

`load()` on a `soft<class<T>>` gives the class, as a `class<T>`.

| Method | Gives | Does | Calls |
| --- | --- | --- | --- |
| `load()` | the object, or the class | Loads what it refers to, waits until it has, and gives it. | `LoadAsset_Blocking`, or for a class, `LoadClassAsset_Blocking` |
| `valid()` | `bool` | Whether it refers to anything. Pure, so it cannot stand on its own. | `IsValidSoftObjectReference`, or for a class, `IsValidSoftClassReference` |

```loom
fn Examples() {
    let spawned = Spawns.load()
    let has_icon = Icon.valid()
}
```

## Operators

From loosest to tightest. Parentheses group as usual.

1. `||`: or
2. `&&`: and
3. `==` `!=`: equal, not equal
4. `<` `<=` `>` `>=`: less, less or equal, greater, greater or equal
5. `|`: bitwise or
6. `&`: bitwise and
7. `+` `-`: add, subtract
8. `*` `/` `%`: multiply, divide, remainder
9. `-x` `!x`: negate, not

`+=`, `-=` and the other compound assignments aren't on this list: each is a statement that applies an operator and assigns the result. See [Compound assignment](/reference/statements/#compound-assignment).

Both sides of an operator are the same type, but where the table says two, as `Vector` and `float` for `Reach * 2.0`: a structure's operators are the engine's own functions, and a structure is not one of the plain kinds. The table says what each operator takes. `&&` and `||` compute both sides, so neither may contain a call that does something.

| Operator | Takes | Gives | Calls |
| --- | --- | --- | --- |
| `\|\|` | `bool` | `bool` | `KismetMathLibrary.BooleanOR` |
| `&&` | `bool` | `bool` | `KismetMathLibrary.BooleanAND` |
| `==` | `int` | `bool` | `KismetMathLibrary.EqualEqual_IntInt` |
| `==` | `int64` | `bool` | `KismetMathLibrary.EqualEqual_Int64Int64` |
| `==` | `float` | `bool` | `KismetMathLibrary.EqualEqual_DoubleDouble` |
| `==` | `bool` | `bool` | `KismetMathLibrary.EqualEqual_BoolBool` |
| `==` | `string` | `bool` | `KismetStringLibrary.EqualEqual_StrStr` |
| `==` | `name` | `bool` | `KismetMathLibrary.EqualEqual_NameName` |
| `==` | a byte | `bool` | `KismetMathLibrary.EqualEqual_ByteByte` |
| `==` | an object | `bool` | `KismetMathLibrary.EqualEqual_ObjectObject` |
| `==` | an enum | `bool` | the editor's enum equality node |
| `==` | `Vector` and `Vector` | `bool` | `KismetMathLibrary.EqualEqual_VectorVector` |
| `==` | `Vector2D` and `Vector2D` | `bool` | `KismetMathLibrary.EqualEqual_Vector2DVector2D` |
| `==` | `Rotator` and `Rotator` | `bool` | `KismetMathLibrary.EqualEqual_RotatorRotator` |
| `!=` | `int` | `bool` | `KismetMathLibrary.NotEqual_IntInt` |
| `!=` | `int64` | `bool` | `KismetMathLibrary.NotEqual_Int64Int64` |
| `!=` | `float` | `bool` | `KismetMathLibrary.NotEqual_DoubleDouble` |
| `!=` | `bool` | `bool` | `KismetMathLibrary.NotEqual_BoolBool` |
| `!=` | `string` | `bool` | `KismetStringLibrary.NotEqual_StrStr` |
| `!=` | `name` | `bool` | `KismetMathLibrary.NotEqual_NameName` |
| `!=` | a byte | `bool` | `KismetMathLibrary.NotEqual_ByteByte` |
| `!=` | an object | `bool` | `KismetMathLibrary.NotEqual_ObjectObject` |
| `!=` | an enum | `bool` | the editor's enum equality node |
| `!=` | `Vector` and `Vector` | `bool` | `KismetMathLibrary.NotEqual_VectorVector` |
| `!=` | `Vector2D` and `Vector2D` | `bool` | `KismetMathLibrary.NotEqual_Vector2DVector2D` |
| `!=` | `Rotator` and `Rotator` | `bool` | `KismetMathLibrary.NotEqual_RotatorRotator` |
| `<` | `int` | `bool` | `KismetMathLibrary.Less_IntInt` |
| `<` | `int64` | `bool` | `KismetMathLibrary.Less_Int64Int64` |
| `<` | `float` | `bool` | `KismetMathLibrary.Less_DoubleDouble` |
| `<=` | `int` | `bool` | `KismetMathLibrary.LessEqual_IntInt` |
| `<=` | `int64` | `bool` | `KismetMathLibrary.LessEqual_Int64Int64` |
| `<=` | `float` | `bool` | `KismetMathLibrary.LessEqual_DoubleDouble` |
| `>` | `int` | `bool` | `KismetMathLibrary.Greater_IntInt` |
| `>` | `int64` | `bool` | `KismetMathLibrary.Greater_Int64Int64` |
| `>` | `float` | `bool` | `KismetMathLibrary.Greater_DoubleDouble` |
| `>=` | `int` | `bool` | `KismetMathLibrary.GreaterEqual_IntInt` |
| `>=` | `int64` | `bool` | `KismetMathLibrary.GreaterEqual_Int64Int64` |
| `>=` | `float` | `bool` | `KismetMathLibrary.GreaterEqual_DoubleDouble` |
| `\|` | `int` | `int` | `KismetMathLibrary.Or_IntInt` |
| `\|` | `int64` | `int64` | `KismetMathLibrary.Or_Int64Int64` |
| `&` | `int` | `int` | `KismetMathLibrary.And_IntInt` |
| `&` | `int64` | `int64` | `KismetMathLibrary.And_Int64Int64` |
| `+` | `int` | `int` | `KismetMathLibrary.Add_IntInt` |
| `+` | `int64` | `int64` | `KismetMathLibrary.Add_Int64Int64` |
| `+` | `float` | `float` | `KismetMathLibrary.Add_DoubleDouble` |
| `+` | `string` | `string` | `KismetStringLibrary.Concat_StrStr` |
| `+` | `Vector` and `Vector` | `Vector` | `KismetMathLibrary.Add_VectorVector` |
| `+` | `Vector` and `float` | `Vector` | `KismetMathLibrary.Add_VectorFloat` |
| `+` | `Vector2D` and `Vector2D` | `Vector2D` | `KismetMathLibrary.Add_Vector2DVector2D` |
| `+` | `Vector2D` and `float` | `Vector2D` | `KismetMathLibrary.Add_Vector2DFloat` |
| `-` | `int` | `int` | `KismetMathLibrary.Subtract_IntInt` |
| `-` | `int64` | `int64` | `KismetMathLibrary.Subtract_Int64Int64` |
| `-` | `float` | `float` | `KismetMathLibrary.Subtract_DoubleDouble` |
| `-` | `Vector` and `Vector` | `Vector` | `KismetMathLibrary.Subtract_VectorVector` |
| `-` | `Vector2D` and `Vector2D` | `Vector2D` | `KismetMathLibrary.Subtract_Vector2DVector2D` |
| `*` | `int` | `int` | `KismetMathLibrary.Multiply_IntInt` |
| `*` | `int64` | `int64` | `KismetMathLibrary.Multiply_Int64Int64` |
| `*` | `float` | `float` | `KismetMathLibrary.Multiply_DoubleDouble` |
| `*` | `Vector` and `Vector` | `Vector` | `KismetMathLibrary.Multiply_VectorVector` |
| `*` | `Vector` and `float` | `Vector` | `KismetMathLibrary.Multiply_VectorFloat` |
| `*` | `Vector2D` and `Vector2D` | `Vector2D` | `KismetMathLibrary.Multiply_Vector2DVector2D` |
| `*` | `Vector2D` and `float` | `Vector2D` | `KismetMathLibrary.Multiply_Vector2DFloat` |
| `*` | `Rotator` and `float` | `Rotator` | `KismetMathLibrary.Multiply_RotatorFloat` |
| `/` | `int` | `int` | `KismetMathLibrary.Divide_IntInt` |
| `/` | `int64` | `int64` | `KismetMathLibrary.Divide_Int64Int64` |
| `/` | `float` | `float` | `KismetMathLibrary.Divide_DoubleDouble` |
| `/` | `Vector` and `Vector` | `Vector` | `KismetMathLibrary.Divide_VectorVector` |
| `/` | `Vector` and `float` | `Vector` | `KismetMathLibrary.Divide_VectorFloat` |
| `/` | `Vector2D` and `Vector2D` | `Vector2D` | `KismetMathLibrary.Divide_Vector2DVector2D` |
| `/` | `Vector2D` and `float` | `Vector2D` | `KismetMathLibrary.Divide_Vector2DFloat` |
| `%` | `int` | `int` | `KismetMathLibrary.Percent_IntInt` |
| `%` | `int64` | `int64` | `KismetMathLibrary.Percent_Int64Int64` |
| `%` | `float` | `float` | `KismetMathLibrary.Percent_FloatFloat` |
| `-x` | `int` | `int` | `KismetMathLibrary.Subtract_IntInt`, as `0 - x` |
| `-x` | `int64` | `int64` | `KismetMathLibrary.Subtract_Int64Int64`, as `0 - x` |
| `-x` | `float` | `float` | `KismetMathLibrary.Subtract_DoubleDouble`, as `0 - x` |
| `!x` | `bool` | `bool` | `KismetMathLibrary.Not_PreBool` |
| `-x` | `Vector` | `Vector` | `KismetMathLibrary.NegateVector` |
| `-x` | `Rotator` | `Rotator` | `KismetMathLibrary.NegateRotator` |

```loom
fn Examples() {
    let playing = Alive && Lives > 0
    let doubled = Speed * 2.0
    let greeting = Title + "!"
    let aimed = Target != none
    let wider = Seed + Seed
}
```
