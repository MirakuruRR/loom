//! Structured code to loom statements: the compiler's temporaries that hold a pure node's value
//! folded back into the expressions that read them, delegates bound as loom binds them, and each
//! remaining local declared where loom can see it.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use super::bytecode::Lx;
use super::code::Flow;
use crate::syntax::{Expr, ExprKind, Span, Stmt, TypeName, UnaryOp};

fn e(kind: ExprKind) -> Expr {
    Expr { kind, span: Span::default() }
}

/// How the body being emitted ends a `return`.
#[derive(Clone)]
pub enum Returns {
    /// A plain `return`.
    Plain,
    /// `return <local>`: a function whose only output is written as its return value, held in
    /// this local until then.
    Value(String),
}

pub struct Emitter {
    /// Locals whose every assignment is a pure value, which are read where they are used rather
    /// than declared.
    pub inline: HashSet<String>,
    /// Names written as other names: an event graph's copy of an event's parameter is the
    /// parameter.
    pub renames: HashMap<String, String>,
    pub returns: Returns,
    delegates: HashMap<String, (Expr, String)>,
    /// The loops around the code being emitted, outermost first, each with its label once a
    /// break or a continue from inside a loop within it has needed one.
    loops: Vec<Option<String>>,
}

impl Emitter {
    pub fn new(inline: HashSet<String>, renames: HashMap<String, String>, returns: Returns) -> Self {
        Emitter { inline, renames, returns, delegates: HashMap::new(), loops: Vec::new() }
    }

    pub fn body(&mut self, flows: &[Flow<Vec<Lx>, Expr>]) -> Result<Vec<Stmt>> {
        let mut body = self.block(flows, &HashMap::new())?;
        if let Returns::Value(local) = &self.returns {
            if !ends(&body) {
                body.push(Stmt::Return { value: Some(e(ExprKind::Name(local.clone()))), span: Span::default() });
            }
        }
        Ok(body)
    }

    fn block(&mut self, flows: &[Flow<Vec<Lx>, Expr>], outer: &HashMap<String, Expr>) -> Result<Vec<Stmt>> {
        let mut pending = outer.clone();
        let mut out = Vec::new();
        for flow in flows {
            match flow {
                Flow::Stmt { stmt, .. } | Flow::Wait { call: stmt, .. } => {
                    for lx in stmt {
                        self.lx(lx, &mut pending, &mut out)?;
                    }
                }
                Flow::If { cond, then, otherwise } => {
                    let cond = self.subst(cond, &pending);
                    let then = self.block(then, &pending)?;
                    let otherwise = self.block(otherwise, &pending)?;
                    match (then.is_empty(), otherwise.is_empty()) {
                        (true, true) => {}
                        (true, false) => out.push(Stmt::If { cond: not(cond), then: otherwise, otherwise: Vec::new(), span: Span::default() }),
                        _ => out.push(Stmt::If { cond, then, otherwise, span: Span::default() }),
                    }
                }
                Flow::Loop { body } => {
                    self.loops.push(None);
                    let body = self.block(body, &pending);
                    let label = self.loops.pop().expect("the loop pushed above");
                    let mut body = body?;
                    // A loop that tests first and leaves when the test fails is a while.
                    let cond = match body.first() {
                        Some(Stmt::If { cond, then, otherwise, .. }) if then.is_empty() && matches!(otherwise.as_slice(), [Stmt::Break { label: None, .. }]) => Some(cond.clone()),
                        Some(Stmt::If { cond, then, otherwise, .. }) if otherwise.is_empty() && matches!(then.as_slice(), [Stmt::Break { label: None, .. }]) => Some(not(cond.clone())),
                        _ => None,
                    };
                    let cond = match cond {
                        Some(c) => {
                            body.remove(0);
                            c
                        }
                        None => e(ExprKind::Bool(true)),
                    };
                    let looped = Stmt::While { cond, body, span: Span::default() };
                    out.push(match label {
                        Some(label) => Stmt::Labelled { label, looped: Box::new(looped), span: Span::default() },
                        None => looped,
                    });
                }
                Flow::Break(outward) => out.push(Stmt::Break { label: self.label(*outward), span: Span::default() }),
                Flow::Continue(outward) => out.push(Stmt::Continue { label: self.label(*outward), span: Span::default() }),
                Flow::Return => {
                    let value = match &self.returns {
                        Returns::Plain => None,
                        Returns::Value(local) => Some(e(ExprKind::Name(local.clone()))),
                    };
                    out.push(Stmt::Return { value, span: Span::default() });
                }
            }
        }
        Ok(out)
    }

    /// The label of the loop a break or a continue names, that many loops out from the innermost,
    /// which only a loop past the innermost needs. A loop is labelled by how deep it is, `loop1`
    /// the outermost.
    fn label(&mut self, outward: usize) -> Option<String> {
        if outward == 0 {
            return None;
        }
        let level = self.loops.len().checked_sub(outward + 1).expect("the structurer names only loops around the code");
        Some(self.loops[level].get_or_insert_with(|| format!("loop{}", level + 1)).clone())
    }

    fn lx(&mut self, lx: &Lx, pending: &mut HashMap<String, Expr>, out: &mut Vec<Stmt>) -> Result<()> {
        match lx {
            Lx::Assign { target, value, pure } => {
                let value = self.subst(value, pending);
                if let ExprKind::Name(n) = &target.kind {
                    let n = self.renamed(n);
                    if *pure && self.inline.contains(&n) {
                        pending.insert(n, value);
                        return Ok(());
                    }
                    pending.remove(&n);
                    out.push(Stmt::Assign { target: e(ExprKind::Name(n)), value, span: Span::default() });
                    return Ok(());
                }
                let target = self.subst(target, pending);
                out.push(Stmt::Assign { target, value, span: Span::default() });
            }
            Lx::Record { name, call } => {
                out.push(Stmt::Let { name: name.clone(), ty: None, value: self.subst(call, pending), span: Span::default() });
            }
            Lx::Do(call) => out.push(Stmt::Expr(self.subst(call, pending))),
            Lx::Bind { var, object, function } => {
                self.delegates.insert(self.renamed(var), (self.subst(object, pending), function.clone()));
            }
            Lx::AddDelegate { dispatcher, var } => {
                let Some((object, function)) = self.delegates.get(&self.renamed(var)).cloned() else {
                    unexpected!("a delegate added to a dispatcher before it was bound");
                };
                if !matches!(&object.kind, ExprKind::Name(n) if n == "self") {
                    cannot!("a dispatcher bound to another object's function");
                }
                let dispatcher = self.subst(dispatcher, pending);
                let bind = ExprKind::Method(Box::new(dispatcher), "bind".into(), vec![e(ExprKind::Name(function))]);
                out.push(Stmt::Expr(e(bind)));
            }
        }
        Ok(())
    }

    fn renamed(&self, n: &str) -> String {
        self.renames.get(n).cloned().unwrap_or_else(|| n.to_string())
    }

    /// The expression with each name renamed, and each pending local replaced by its value.
    fn subst(&self, x: &Expr, pending: &HashMap<String, Expr>) -> Expr {
        let s = |y: &Expr| Box::new(self.subst(y, pending));
        let all = |ys: &[Expr]| ys.iter().map(|y| self.subst(y, pending)).collect::<Vec<_>>();
        let kind = match &x.kind {
            ExprKind::Name(n) => {
                let n = self.renamed(n);
                if let Some(v) = pending.get(&n) {
                    return v.clone();
                }
                ExprKind::Name(n)
            }
            ExprKind::Unary(op, a) => ExprKind::Unary(*op, s(a)),
            ExprKind::Binary(op, a, b) => ExprKind::Binary(*op, s(a), s(b)),
            ExprKind::Call(n, args, quoted) => ExprKind::Call(n.clone(), all(args), *quoted),
            ExprKind::Make(n, fields) => ExprKind::Make(n.clone(), fields.iter().map(|(f, v)| (f.clone(), self.subst(v, pending))).collect()),
            ExprKind::Index(a, i) => ExprKind::Index(s(a), s(i)),
            ExprKind::ArrayLiteral(items) => ExprKind::ArrayLiteral(all(items)),
            ExprKind::MapLiteral(entries) => ExprKind::MapLiteral(entries.iter().map(|(k, v)| (self.subst(k, pending), self.subst(v, pending))).collect()),
            ExprKind::Member(o, m) => ExprKind::Member(s(o), m.clone()),
            ExprKind::Method(o, m, args) => ExprKind::Method(s(o), m.clone(), all(args)),
            ExprKind::If(c, a, b) => ExprKind::If(s(c), s(a), s(b)),
            ExprKind::Match(v, arms, other) => ExprKind::Match(s(v), arms.iter().map(|(vs, a)| (all(vs), self.subst(a, pending))).collect(), s(other)),
            other => other.clone(),
        };
        Expr { kind, span: x.span }
    }
}

fn not(cond: Expr) -> Expr {
    match cond.kind {
        ExprKind::Unary(UnaryOp::Not, inner) => *inner,
        _ => e(ExprKind::Unary(UnaryOp::Not, Box::new(cond))),
    }
}

/// Whether a body always ends in a return, so nothing can fall off its end.
fn ends(body: &[Stmt]) -> bool {
    match body.last() {
        Some(Stmt::Return { .. }) => true,
        Some(Stmt::If { then, otherwise, .. }) => ends(then) && !otherwise.is_empty() && ends(otherwise),
        _ => false,
    }
}

/// Turns each assignment of a call's value to one of `locals` that the body never reads into the
/// call on its own, and drops an assignment of a pure value that nothing reads.
pub fn drop_unread(body: &mut Vec<Stmt>, locals: &HashSet<String>) {
    let mut unread = Vec::new();
    for name in locals {
        if !reads_any(body, name) {
            unread.push(name.clone());
        }
    }
    strip(body, &unread);
}

fn strip(body: &mut Vec<Stmt>, unread: &[String]) {
    let mut out = Vec::new();
    for s in body.drain(..) {
        match s {
            Stmt::Assign { target, value, span } => match &target.kind {
                ExprKind::Name(n) if unread.contains(n) => {
                    if matches!(value.kind, ExprKind::Call(..) | ExprKind::Method(..)) {
                        out.push(Stmt::Expr(value));
                    }
                }
                _ => out.push(Stmt::Assign { target, value, span }),
            },
            Stmt::If { cond, mut then, mut otherwise, span } => {
                strip(&mut then, unread);
                strip(&mut otherwise, unread);
                out.push(Stmt::If { cond, then, otherwise, span });
            }
            Stmt::While { cond, mut body, span } => {
                strip(&mut body, unread);
                out.push(Stmt::While { cond, body, span });
            }
            Stmt::For { names, items, mut body, span } => {
                strip(&mut body, unread);
                out.push(Stmt::For { names, items, body, span });
            }
            Stmt::Labelled { label, looped, span } => {
                let mut inner = vec![*looped];
                strip(&mut inner, unread);
                let looped = Box::new(inner.pop().expect("a loop is never stripped"));
                out.push(Stmt::Labelled { label, looped, span });
            }
            other => out.push(other),
        }
    }
    *body = out;
}

/// Whether the body reads a name anywhere: in any expression other than as the whole target of
/// an assignment.
fn reads_any(body: &[Stmt], name: &str) -> bool {
    body.iter().any(|s| match s {
        Stmt::Assign { target, value, .. } => {
            mentions_expr(value, name) || (!matches!(&target.kind, ExprKind::Name(n) if n == name) && mentions_expr(target, name))
        }
        Stmt::If { cond, then, otherwise, .. } => mentions_expr(cond, name) || reads_any(then, name) || reads_any(otherwise, name),
        Stmt::While { cond, body, .. } => mentions_expr(cond, name) || reads_any(body, name),
        Stmt::For { items, body, .. } => mentions_expr(items, name) || reads_any(body, name),
        Stmt::Labelled { looped, .. } => reads_any(std::slice::from_ref(looped), name),
        other => mentions(other, name),
    })
}

/// Declares each of `locals` that the body uses: as `let name = value` where the local is first
/// set by an assignment that every later use comes after, in the innermost block that holds
/// every use, else at the top of the body, set to its zero.
pub fn declare(body: &mut Vec<Stmt>, locals: &[(String, TypeName, Expr)]) {
    let mut top = Vec::new();
    for (name, ty, zero) in locals {
        // A name a `for` binds is its own, declared by the `for`.
        if !mentions_any(body, name) || binds(body, name) {
            continue;
        }
        if !let_in_place(body, name, ty) {
            top.push(Stmt::Let { name: name.clone(), ty: Some(ty.clone()), value: zero.clone(), span: Span::default() });
        }
    }
    top.append(body);
    *body = top;
}

/// Turns the first assignment to a local into its `let`, when that is sound: the assignment is a
/// statement of the innermost block that holds every use, and comes before every other use.
fn let_in_place(block: &mut Vec<Stmt>, name: &str, ty: &TypeName) -> bool {
    let users: Vec<usize> = block.iter().enumerate().filter(|(_, s)| mentions(s, name)).map(|(i, _)| i).collect();
    let Some(&first) = users.first() else { return false };
    if users.len() == 1 {
        // Every use is inside this one statement: look in its blocks, unless it is a loop, whose
        // body runs again with the value of the pass before.
        match &mut block[first] {
            Stmt::If { then, otherwise, .. } => {
                let in_then = then.iter().any(|s| mentions(s, name));
                let in_else = otherwise.iter().any(|s| mentions(s, name));
                return match (in_then, in_else) {
                    (true, false) => let_in_place(then, name, ty),
                    (false, true) => let_in_place(otherwise, name, ty),
                    _ => false,
                };
            }
            Stmt::Assign { .. } => {}
            _ => return false,
        }
    }
    let Stmt::Assign { target, value, .. } = &block[first] else { return false };
    if !matches!(&target.kind, ExprKind::Name(n) if n == name) || mentions_expr(value, name) {
        return false;
    }
    // A value that says nothing of its type needs the type written.
    let needs_type = matches!(value.kind, ExprKind::Null | ExprKind::EmptyArray | ExprKind::ArrayLiteral(_) | ExprKind::MapLiteral(_) | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_));
    let value = value.clone();
    block[first] = Stmt::Let { name: name.to_string(), ty: needs_type.then(|| ty.clone()), value, span: Span::default() };
    true
}

/// Whether a `for` in the body binds a name.
fn binds(body: &[Stmt], name: &str) -> bool {
    body.iter().any(|s| match s {
        Stmt::For { names, body, .. } => names.iter().any(|n| n == name) || binds(body, name),
        Stmt::Count { name: n, body, .. } => n == name || binds(body, name),
        Stmt::While { body, .. } => binds(body, name),
        Stmt::If { then, otherwise, .. } => binds(then, name) || binds(otherwise, name),
        Stmt::Labelled { looped, .. } => binds(std::slice::from_ref(looped), name),
        Stmt::Match { arms, otherwise, .. } => arms.iter().any(|(_, b)| binds(b, name)) || otherwise.as_ref().is_some_and(|b| binds(b, name)),
        _ => false,
    })
}

fn mentions_any(body: &[Stmt], name: &str) -> bool {
    body.iter().any(|s| mentions(s, name))
}

pub fn mentions(s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::Let { name: n, value, .. } => n == name || mentions_expr(value, name),
        Stmt::Assign { target, value, .. } | Stmt::Compound { target, value, .. } => mentions_expr(target, name) || mentions_expr(value, name),
        Stmt::Return { value, .. } => value.as_ref().is_some_and(|v| mentions_expr(v, name)),
        Stmt::While { cond, body, .. } => mentions_expr(cond, name) || mentions_any(body, name),
        Stmt::For { names, items, body, .. } => names.iter().any(|n| n == name) || mentions_expr(items, name) || mentions_any(body, name),
        Stmt::Count { name: n, first, last, body, .. } => n == name || mentions_expr(first, name) || mentions_expr(last, name) || mentions_any(body, name),
        Stmt::If { cond, then, otherwise, .. } => mentions_expr(cond, name) || mentions_any(then, name) || mentions_any(otherwise, name),
        Stmt::Match { value, arms, otherwise, .. } => {
            mentions_expr(value, name)
                || arms.iter().any(|(vs, b)| vs.iter().any(|v| mentions_expr(v, name)) || mentions_any(b, name))
                || otherwise.as_ref().is_some_and(|b| mentions_any(b, name))
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => false,
        Stmt::Labelled { looped, .. } => mentions(looped, name),
        Stmt::Expr(x) => mentions_expr(x, name),
    }
}

pub fn mentions_expr(x: &Expr, name: &str) -> bool {
    let m = |y: &Expr| mentions_expr(y, name);
    match &x.kind {
        ExprKind::Name(n) => n == name,
        ExprKind::Unary(_, a) => m(a),
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) => m(a) || m(b),
        ExprKind::Call(_, args, _) | ExprKind::ArrayLiteral(args) => args.iter().any(m),
        ExprKind::Make(_, fields) => fields.iter().any(|(_, v)| m(v)),
        ExprKind::MapLiteral(entries) => entries.iter().any(|(k, v)| m(k) || m(v)),
        ExprKind::Member(o, _) => m(o),
        ExprKind::Method(o, _, args) => m(o) || args.iter().any(m),
        ExprKind::If(c, a, b) => m(c) || m(a) || m(b),
        ExprKind::Match(v, arms, other) => m(v) || arms.iter().any(|(vs, a)| vs.iter().any(m) || m(a)) || m(other),
        _ => false,
    }
}

/// Whether a local's first use in the body reads it, so its value comes from before the body
/// runs: for the event graph, from another event.
pub fn read_first(body: &[Stmt], name: &str) -> bool {
    walk(body, name).unwrap_or(false)
}

/// Some(true) when the first use reads the local, Some(false) when it sets it, None when the
/// body does not use it.
fn walk(body: &[Stmt], name: &str) -> Option<bool> {
    for (i, s) in body.iter().enumerate() {
        let found = match s {
            Stmt::Assign { target, value, .. } => {
                if mentions_expr(value, name) {
                    Some(true)
                } else if matches!(&target.kind, ExprKind::Name(n) if n == name) {
                    Some(false)
                } else if mentions_expr(target, name) {
                    Some(true)
                } else {
                    None
                }
            }
            Stmt::Let { name: n, value, .. } => {
                if mentions_expr(value, name) {
                    Some(true)
                } else if n == name {
                    Some(false)
                } else {
                    None
                }
            }
            Stmt::If { cond, then, otherwise, .. } => {
                if mentions_expr(cond, name) {
                    Some(true)
                } else {
                    match (walk(then, name), walk(otherwise, name)) {
                        (Some(true), _) | (_, Some(true)) => Some(true),
                        (Some(false), Some(false)) => Some(false),
                        // Set on one side only: the other side goes on with the value from
                        // before, so it is read first when anything after the if reads it.
                        (Some(false), None) | (None, Some(false)) => Some(mentions_any(&body[i + 1..], name)),
                        (None, None) => None,
                    }
                }
            }
            Stmt::While { cond, body, .. } => {
                if mentions_expr(cond, name) {
                    Some(true)
                } else {
                    walk(body, name)
                }
            }
            Stmt::Labelled { looped, .. } => walk(std::slice::from_ref(looped), name),
            // A name a `for` binds is its own there, not the local.
            Stmt::For { names, items, body, .. } => {
                if mentions_expr(items, name) {
                    Some(true)
                } else if names.iter().any(|n| n == name) {
                    None
                } else {
                    walk(body, name)
                }
            }
            Stmt::Count { name: n, first, last, body, .. } => {
                if mentions_expr(first, name) || mentions_expr(last, name) {
                    Some(true)
                } else if n == name {
                    None
                } else {
                    walk(body, name)
                }
            }
            other if mentions(other, name) => Some(true),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jump_past_the_innermost_loop_labels_the_loop_it_names() {
        let flows: Vec<Flow<Vec<Lx>, Expr>> = vec![Flow::Loop {
            body: vec![Flow::Loop { body: vec![Flow::If { cond: e(ExprKind::Bool(true)), then: vec![Flow::Break(1)], otherwise: vec![] }] }],
        }];
        let body = Emitter::new(HashSet::new(), HashMap::new(), Returns::Plain).body(&flows).unwrap();
        let [Stmt::Labelled { label, looped, .. }] = body.as_slice() else { panic!("{body:?}") };
        assert_eq!(label, "loop1");
        let Stmt::While { body: outer, .. } = looped.as_ref() else { panic!("{looped:?}") };
        // The inner loop's break names another loop, so it is not the inner loop's condition.
        let [Stmt::While { body: inner, .. }] = outer.as_slice() else { panic!("{outer:?}") };
        let [Stmt::If { then, .. }] = inner.as_slice() else { panic!("{inner:?}") };
        assert!(matches!(then.as_slice(), [Stmt::Break { label: Some(l), .. }] if l == "loop1"), "{then:?}");
    }
}
