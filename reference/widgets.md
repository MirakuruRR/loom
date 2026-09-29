---
title: Widgets
description: Widget Blueprints and their trees.
sidebar:
  order: 7
---

A Blueprint whose parent is `UserWidget` is a widget Blueprint. `widget Name: Class` adds a widget
to its tree, and each widget is a variable of the Blueprint.

## Placing widgets

`at x y size w h` places a widget on the tree's root canvas. A widget with no place is the tree's
root, and then it is the only widget.

```loom
blueprint WBP_Score : UserWidget at /Game/UI/WBP_Score

widget Score: TextBlock(Text: "0", ColorAndOpacity: "#ffcc00") at 20 20 size 200 40
widget Picture: Image at 20 80 size 160 100

fn Show(points: int) {
    Score.SetText(text(string(points)))
}
```

```loom
blueprint WBP_Title : UserWidget at /Game/UI/WBP_Title

widget Title: TextBlock(Text: "Hello")
```

## Widgets inside widgets

A widget that holds others, such as a button or a box, holds them in a block after it. The
widgets in the block are its children, in order, and a button, a size box or any other widget that
holds one widget takes only one. `slot(field: value, ...)` sets the fields of the slot its parent
holds it in, such as its padding and its alignment, which depend on the parent: a child of a
button is held by a `ButtonSlot`, and a child of a horizontal box by a `HorizontalBoxSlot`.

```loom
blueprint WBP_Toggle : UserWidget at /Game/UI/WBP_Toggle

widget Toggle: Button {
    widget Row: HorizontalBox slot(Padding: Margin(Left: 4, Top: 2, Right: 4, Bottom: 2)) {
        widget Icon: Image(ColorAndOpacity: "#dfc07d") slot(VerticalAlignment: EVerticalAlignment.VAlign_Center)
        widget Label: TextBlock(Text: "Show") slot(Padding: Margin(Left: 6))
    }
}
```

## Creating a widget

`create_widget(Class)` makes a widget of one of them.

```loom program="hud"
blueprint WBP_Hud : UserWidget at /Game/UI/WBP_Hud

widget Label: TextBlock at 0 0 size 200 40
```

```loom program="hud"
blueprint BP_ShowHud : Actor at /Game/UI/BP_ShowHud

on ReceiveBeginPlay {
    let hud = create_widget(WBP_Hud)
    hud.AddToViewport(10)
}
```
