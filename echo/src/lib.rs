#![deny(unused_must_use, unnameable_types)]
#![allow(clippy::too_many_arguments, clippy::single_match)]

mod app;
mod builder;
mod ctx;

pub use app::App;
pub use builder::{
    __MemoScope, __Scope, Body, Builder, PathElem, StateCell as __StateCell, StateMut,
};

pub use echo_macro::tree;
