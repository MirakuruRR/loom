//! A function's bytecode as the structurer sees it, and the structured code it makes of it.
//!
//! Only the instructions that move between statements are spelled out here. Everything else is a
//! statement, `S`, or a condition, `C`, which the structurer carries without looking inside.

/// One function's bytecode, in offset order: each instruction with the code offset it starts at.
/// Jumps name these offsets.
#[derive(Debug, Clone)]
pub struct Code<S, C> {
    pub insns: Vec<(usize, Insn<S, C>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Insn<S, C> {
    /// Anything that runs and goes on to the next instruction.
    Stmt(S),
    /// A call that waits, such as `Delay`. The VM goes on to the next instruction at once, which
    /// normally ends the thread, and once the wait is over the event graph is entered again at
    /// `resume`, with the flow stack as the graph's preamble leaves it.
    Latent { call: S, resume: usize },
    /// `EX_Jump`: goes on at the offset.
    Jump(usize),
    /// `EX_JumpIfNot`: goes on at the offset when the condition is false, else at the next
    /// instruction.
    JumpIfNot(C, usize),
    /// `EX_PushExecutionFlow`: pushes the offset on the flow stack and goes on to the next
    /// instruction. A Sequence node pushes its later pins, and a function pushes its return.
    Push(usize),
    /// `EX_PopExecutionFlow`: the end of a thread. It goes on at the offset it pops, or ends the
    /// function when the stack is empty.
    Pop,
    /// `EX_PopExecutionFlowIfNot`: pops, as `Pop` does, when the condition is false, else goes on
    /// to the next instruction. A Branch whose false pin is not wired compiles to it.
    PopIfNot(C),
    /// `EX_Return`: leaves the function. Out parameters and the return value are assigned by
    /// statements before it.
    Return,
    /// `EX_ComputedJump`: the event graph's jump to the entry point it was called with. It only
    /// ends the graph's preamble.
    ComputedJump,
    /// `EX_EndOfScript`: the end of the bytecode, never reached.
    End,
}

/// Structured code: what a function or an event's handler does, as a loom body would say it.
#[derive(Debug, Clone, PartialEq)]
pub enum Flow<S, C> {
    /// A statement, with the offset of the instruction it came from.
    Stmt { at: usize, stmt: S },
    /// A call that waits. What follows it runs once the wait is over.
    Wait { at: usize, call: S },
    /// `then` when the condition holds, else `otherwise`. Either may be empty.
    If { cond: C, then: Vec<Flow<S, C>>, otherwise: Vec<Flow<S, C>> },
    /// Runs its body again and again, until a `Break` or a `Return` leaves it.
    Loop { body: Vec<Flow<S, C>> },
    /// Leaves a `Loop`: the innermost with 0, the one around it with 1, and so on out.
    Break(usize),
    /// Starts a `Loop`'s next pass, counting loops out as `Break` does.
    Continue(usize),
    /// Leaves the function or the handler. Never the last item of the body it ends, since
    /// falling off the end does the same.
    Return,
}
