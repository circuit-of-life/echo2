//! What echo's `#[tree]` macro parses in a project, as JSON: the `echo_export` command writes
//! it (see README.md), and [`schema`] has Rust types to read it back into, with serde.
//!
//! To use only the types: `echo_export = { path = "..", default-features = false }`.

pub mod schema;
