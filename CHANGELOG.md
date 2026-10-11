# Changelog

## Unreleased

### The language

- A struct literal fills in the fields it leaves out: `Rotator(Yaw: 90.0)` and `Rotator()` compile, the
  fields left out being their type's zero, or the default the struct's own make function gives them
  (a `Transform` left without its Scale has a scale of 1). The reference promised this already.
- Operators on a struct, carried out by the engine's own functions: `+`, `-`, `*`, `/`, `==` and `!=`
  on a `Vector` or a `Vector2D`, `* float` and `/ float` with one, `* float`, `==` and `!=` on a
  `Rotator`, and `-x`. Whichever side the struct is written on it is the call's first argument, so
  `2.0 * Reach` is `Reach * 2.0`. An operator the struct has not is refused with the list of the ones
  it has. The engine has no `Subtract_VectorFloat`, so `v - 2.0` stays refused.
- `int64` has its operators: `+ - * / %`, comparisons, `& |` and `-x`, and a whole number written
  beside an `int64` is one, as beside a `float` it is a `float`.
- A `class<T>` and a soft reference compare with `none`.
- `Key(KeyName: "Escape")` works in an expression, not only as a variable's default; a key's own name
  still cannot be read, since Blueprint has no pin for it.
- A field of a `ref` struct parameter is set in place, with a Set Members node on the parameter
  instead of rebuilding the struct, which a make function refuses. A parameter passed by value still
  cannot be assigned.
- `format(pattern, value, ...)` gives `text`, showing numbers and strings in the pattern's `{0}`,
  `{1}`, … placeholders, which the engine's `Format` node carries out.
- An empty array literal as a local stays what it was — emptying in place — while a `set`'s or a
  `map`'s empty literal, which Blueprint cannot make, is refused with what to write instead.

### Fixed

- A `Key` variable's default is written as the key's own name: `var CloseKey: Key = Key(KeyName:
  "Escape")` is `Escape`. The struct literal it used to write, `(KeyName="Escape")`, is imported as
  the single character `(` — a built Blueprint's name table holds `(` and no `Escape`, so such a key
  never matched the key it named. A key in an expression and a key given as an argument were already
  written by name.

### Checks and messages

- `loom check` names a cycle between Blueprints itself: `these Blueprints use each other in a cycle:
  BP_MapLoad uses WBP_Chains, and WBP_Chains uses BP_MapLoad`. `loom build` no longer writes
  operations for a program it cannot order.
- `loom check` reports every source's errors. A file that does not parse no longer hides the type
  errors of the others; the errors that named one of its types are left out instead of cascading.
- A struct field the engine reads through a function says which names to use: `Transform has no
  Translation` adds `Transform's fields are read through BreakTransform, whose outputs are Location
  (Vector), Rotation (Rotator) and Scale (Vector): write BreakTransform(value).Location`. A name a
  make function does not take is refused with the names it does.
- `loom types <Struct>` prints those two lines for a struct built and read by functions.
- An empty `[]`, `{}` or `none` is refused where Blueprint has no node to wire one — a local `set` or
  `map`, a struct's field, a call's argument, a return — with what to write instead. A Blueprint
  variable's defaulting to `[]` and `let more: array<int> = []` still work.
- `loom check --json`'s `missing` lists only what the types really lack: the builtins (`cast`,
  `float`, `string`) and the names that resolved are no longer in it, so whatever loads the types is
  no longer sent after them.

## 0.1.0 · 2026-09-29

Initial release.