//! The Loom compiler: parsing source, lowering it to editor operations, and reading an Unreal
//! project's sources and types.

mod builtins;
pub mod docs;
mod editors;
#[cfg(test)]
mod fixtures;
pub mod lift;
pub mod lower;
pub mod ops;
pub mod project;
mod reference;
pub mod syntax;
pub mod types;
