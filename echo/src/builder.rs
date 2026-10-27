use std::{
    cell::{Cell, UnsafeCell},
    fmt,
    hash::Hash,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    panic::Location,
    rc::Rc,
};

use godot::{
    classes::{Control, Font, Node, StyleBox, Texture2D},
    meta::AsArg,
    obj::{Gd, Inherits, NewAlloc},
    prelude::*,
};

use smallvec::SmallVec;

use crate::ctx::{CtxRef, hash_one};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathElem {
    Inc(u64),
    Hash(u64),
}

/// A tree function's body parameter: what the caller writes after the call, given `A` (a type,
/// a tuple for several arguments, or `()` for none) and placed inside an `N` node:
///
/// ```ignore
/// #[tree]
/// fn card(title: &str, body: Body<VBoxContainer, i32>) {
///     VBoxContainer..{
///         Label..{ UPDATE(text = title) };
///         body(42);
///     };
/// }
/// // card("Hi")..|n| { .. };
/// ```
///
/// In a tree function's or tree closure's signature, the macro writes it out as
/// `&mut dyn FnMut(Builder<N>, A)`, so references in `A` can be borrows of the function's own
/// locals (`Body<Node, &str>` takes any `&str`, however short-lived).
///
/// Written by hand (calling a tree function from plain Rust), an empty body is `&mut |_, _| {}`.
pub type Body<'a, N = Node, A = ()> = &'a mut dyn FnMut(Builder<N>, A);

#[doc(hidden)]
pub struct StateCell<T> {
    in_use: Cell<bool>,
    value: UnsafeCell<T>,
}

/// The state from `let x = STATE(init);`. It derefs to the value, mutably too (with `let mut`),
/// so it works like a `&mut T`: `*x += 1`, `x.push(..)`, `f(&mut x)`, `format!("{x}")`.
///
/// It has the state to itself until it's dropped (normally at the end of its block), so keeping
/// one around until the next run makes that run's `STATE` panic.
pub struct StateMut<T: 'static> {
    cell: Rc<StateCell<T>>,
}

impl<T> StateMut<T> {
    fn new(cell: Rc<StateCell<T>>) -> Self {
        assert!(
            !cell.in_use.replace(true),
            "echo: this STATE is still in use (is a `StateMut` from an earlier run being kept?)"
        );
        Self { cell }
    }
}

impl<T> Drop for StateMut<T> {
    fn drop(&mut self) {
        self.cell.in_use.set(false);
    }
}

impl<T> Deref for StateMut<T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: `in_use` makes this `StateMut` the only one for its cell, and the value is only
        // reachable through it.
        unsafe { &*self.cell.value.get() }
    }
}

impl<T> DerefMut for StateMut<T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in `deref`, and `&mut self` makes this the only borrow.
        unsafe { &mut *self.cell.value.get() }
    }
}

impl<T: fmt::Debug> fmt::Debug for StateMut<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: fmt::Display> fmt::Display for StateMut<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

/// A handle to the scope the tree code is currently building, typed by the node it builds.
///
/// It is cheap to clone, and the type parameter is only a compile-time view: [`cast`](Self::cast)
/// and [`upcast`](Self::upcast) are free, and [`node`](Self::node) does the checked conversion.
pub struct Builder<P> {
    ctx: CtxRef,
    _p: PhantomData<P>,
}

impl<P> Clone for Builder<P> {
    fn clone(&self) -> Self {
        Self::new(self.ctx.clone())
    }
}

/// Closes a scope opened by the tree code (a node, a branch, an iteration, a memo) on drop.
#[doc(hidden)]
#[must_use]
pub struct __Scope {
    ctx: CtxRef,
    depth: usize,
}

impl Drop for __Scope {
    fn drop(&mut self) {
        let Ok(mut ctx) = self.ctx.try_borrow_mut() else {
            return;
        };
        if std::thread::panicking() {
            ctx.unwind_to(self.depth);
        } else {
            ctx.exit_to(self.depth);
        }
    }
}

#[doc(hidden)]
#[must_use]
pub struct __MemoScope(Option<__Scope>);

impl __MemoScope {
    /// Whether the memo's body has to run (its value changed, or memos are being forced).
    #[inline]
    pub fn __run(&self) -> bool {
        self.0.is_some()
    }
}

fn new_node<C: Inherits<Node> + NewAlloc>() -> Gd<Node> {
    C::new_alloc().upcast()
}

// Most methods are deliberately unbounded: a bound here is one more obligation the type checker
// (and rust-analyzer) has to prove at every one of the thousands of generated calls.
impl<P> Builder<P> {
    pub(crate) fn new(ctx: CtxRef) -> Self {
        Self {
            ctx,
            _p: PhantomData,
        }
    }
    fn scope(&self, depth: usize) -> __Scope {
        __Scope {
            ctx: self.ctx.clone(),
            depth,
        }
    }

    #[doc(hidden)]
    pub fn __child<C: Inherits<Node> + NewAlloc>(&self) -> (__Scope, Builder<C>) {
        let depth = self.ctx.borrow_mut().enter_node(new_node::<C>);
        (self.scope(depth), Builder::new(self.ctx.clone()))
    }

    /// A scope around a call or a body, so its contents keep to one position.
    #[doc(hidden)]
    pub fn __scope(&self) -> __Scope {
        let depth = self.ctx.borrow_mut().enter_scope();
        self.scope(depth)
    }

    /// Reserves one position for an `if`/`match`/`for`, whose branches/iterations are entered
    /// with [`Self::__keyed`].
    #[doc(hidden)]
    pub fn __slot(&self) -> u64 {
        self.ctx.borrow_mut().slot()
    }

    #[doc(hidden)]
    pub fn __keyed(&self, slot: u64, key: impl Hash) -> __Scope {
        let depth = self.ctx.borrow_mut().enter_keyed(slot, hash_one(key));
        self.scope(depth)
    }

    #[doc(hidden)]
    pub fn __memo<T: PartialEq + 'static>(&self, value: T) -> __MemoScope {
        let mut ctx = self.ctx.borrow_mut();
        let force = ctx.force_memo();
        let (key, old) = ctx.memo_key();
        let same = !force
            && old
                .and_then(|old| old.downcast_ref::<T>())
                .is_some_and(|old| *old == value);
        if same {
            ctx.skip_memo(key);
            __MemoScope(None)
        } else {
            let depth = ctx.enter_memo(key, Box::new(value));
            drop(ctx);
            __MemoScope(Some(self.scope(depth)))
        }
    }

    #[doc(hidden)]
    pub fn __state<T: 'static>(&self, init: impl FnOnce() -> T) -> Rc<T> {
        let key = {
            let mut ctx = self.ctx.borrow_mut();
            let (key, existing) = ctx.state_key();
            if let Some(v) = existing.and_then(|v| v.downcast_ref::<Rc<T>>()) {
                return v.clone();
            }
            key
        };
        // not borrowed: `init` is user code
        let value = Rc::new(init());
        self.ctx
            .borrow_mut()
            .set_state(key, Box::new(value.clone()));
        value
    }

    /// What `let x = STATE(init);` lowers to.
    #[doc(hidden)]
    pub fn __state_mut<T: 'static>(&self, init: impl FnOnce() -> T) -> StateMut<T> {
        StateMut::new(self.__state(|| StateCell {
            in_use: Cell::new(false),
            value: UnsafeCell::new(init()),
        }))
    }

    #[doc(hidden)]
    pub fn __set_prop<T: ToGodot>(&self, prop: &'static str, value: T) {
        let value = value.to_variant();
        let mut ctx = self.ctx.borrow_mut();
        let (node, name) = ctx.node_and_name(prop);
        node.set(name, &value);
    }

    /// What `ON(signal)` lowers to: registers the signal for the current node and returns the
    /// arguments of its pending emission, if any.
    #[doc(hidden)]
    #[track_caller]
    pub fn __signal(&self, signal: &'static str) -> Option<SmallVec<[Variant; 4]>> {
        self.ctx.borrow_mut().signal(signal, Location::caller())
    }

    /// The position of the current scope (for debugging).
    pub fn path(&self) -> Vec<PathElem> {
        self.ctx.borrow().path()
    }
    /// Whether the node being built was created during this run.
    pub fn init(&self) -> bool {
        self.ctx.borrow().current_is_new()
    }
    /// Whether this run re-runs every `MEMO` regardless of its value.
    pub fn forcing_memos(&self) -> bool {
        self.ctx.borrow().force_memo()
    }
}

impl<P: Inherits<Node>> Builder<P> {
    /// Reinterprets the builder as building a more derived class. The node isn't checked
    /// until [`node`](Self::node) is called.
    #[inline]
    pub fn cast<T: Inherits<Node> + Inherits<P>>(self) -> Builder<T> {
        Builder::new(self.ctx)
    }
    #[inline]
    pub fn upcast<T: Inherits<Node>>(self) -> Builder<T>
    where
        P: Inherits<T>,
    {
        Builder::new(self.ctx)
    }

    /// The node being built.
    pub fn node(&self) -> Gd<P> {
        self.ctx.borrow().current_node().clone().cast()
    }
}

impl<P: Inherits<Control> + Inherits<Node>> Builder<P> {
    fn with_control(&self, prop: &'static str, f: impl FnOnce(&mut Gd<Control>, &StringName)) {
        let mut ctx = self.ctx.borrow_mut();
        let (node, name) = ctx.node_and_name(prop);
        f(&mut node.clone().cast::<Control>(), name);
    }
    #[doc(hidden)]
    pub fn __set_theme_color_override(&self, prop: &'static str, value: Color) {
        self.with_control(prop, |c, name| c.add_theme_color_override(name, value));
    }
    #[doc(hidden)]
    pub fn __set_theme_constant_override(&self, prop: &'static str, value: i32) {
        self.with_control(prop, |c, name| c.add_theme_constant_override(name, value));
    }
    #[doc(hidden)]
    pub fn __set_theme_font_override(&self, prop: &'static str, value: impl AsArg<Gd<Font>>) {
        self.with_control(prop, |c, name| c.add_theme_font_override(name, value));
    }
    #[doc(hidden)]
    pub fn __set_theme_font_size_override(&self, prop: &'static str, value: i32) {
        self.with_control(prop, |c, name| c.add_theme_font_size_override(name, value));
    }
    #[doc(hidden)]
    pub fn __set_theme_icon_override(&self, prop: &'static str, value: impl AsArg<Gd<Texture2D>>) {
        self.with_control(prop, |c, name| c.add_theme_icon_override(name, value));
    }
    #[doc(hidden)]
    pub fn __set_theme_stylebox_override(
        &self,
        prop: &'static str,
        value: impl AsArg<Gd<StyleBox>>,
    ) {
        self.with_control(prop, |c, name| c.add_theme_stylebox_override(name, value));
    }
}
