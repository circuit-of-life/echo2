#![deny(unused_must_use, unnameable_types)]
#![allow(clippy::too_many_arguments, clippy::single_match)]

extern crate self as echo;

mod app;
mod backend;
mod builder;
mod ctx;
pub mod memory;

pub use app::EchoApp;
pub use backend::{AnyNode, Backend, ConnId, Create, Element, Is, NodeId};
pub use builder::{
    __MemoScope, __Scope, Body, Builder, PathElem, StateCell as __StateCell, StateMut,
};

pub use echo_macro::tree;
