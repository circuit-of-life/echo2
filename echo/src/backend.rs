//! What echo needs from the tree it keeps in sync: a [`Backend`] for the operations, and
//! [`Element`]s for the node types the tree code names (`Label..{}`, `#[tree(in Control)]`).
//!
//! echo works on [`NodeId`]s and calls the backend through a `dyn` reference, so the runtime is
//! compiled once, whatever the backend. Only the thin typed layer ([`Builder`](crate::Builder),
//! [`App`](crate::App)) is generic over elements.

use std::{any::Any, cell::Cell, rc::Rc};

/// A node in a [`Backend`]'s tree, as the backend identifies it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub u64);

/// A connection made by [`Backend::connect`], as the backend identifies it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ConnId(pub u64);

/// A tree of nodes echo can keep in sync with tree code.
///
/// echo calls these while it holds its own state, so they shouldn't call back into echo
/// (events fired synchronously from them are ignored, as they come from echo's own changes).
pub trait Backend: 'static {
    /// What `ON(event)` gives when the event fired: its arguments.
    type Event: Clone + 'static;

    /// Makes `child` `parent`'s child at `index`, shifting the children from there on. `child`
    /// is either new (not in the tree yet) or already one of `parent`'s children, to be moved.
    fn place(&self, parent: NodeId, child: NodeId, index: usize);

    /// `node`'s index among its parent's children, or `None` if it has no parent.
    fn index(&self, node: NodeId) -> Option<usize>;

    /// Removes `node` and everything under it. (echo only calls this for the topmost node of
    /// a subtree that went away.)
    fn remove(&self, node: NodeId);

    /// Whether `node` still exists. Only matters if something other than echo can remove nodes:
    /// a node echo kept that's gone is created again.
    fn alive(&self, node: NodeId) -> bool {
        let _ = node;
        true
    }

    /// Calls `fire` with the event's arguments whenever `node` fires `event`, until
    /// [`disconnect`](Self::disconnect)ed.
    fn connect(&self, node: NodeId, event: &'static str, fire: Box<dyn Fn(Self::Event)>) -> ConnId;

    fn disconnect(&self, node: NodeId, event: &'static str, conn: ConnId);

    /// An event an `ON(..)` waits for fired outside of a run: the app should run again, with
    /// `force_memo` set, so that `ON(..)` sees it (even inside a skipped `MEMO`).
    fn request_rerun(&self, root: NodeId);

    /// Sets `changed` whenever `parent`'s children are added, moved or removed (by anyone,
    /// echo included), and returns `true`; echo then skips asking for the [`index`](Self::index)
    /// of children that can't have moved. Returning `false` (the default) is always correct,
    /// just slower for nodes with many children.
    ///
    /// If nothing but echo ever changes the tree, returning `true` without ever setting it is
    /// fine too.
    fn watch_children(&self, parent: NodeId, changed: Rc<Cell<bool>>) -> bool {
        let _ = (parent, changed);
        false
    }

    /// Reports something echo thinks is a mistake in the tree code (e.g. a duplicate `KEY`).
    fn warn(&self, message: &str) {
        eprintln!("{message}");
    }
}

/// A type of node: what `Label..{}` creates, what `#[tree(in Control)]` and `Builder<Control>`
/// name.
pub trait Element: 'static {
    type Backend: Backend;
    /// What [`Builder::node`](crate::Builder::node) and `let label = Label..{}` give: a
    /// handle to the node. `INIT(text = ..)` and `UPDATE(text = ..)` call `set_text(..)` on it.
    type Handle;

    fn handle(backend: &Rc<Self::Backend>, node: NodeId) -> Self::Handle;
}

/// An [`Element`] that `T..{}` can create.
pub trait Create: Element {
    /// A new node, not in the tree yet (echo [`place`](Backend::place)s it right after).
    fn create(backend: &Rc<Self::Backend>) -> NodeId;
}

/// `Self` can be used where a `T` is expected: it is a `T`, or a more specific kind of one.
/// For [`Builder::upcast`](crate::Builder::upcast), so for calling an `in T` tree function or
/// placing a `Body<T, ..>` inside a `Self`.
///
/// Every element is itself and an [`AnyNode`]; a backend adds the rest
/// (`impl Is<Control> for Label {}`).
pub trait Is<T: ?Sized> {}

impl<T: Element> Is<T> for T {}
impl<T: Element> Is<AnyNode> for T {}
impl Is<AnyNode> for AnyNode {}

/// Where a tree function without `in T` can be used: in any node. Its builder can create
/// children and keep state, memos and so on, but not reach the node itself.
pub enum AnyNode {}

/// [`Backend`] with type-erased events, for the runtime.
pub(crate) trait DynBackend {
    fn place(&self, parent: NodeId, child: NodeId, index: usize);
    fn index(&self, node: NodeId) -> Option<usize>;
    fn remove(&self, node: NodeId);
    fn alive(&self, node: NodeId) -> bool;
    fn connect(&self, node: NodeId, event: &'static str, fire: Box<dyn Fn(Box<dyn Any>)>)
    -> ConnId;
    fn disconnect(&self, node: NodeId, event: &'static str, conn: ConnId);
    fn request_rerun(&self, root: NodeId);
    fn watch_children(&self, parent: NodeId, changed: Rc<Cell<bool>>) -> bool;
    fn warn(&self, message: &str);
}

impl<B: Backend> DynBackend for B {
    fn place(&self, parent: NodeId, child: NodeId, index: usize) {
        Backend::place(self, parent, child, index)
    }
    fn index(&self, node: NodeId) -> Option<usize> {
        Backend::index(self, node)
    }
    fn remove(&self, node: NodeId) {
        Backend::remove(self, node)
    }
    fn alive(&self, node: NodeId) -> bool {
        Backend::alive(self, node)
    }
    fn connect(
        &self,
        node: NodeId,
        event: &'static str,
        fire: Box<dyn Fn(Box<dyn Any>)>,
    ) -> ConnId {
        Backend::connect(self, node, event, Box::new(move |e| fire(Box::new(e))))
    }
    fn disconnect(&self, node: NodeId, event: &'static str, conn: ConnId) {
        Backend::disconnect(self, node, event, conn)
    }
    fn request_rerun(&self, root: NodeId) {
        Backend::request_rerun(self, root)
    }
    fn watch_children(&self, parent: NodeId, changed: Rc<Cell<bool>>) -> bool {
        Backend::watch_children(self, parent, changed)
    }
    fn warn(&self, message: &str) {
        Backend::warn(self, message)
    }
}
