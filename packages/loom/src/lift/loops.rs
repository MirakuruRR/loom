//! The loops Blueprint's macros make, written back as loom's `for`.
//!
//! For Each counts through an array with a counter of its own and hands the body the index:
//!
//! ```text
//! counter = 0
//! index = 0
//! while counter < items.length() {
//!     index = counter
//!     ...items[index]...
//!     counter = counter + 1
//! }
//! ```
//!
//! and For Loop counts a temporary from its first index to its last, both included, reading the
//! last again before each pass:
//!
//! ```text
//! count = first
//! while count <= last {
//!     ...
//!     count = count + 1
//! }
//! ```
//!
//! Loom's `for item, i in items` and `for i in first..last` do the same, step for step, so a
//! loop of either shape is written as one when that loses nothing: its counters are the
//! function's own locals, nothing else reads or sets them, and nothing in the body starts the
//! next pass early, which in the while would skip the step.

use std::collections::HashSet;

use super::emit::{mentions, mentions_expr};
use crate::syntax::{BinaryOp, Expr, ExprKind, Span, Stmt};

/// The name For Each gives the item it hands the body, which the item is named after here.
const ITEM: &str = "arrayElement";

/// Writes each loop of the macros' shapes in `body` as a `for`, where `locals` are the locals
/// that the body alone holds, which a `for` may take over.
pub fn recover(body: &mut Vec<Stmt>, locals: &HashSet<String>) {
    let mut tried = HashSet::new();
    loop {
        let mut next = body.clone();
        let item = item_name(body);
        let Some(counter) = convert_first(&mut next, locals, &tried, &item) else { return };
        tried.insert(counter.counter.clone());
        if counter.stays_inside(&next) {
            *body = next;
        }
    }
}

/// The names a converted loop took over: they must now appear nowhere but in it.
struct Converted {
    /// For Each's counter, or For Loop's count, which the `for` names itself.
    counter: String,
    /// For Each's index, which the `for` names when the body reads it.
    index: Option<String>,
}

impl Converted {
    fn stays_inside(&self, body: &[Stmt]) -> bool {
        let names: Vec<&String> = std::iter::once(&self.counter).chain(&self.index).collect();
        names.iter().all(|name| !outside_binding(body, name))
    }
}

/// Whether a name is used anywhere but inside a `for` that binds it.
fn outside_binding(body: &[Stmt], name: &str) -> bool {
    body.iter().any(|s| match s {
        Stmt::For { names, .. } if names.iter().any(|n| n == name) => false,
        Stmt::Count { name: n, .. } if n == name => false,
        Stmt::For { items, body, .. } => mentions_expr(items, name) || outside_binding(body, name),
        Stmt::Count { first, last, body, .. } => mentions_expr(first, name) || mentions_expr(last, name) || outside_binding(body, name),
        Stmt::While { cond, body, .. } => mentions_expr(cond, name) || outside_binding(body, name),
        Stmt::If { cond, then, otherwise, .. } => mentions_expr(cond, name) || outside_binding(then, name) || outside_binding(otherwise, name),
        Stmt::Labelled { looped, .. } => outside_binding(std::slice::from_ref(looped), name),
        Stmt::Match { value, arms, otherwise, .. } => {
            mentions_expr(value, name)
                || arms.iter().any(|(vs, b)| vs.iter().any(|v| mentions_expr(v, name)) || outside_binding(b, name))
                || otherwise.as_ref().is_some_and(|b| outside_binding(b, name))
        }
        other => mentions(other, name),
    })
}

/// A name for For Each's item that nothing in the body has.
fn item_name(body: &[Stmt]) -> String {
    let mut name = ITEM.to_string();
    let mut i = 2;
    while body.iter().any(|s| mentions(s, &name)) {
        name = format!("{ITEM}{i}");
        i += 1;
    }
    name
}

/// Converts the first loop of either shape, in the body or any block inside it, whose counter
/// is not in `tried`.
fn convert_first(block: &mut Vec<Stmt>, locals: &HashSet<String>, tried: &HashSet<String>, item: &str) -> Option<Converted> {
    for k in 0..block.len() {
        if let Some(done) = for_each(block, k, locals, tried, item).or_else(|| for_loop(block, k, locals, tried)) {
            return Some(done);
        }
        let found = match &mut block[k] {
            Stmt::If { then, otherwise, .. } => convert_first(then, locals, tried, item).or_else(|| convert_first(otherwise, locals, tried, item)),
            Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::Count { body, .. } => convert_first(body, locals, tried, item),
            Stmt::Labelled { looped, .. } => match looped.as_mut() {
                Stmt::While { body, .. } => convert_first(body, locals, tried, item),
                _ => None,
            },
            Stmt::Match { arms, otherwise, .. } => {
                arms.iter_mut().find_map(|(_, b)| convert_first(b, locals, tried, item)).or_else(|| otherwise.as_mut().and_then(|b| convert_first(b, locals, tried, item)))
            }
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn e(kind: ExprKind) -> Expr {
    Expr { kind, span: Span::default() }
}

fn is_name(x: &Expr, name: &str) -> bool {
    matches!(&x.kind, ExprKind::Name(n) if n == name)
}

/// The name a statement sets to a value: `name = value`.
fn assigns(s: &Stmt) -> Option<(&str, &Expr)> {
    match s {
        Stmt::Assign { target, value, .. } => match &target.kind {
            ExprKind::Name(n) => Some((n, value)),
            _ => None,
        },
        _ => None,
    }
}

/// Whether a statement is `name = name + 1`.
fn steps(s: &Stmt, name: &str) -> bool {
    match assigns(s) {
        Some((n, value)) if n == name => {
            matches!(&value.kind, ExprKind::Binary(BinaryOp::Add, a, b) if is_name(a, name) && matches!(b.kind, ExprKind::Int(1)))
        }
        _ => false,
    }
}

/// Whether code sets a name anywhere.
fn sets(body: &[Stmt], name: &str) -> bool {
    body.iter().any(|s| match s {
        Stmt::Assign { target, .. } | Stmt::Compound { target, .. } => is_name(target, name),
        Stmt::Let { name: n, .. } => n == name,
        Stmt::If { then, otherwise, .. } => sets(then, name) || sets(otherwise, name),
        Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::Count { body, .. } => sets(body, name),
        Stmt::Labelled { looped, .. } => sets(std::slice::from_ref(looped), name),
        Stmt::Match { arms, otherwise, .. } => arms.iter().any(|(_, b)| sets(b, name)) || otherwise.as_ref().is_some_and(|b| sets(b, name)),
        _ => false,
    })
}

/// Whether code starts its own loop's next pass: a `continue` that names no loop, outside any
/// loop of the code's own.
fn continues(body: &[Stmt]) -> bool {
    body.iter().any(|s| match s {
        Stmt::Continue { label: None, .. } => true,
        Stmt::If { then, otherwise, .. } => continues(then) || continues(otherwise),
        Stmt::Match { arms, otherwise, .. } => arms.iter().any(|(_, b)| continues(b)) || otherwise.as_ref().is_some_and(|b| continues(b)),
        _ => false,
    })
}

/// Of the two statements right before `at`, the places of those that set one of `names`, each
/// once, with the name and what it is set to.
fn inits(block: &[Stmt], at: usize, names: &[&str]) -> Vec<(usize, String, Expr)> {
    let mut found: Vec<(usize, String, Expr)> = Vec::new();
    for before in (at.saturating_sub(2)..at).rev() {
        match assigns(&block[before]) {
            Some((n, value)) if names.contains(&n) && !found.iter().any(|(_, t, _)| t == n) => found.push((before, n.to_string(), value.clone())),
            _ => break,
        }
    }
    found
}

/// For Each at `block[k]`, written as `for item[, index] in items`.
fn for_each(block: &mut Vec<Stmt>, k: usize, locals: &HashSet<String>, tried: &HashSet<String>, item: &str) -> Option<Converted> {
    let Stmt::While { cond, body, .. } = &block[k] else { return None };
    let ExprKind::Binary(BinaryOp::Lt, counter, length) = &cond.kind else { return None };
    let ExprKind::Name(counter) = &counter.kind else { return None };
    let ExprKind::Method(items, method, args) = &length.kind else { return None };
    if method != "Length" || !args.is_empty() || body.len() < 2 {
        return None;
    }
    let (index, from) = assigns(&body[0])?;
    let (counter, index, items) = (counter.clone(), index.to_string(), (**items).clone());
    if !is_name(from, &counter) || !steps(body.last()?, &counter) || tried.contains(&counter) {
        return None;
    }
    if counter == index || !locals.contains(&counter) || !locals.contains(&index) {
        return None;
    }
    let inner = &body[1..body.len() - 1];
    if mentions_expr(&items, &counter) || mentions_expr(&items, &index) || inner.iter().any(|s| mentions(s, &counter)) || sets(inner, &index) || continues(inner) {
        return None;
    }
    // For Each starts its counter at 0 right before the loop, and sets its index there too.
    let starts = inits(block, k, &[&counter, &index]);
    if !starts.iter().any(|(_, n, v)| *n == counter && matches!(v.kind, ExprKind::Int(0))) {
        return None;
    }
    let mut inner = inner.to_vec();
    let at = Expr { kind: ExprKind::Index(Box::new(items.clone()), Box::new(e(ExprKind::Name(index.clone())))), span: Span::default() };
    for s in &mut inner {
        replace_stmt(s, &at, &e(ExprKind::Name(item.to_string())));
    }
    let reads_index = inner.iter().any(|s| mentions(s, &index));
    let mut names = vec![item.to_string()];
    if reads_index {
        names.push(index.clone());
    }
    let span = Span::default();
    block[k] = Stmt::For { names, items, body: inner, span };
    // The starts are before the loop, highest first, so each removal leaves the next in place.
    for (before, ..) in starts {
        block.remove(before);
    }
    Some(Converted { counter, index: Some(index) })
}

/// For Loop at `block[k]`, written as `for count in first..last`.
fn for_loop(block: &mut Vec<Stmt>, k: usize, locals: &HashSet<String>, tried: &HashSet<String>) -> Option<Converted> {
    let Stmt::While { cond, body, .. } = &block[k] else { return None };
    let ExprKind::Binary(BinaryOp::Le, count, last) = &cond.kind else { return None };
    let ExprKind::Name(count) = &count.kind else { return None };
    let (count, last) = (count.clone(), (**last).clone());
    if !steps(body.last()?, &count) || tried.contains(&count) || !locals.contains(&count) || mentions_expr(&last, &count) {
        return None;
    }
    let inner = body[..body.len() - 1].to_vec();
    if sets(&inner, &count) || continues(&inner) {
        return None;
    }
    let (first, before) = match k.checked_sub(1).map(|b| assigns(&block[b])) {
        Some(Some((n, first))) if n == count && !mentions_expr(first, &count) => (first.clone(), k - 1),
        _ => return None,
    };
    block[k] = Stmt::Count { name: count.clone(), first, last, body: inner, span: Span::default() };
    block.remove(before);
    Some(Converted { counter: count, index: None })
}

fn replace_stmt(s: &mut Stmt, from: &Expr, to: &Expr) {
    let r = |x: &mut Expr| replace_expr(x, from, to);
    match s {
        Stmt::Let { value, .. } => r(value),
        Stmt::Assign { target, value, .. } | Stmt::Compound { target, value, .. } => {
            r(target);
            r(value);
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                r(v);
            }
        }
        Stmt::While { cond, body, .. } => {
            r(cond);
            body.iter_mut().for_each(|s| replace_stmt(s, from, to));
        }
        Stmt::For { items, body, .. } => {
            r(items);
            body.iter_mut().for_each(|s| replace_stmt(s, from, to));
        }
        Stmt::Count { first, last, body, .. } => {
            r(first);
            r(last);
            body.iter_mut().for_each(|s| replace_stmt(s, from, to));
        }
        Stmt::If { cond, then, otherwise, .. } => {
            r(cond);
            then.iter_mut().chain(otherwise.iter_mut()).for_each(|s| replace_stmt(s, from, to));
        }
        Stmt::Match { value, arms, otherwise, .. } => {
            r(value);
            for (values, b) in arms.iter_mut() {
                values.iter_mut().for_each(|v| replace_expr(v, from, to));
                b.iter_mut().for_each(|s| replace_stmt(s, from, to));
            }
            if let Some(b) = otherwise {
                b.iter_mut().for_each(|s| replace_stmt(s, from, to));
            }
        }
        Stmt::Labelled { looped, .. } => replace_stmt(looped, from, to),
        Stmt::Expr(x) => r(x),
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
    }
}

/// An expression with every span cleared, so two written in different places compare equal.
fn unspanned(x: &Expr) -> Expr {
    let mut x = x.clone();
    clear(&mut x);
    x
}

fn clear(x: &mut Expr) {
    x.span = Span::default();
    match &mut x.kind {
        ExprKind::Unary(_, a) | ExprKind::Member(a, _) => clear(a),
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) => {
            clear(a);
            clear(b);
        }
        ExprKind::Call(_, args, _) | ExprKind::ArrayLiteral(args) => args.iter_mut().for_each(clear),
        ExprKind::Make(_, fields) => fields.iter_mut().for_each(|(_, v)| clear(v)),
        ExprKind::MapLiteral(entries) => entries.iter_mut().for_each(|(k, v)| {
            clear(k);
            clear(v);
        }),
        ExprKind::Method(o, _, args) => {
            clear(o);
            args.iter_mut().for_each(clear);
        }
        ExprKind::If(c, a, b) => {
            clear(c);
            clear(a);
            clear(b);
        }
        ExprKind::Match(v, arms, other) => {
            clear(v);
            for (values, a) in arms.iter_mut() {
                values.iter_mut().for_each(clear);
                clear(a);
            }
            clear(other);
        }
        _ => {}
    }
}

/// Replaces each expression written as `from` with `to`, wherever it was written.
fn replace_expr(x: &mut Expr, from: &Expr, to: &Expr) {
    if unspanned(x) == unspanned(from) {
        *x = to.clone();
        return;
    }
    let r = |y: &mut Expr| replace_expr(y, from, to);
    match &mut x.kind {
        ExprKind::Unary(_, a) | ExprKind::Member(a, _) => r(a),
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) => {
            r(a);
            r(b);
        }
        ExprKind::Call(_, args, _) | ExprKind::ArrayLiteral(args) => args.iter_mut().for_each(r),
        ExprKind::Make(_, fields) => fields.iter_mut().for_each(|(_, v)| r(v)),
        ExprKind::MapLiteral(entries) => entries.iter_mut().for_each(|(k, v)| {
            replace_expr(k, from, to);
            replace_expr(v, from, to);
        }),
        ExprKind::Method(o, _, args) => {
            r(o);
            args.iter_mut().for_each(|a| replace_expr(a, from, to));
        }
        ExprKind::If(c, a, b) => {
            r(c);
            replace_expr(a, from, to);
            replace_expr(b, from, to);
        }
        ExprKind::Match(v, arms, other) => {
            r(v);
            for (values, a) in arms.iter_mut() {
                values.iter_mut().for_each(|v| replace_expr(v, from, to));
                replace_expr(a, from, to);
            }
            replace_expr(other, from, to);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{parse_source, Item, Source};

    /// `body` with its loops recovered, taking over any of `locals`, as printed.
    fn recovered(body: &str, locals: &[&str]) -> String {
        let text = format!("blueprint B : Actor at /Game/B\nvar Items: array<int>\nvar Total: int\nfn F(first: int, last: int) {{\n{body}}}\n");
        let Source::Blueprint(mut file) = parse_source(&text).unwrap() else { unreachable!() };
        let Some(Item::Fn { body, .. }) = file.items.iter_mut().find(|i| matches!(i, Item::Fn { .. })) else { unreachable!() };
        recover(body, &locals.iter().map(|l| l.to_string()).collect());
        let printed = super::super::print::source(&Source::Blueprint(file));
        let inside = printed.split_once("{\n").unwrap().1;
        inside.strip_suffix("}\n").unwrap().to_string()
    }

    const FOR_EACH: &str = "    c = 0\n    i = 0\n    while c < Items.Length() {\n        i = c\n        Total = Total + Items[i] * i\n        c = c + 1\n    }\n";

    #[test]
    fn for_each_is_a_for_over_the_array() {
        assert_eq!(recovered(FOR_EACH, &["c", "i"]), "    for arrayElement, i in Items {\n        Total = Total + arrayElement * i\n    }\n");
        // Without reading the index, the for names only the item.
        let only_items = FOR_EACH.replace(" * i\n", "\n");
        assert_eq!(recovered(&only_items, &["c", "i"]), "    for arrayElement in Items {\n        Total = Total + arrayElement\n    }\n");
    }

    #[test]
    fn for_each_whose_counters_are_used_elsewhere_stays_a_while() {
        // The counters are not the body's own.
        assert_eq!(recovered(FOR_EACH, &["c"]), FOR_EACH);
        // The index is read after the loop, which a for's index cannot be.
        let after = format!("{FOR_EACH}    Total = i\n");
        assert_eq!(recovered(&after, &["c", "i"]), after);
        // A continue would skip the step in the while, and not in a for.
        let skips = FOR_EACH.replace("        Total", "        if Total > 3 {\n            continue\n        }\n        Total");
        assert_eq!(recovered(&skips, &["c", "i"]), skips);
    }

    #[test]
    fn for_loop_is_a_count() {
        let body = "    n = first\n    while n <= last {\n        Total = Total + n\n        n = n + 1\n    }\n";
        assert_eq!(recovered(body, &["n"]), "    for n in first..last {\n        Total = Total + n\n    }\n");
        // A body that sets the count is not a count's.
        let sets = body.replace("Total = Total + n", "n = n + 2");
        assert_eq!(recovered(&sets, &["n"]), sets);
    }
}
