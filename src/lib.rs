//! Plenty — a stack-based language built around a small memory footprint.
//!
//! Source flows through three layers, each its own module:
//!
//! ```text
//!   text --lexer::lex--> Tok --op::compile--> Op --Vm::exec--> effects
//! ```
//!
//! [`value`] sits underneath all three: the [`Value`]s that live on the stack,
//! kept to 16 bytes apiece, and the heap that backs the variable-sized ones.

mod codegen;
mod lexer;
mod op;
mod value;
mod vm;

pub use codegen::compile_source_to_executable;
pub use op::{FnSig, Ty};
pub use value::{StrId, Value};
pub use vm::Vm;
