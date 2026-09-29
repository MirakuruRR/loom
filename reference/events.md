---
title: Events
description: Handling events, and binding to dispatchers.
sidebar:
  order: 6
---

## Handling an event

`on Event { ... }` handles an event the parent class declares. When the event passes values, the
handler names each of them, in order, with its type.

```loom
var Time: float = 0.0

on ReceiveBeginPlay {
    Time = 0.0
}

on ReceiveTick(dt: float) {
    Time = Time + dt
}
```

A handler that leaves out an event's values is refused, and the error says what the event passes:

```loom error="DeltaSeconds"
on ReceiveTick {
}
```

## Waiting

A call that waits, such as `Delay`, pauses the handler, which goes on from there once the wait is
over. Only an event's handler can wait. Loom lays a handler that waits out in the Blueprint's
event graph, where Unreal can pause it, rather than in a function of its own, and makes each of
its `let` names a variable of the Blueprint.

```loom
var Ready: bool = false

on ReceiveBeginPlay {
    KismetSystemLibrary.Delay(2.0)
    Ready = true
}
```

The event can fire again while its handler waits. The handler then runs again up to the wait,
and the call decides what happens there, as its node does in the editor: `Delay` goes on waiting
as before, and `RetriggerableDelay` starts its wait over. Since the handler's `let` names are
variables, the second run changes them for the first.

A function can't wait:

```loom error="Delay waits, so it can only be called in an event's"
fn Later() {
    KismetSystemLibrary.Delay(1.0)
}
```

## Custom events

`event Name(params) { }` declares an event of the Blueprint's own, as the editor's Add Custom Event
does. It is called as a function is, and other Blueprints can call it too, but it gives nothing
back, and it may wait, since it runs in the event graph as a handler does. A call to it goes on as
soon as the event waits, while the event carries on later.

```loom
var Lit: bool = false

event Pulse(times: int) {
    for i in 1..times {
        Lit = !Lit
        KismetSystemLibrary.Delay(0.5)
    }
}

on ReceiveBeginPlay {
    Pulse(3)
}
```

An event's parameters are values, and it has no `-> Type`:

```loom error="an event gives nothing back"
event Total(a: int) -> int {
}
```

## An interface's functions

A handler whose name the parent doesn't declare implements that function of an interface the
Blueprint [implements](/reference/#implementing-an-interface), or its parent does. It is written
as any handler is. A function that returns nothing is an event, as it is in the editor, and one
that returns a value, or is const, is a function of the Blueprint, as an
[event that returns a value](/reference/editor/#events-that-return-a-value) is.

```loom
blueprint BP_Simulated : Actor at /Game/Sim/BP_Simulated
implements DataflowSimulationActor
implements MovieSceneEasingFunction

var Steps: int = 0

on PreDataflowSimulationTick(time: float, dt: float) {
    Steps = Steps + 1
}

on OnEvaluate(t: float) -> float {
    return t * 2
}
```

A function left without a handler does nothing, and one that returns a value returns its type's
zero, as in the editor. A handler that matches nothing the parent or the interfaces declare is
refused:

```loom error="neither has any interface it implements"
blueprint BP_Simulated : Actor at /Game/Sim/BP_Simulated
implements MovieSceneEasingFunction

on OnStep() {
}
```

## The parent's version

A handler replaces the parent's own handling of the event. `super.Event(values)`, in the handler,
runs the parent's as well, as the editor's "Add call to parent function" does. The same goes for a
function that overrides one of the parent's.

```loom program="doors"
blueprint BP_Door : Actor at /Game/Doors/BP_Door

var Opened: bool = false

on ReceiveBeginPlay {
    Opened = false
}
```

```loom program="doors"
blueprint BP_GreatDoor : BP_Door at /Game/Doors/BP_GreatDoor

var Visitors: int = 0

on ReceiveBeginPlay {
    super.ReceiveBeginPlay()
    Visitors = 0
}
```

`super` is only for the event or function being overridden:

```loom error="goes in the"
on ReceiveBeginPlay {
    super.ReceiveTick(0.0)
}
```

## A component's dispatcher

`on Component.Dispatcher` handles a dispatcher of one of the Blueprint's components, as its green
`+` in the Details panel does.

```loom
component Sound: AudioComponent
var Done: bool = false

on Sound.OnAudioFinished {
    Done = true
}
```

## Binding a dispatcher

`object.Dispatcher.bind(Function)` has an object's dispatcher call a function from then on, each
time it broadcasts, as the editor's Bind Event does. `unbind(Function)` stops it calling that
function, and `unbind_all()` stops it calling anything. A function bound twice is bound once.

```loom
component Sound: AudioComponent
var Plays: int = 0

fn Finished() {
    Plays = Plays + 1
}

on ReceiveBeginPlay {
    Sound.OnAudioFinished.bind(Finished)
}

fn Quiet() {
    Sound.OnAudioFinished.unbind(Finished)
}
```

This Blueprint's own [dispatcher](/reference/members/#dispatchers) is named on its own, and one it
inherits through `self`:

```loom
dispatcher Scored(points: int)
var Total: int = 0

fn Add(points: int) {
    Total += points
}

fn Gone(actor: Actor) {
    Total = 0
}

on ReceiveBeginPlay {
    Scored.bind(Add)
    self.OnDestroyed.bind(Gone)
}

fn Reset() {
    Scored.unbind_all()
}
```

The function is one of this Blueprint's, one of its parent's, or with `object.Function`, another
object's. It takes exactly what the dispatcher passes, as Blueprint requires: the same types, in
order, each passed the same way, and returns nothing. A value the engine passes by const reference,
such as a hit's `HitResult`, is a `ref` parameter:

```loom
component Box: BoxComponent
var Hits: int = 0

fn Hit(me: PrimitiveComponent, other: Actor, part: PrimitiveComponent, impulse: Vector, hit: ref HitResult) {
    Hits += 1
}

on ReceiveBeginPlay {
    Box.OnComponentHit.bind(Hit)
}
```

```loom error="Add is fn(), and a function bound to Scored must be fn(points: int)"
dispatcher Scored(points: int)

fn Add() {
}

on ReceiveBeginPlay {
    Scored.bind(Add)
}
```

Where the engine passes a 32-bit float, as it often does, loom makes a function of this
Blueprint's own that is bound to it take a 32-bit float there, as the editor makes a function to
bind. Blueprint then binds the function itself, and `unbind` finds it. The function can't also be
bound where a 64-bit float is passed, such as by this Blueprint's own dispatchers, whose floats are
loom's. Any other function must take a 32-bit float there already.

```loom
var Health: float = 100.0

fn Hurt(actor: Actor, damage: float, kind: DamageType, by: Controller, causer: Actor) {
    Health -= damage
}

on ReceiveBeginPlay {
    self.OnTakeAnyDamage.bind(Hurt)
}
```

```loom error="which passes a 64-bit one; bind a function to each"
component Fade: TimelineComponent
dispatcher Faded(amount: float)
var Alpha: float = 0.0

fn Set(amount: float) {
    Alpha = amount
}

fn Start(curve: CurveFloat) {
    Faded.bind(Set)
    Fade.AddInterpFloat(curve, Set)
}
```

## Functions as delegates

A function named on its own, where one of the engine's functions takes a delegate, is a delegate
to it, as the editor's Create Event makes one. Set Timer by Event is one such function:

```loom
var Ticks: int = 0

fn Count() {
    Ticks += 1
}

on ReceiveBeginPlay {
    KismetSystemLibrary.K2_SetTimerDelegate(Count, 0.5, true)
}

fn Later(other: Actor) {
    KismetSystemLibrary.K2_SetTimerDelegate(other.K2_DestroyActor, 5.0, false)
}
```

The function is found and checked as for [binding a dispatcher](#binding-a-dispatcher): it takes
exactly what the delegate passes, and returns what it returns. A function
[takes and returns](/reference/types/#delegates) a delegate by its type, to pass it on. Blueprint
has no way to call a delegate, or to bind a function that is pure, that waits, or that is static:

```loom error="Count is fn(times: int), and a function bound to TimerDynamicDelegate must be fn()" refusals="delegates"
blueprint BP_Clock : Actor at /Game/BP_Clock

fn Count(times: int) {
}

on ReceiveBeginPlay {
    KismetSystemLibrary.K2_SetTimerDelegate(Count, 0.5, true)
}
```

```loom error="Count is pure" refusals="delegates"
blueprint BP_Clock : Actor at /Game/BP_Clock

pure fn Count() -> int {
    return 1
}

on ReceiveBeginPlay {
    KismetSystemLibrary.K2_SetTimerDelegate(Count, 0.5, true)
}
```

```loom error="PrintString is static" refusals="delegates"
blueprint BP_Clock : Actor at /Game/BP_Clock

on ReceiveBeginPlay {
    KismetSystemLibrary.K2_SetTimerDelegate(KismetSystemLibrary.PrintString, 0.5, true)
}
```
