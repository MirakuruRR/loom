//! Lifting: from cooked Blueprints, structs and enums, as CUE4Parse writes a package to JSON,
//! back to loom source.
//!
//! A cooked Blueprint keeps its classes, properties, defaults and components, and its logic only
//! as the bytecode the Blueprint compiler made of its graphs. The lifter reads the bytecode's
//! statements (`bytecode`), structures its jumps and flow stack into ifs and loops (`flow`),
//! folds the compiler's temporaries back into expressions (`emit`), and prints the result as a
//! source (`print`), which loom then compiles as any other.

/// Refuses with loom cannot express, as `bail!` does: `cannot!("a thing")`, or
/// `cannot!(at: offset, "a thing")`. The other kinds are `not_yet!`, `unexpected!` and `missing!`, and each
/// has a form that makes the error without returning it, `refusal!(Kind, ...)`.
macro_rules! refusal {
    ($kind:ident, at: $at:expr, $($what:tt)+) => {
        anyhow::Error::from($crate::lift::Refused { kind: $crate::lift::Refusal::$kind, at: Some($at), what: format!($($what)+) })
    };
    ($kind:ident, $($what:tt)+) => {
        anyhow::Error::from($crate::lift::Refused { kind: $crate::lift::Refusal::$kind, at: None, what: format!($($what)+) })
    };
}

macro_rules! cannot {
    ($($t:tt)+) => { return Err(refusal!(Inexpressible, $($t)+)) };
}

macro_rules! not_yet {
    ($($t:tt)+) => { return Err(refusal!(NotYet, $($t)+)) };
}

macro_rules! unexpected {
    ($($t:tt)+) => { return Err(refusal!(Unexpected, $($t)+)) };
}

macro_rules! missing {
    ($($t:tt)+) => { return Err(refusal!(Missing, $($t)+)) };
}

mod blueprint;
pub mod bytecode;
pub mod code;
mod emit;
pub mod flow;
mod loops;
pub mod input;
pub mod print;
pub mod program;
#[cfg(test)]
mod tests;

use anyhow::{anyhow, Context, Result};

use crate::syntax::{self, Enum, EnumValue, Field, Source, Span, Struct, TypeName};
use crate::types::TypeDb;
use input::Package;
use program::Program;

/// Why the lifter refused something, which says what would let it through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Loom has no way to write it, so loom itself would have to change.
    Inexpressible,
    /// Loom can write it, but the lifter does not read it yet.
    NotYet,
    /// The input is broken, or is a shape the lifter did not expect a cooked package to hold.
    Unexpected,
    /// The input uses something the project does not have, such as a type from a plugin the
    /// mod's project had and this one does not. The same mod lifts once the project has it.
    Missing,
}

/// A refusal: its kind, the bytecode offset it happened at when there is one, and what was
/// refused. Which asset and function it is in are the context around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub kind: Refusal,
    pub at: Option<usize>,
    pub what: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.kind {
            Refusal::Inexpressible => "loom cannot express",
            Refusal::NotYet => "not lifted yet",
            Refusal::Unexpected => "unexpected input",
            Refusal::Missing => "the project lacks it",
        })?;
        if let Some(at) = self.at {
            write!(f, ", at offset {at}")?;
        }
        write!(f, ": {}", self.what)
    }
}

impl std::error::Error for Refused {}

impl Refused {
    /// The refusal an error ends in, if it is one. Any other error is a fault in the lifter.
    pub fn of(err: &anyhow::Error) -> Option<&Refused> {
        err.chain().find_map(|e| e.downcast_ref::<Refused>())
    }
}

/// One lifted asset: its package path, and its source, or why it could not be lifted.
pub struct Lifted {
    pub path: String,
    pub source: Result<String>,
}

/// Lifts every package: each is read against the others, so they can use each other's
/// Blueprints, structs and enums, and against the project's types.
pub fn lift(db: &TypeDb, packages: Vec<Package>) -> Result<Vec<Lifted>> {
    let mut program = Program::read(db, packages)?;
    let mut out = Vec::new();
    for (path, err) in std::mem::take(&mut program.unread) {
        let err = err.context(format!("reading {path}"));
        out.push(Lifted { path, source: Err(err) });
    }
    for s in &program.structs {
        out.push(Lifted { path: s.path.clone(), source: lift_struct(&program, s).map(|src| print::source(&src)) });
    }
    for e in &program.enums {
        out.push(Lifted { path: e.path.clone(), source: Ok(print::source(&lift_enum(e))) });
    }
    for bp in &program.blueprints {
        let source = if bp.interface {
            blueprint::lift_interface(&program, bp).map(|i| print::source(&Source::Interface(i)))
        } else {
            blueprint::lift(&program, bp).map(|file| print::source(&Source::Blueprint(file)))
        };
        // What is printed must read back, or the printer and the parser disagree.
        let source = source.and_then(|text| {
            syntax::parse_source(&text).map_err(|err| anyhow!("the lifted source does not parse: {err}\n{text}"))?;
            Ok(text)
        });
        out.push(Lifted { path: bp.path.clone(), source: source.with_context(|| format!("lifting {}", bp.path)) });
    }
    Ok(out)
}

fn lift_struct(program: &Program, s: &program::UserStruct) -> Result<Source> {
    let mut fields = Vec::new();
    for (cooked, name, ty) in &s.fields {
        let default = match s.defaults.get(cooked) {
            Some(v) => program.literal(ty, v).with_context(|| format!("field {name}'s default"))?,
            None => None,
        };
        fields.push(Field { name: name.clone(), ty: TypeName { name: program.type_name(ty)?, span: Span::default() }, default, doc: None, span: Span::default() });
    }
    Ok(Source::Struct(Struct { name: s.name.clone(), path: s.path.clone(), fields, doc: None, span: Span::default() }))
}

fn lift_enum(e: &program::UserEnum) -> Source {
    let values = e
        .values
        .iter()
        .map(|(_, name)| EnumValue { name: name.clone(), label: None, doc: None, span: Span::default() })
        .collect();
    Source::Enum(Enum { name: e.name.clone(), path: e.path.clone(), values, doc: None, span: Span::default() })
}
