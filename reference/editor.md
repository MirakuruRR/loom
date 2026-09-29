---
title: Editor Blueprints
description: Blueprints that run only in the editor.
sidebar:
  order: 8
---

`editor blueprint` declares a Blueprint whose parent runs only in the editor, such as a validator.
It may use the editor's classes, and the game never loads it. A Blueprint that is not an editor
Blueprint cannot have an editor-only parent.

## Events that return a value

An event that returns a value, such as a validator's check, names its return type.

```loom
editor blueprint BP_NoEmptyNames : EditorValidatorBase at /Game/BP_NoEmptyNames

on K2_CanValidateAsset(asset: Object) -> bool {
    return asset != none
}
```

## Subsystems

`subsystem(Class)` is the engine's or the editor's subsystem of that class.

```loom
editor blueprint BP_Tool : EditorUtilityObject at /Game/Checks/BP_Tool

fn Selected() -> int {
    let assets = subsystem(EditorAssetSubsystem)
    return 0
}
```
