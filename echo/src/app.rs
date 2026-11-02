use std::{cell::RefCell, rc::Rc};

use godot::{
    classes::Node,
    global::godot_warn,
    obj::{Gd, Inherits},
    prelude::*,
};

use crate::{
    builder::Builder,
    ctx::{Ctx, CtxRef},
};

/// How an [`App`] takes its state `S`, in [`run`](App::run) and in its view function.
///
/// [`One`] (the default) takes it as `&mut S`. [`Split`] takes a tuple state `(A, B, ..)` as
/// separate borrows, `(&mut A, &mut B, ..)`, so its parts can live in different places.
pub trait StateRefs<S> {
    /// What `run` and the view function get.
    type Refs<'a>;
}

/// The state as one `&mut S` (the default).
pub struct One;

impl<S: 'static> StateRefs<S> for One {
    type Refs<'a> = &'a mut S;
}

/// A tuple state `(A, B, ..)` as separate borrows `(&mut A, &mut B, ..)`:
/// `App<Control, (Game, Settings), Split>` runs with `app.run((&mut game, &mut settings), ..)`.
pub struct Split;

macro_rules! split {
    ($($t:ident),+) => {
        impl<$($t: 'static),+> StateRefs<($($t,)+)> for Split {
            type Refs<'a> = ($(&'a mut $t,)+);
        }
    };
}
split!(A);
split!(A, B);
split!(A, B, C);
split!(A, B, C, D);
split!(A, B, C, D, E);
split!(A, B, C, D, E, F);
split!(A, B, C, D, E, F, G);
split!(A, B, C, D, E, F, G, H);

type ViewFn<R, S, M> = dyn for<'a> FnMut(Builder<R>, <M as StateRefs<S>>::Refs<'a>);

/// A tree of nodes under `root`, kept in sync with a view function by [`run`](Self::run).
///
/// `App` is a cheap handle: clones share the same tree, which lets the app live inside the state
/// it's run with:
///
/// ```ignore
/// let app = self.app.clone().unwrap();
/// app.run(self, false);
/// ```
///
/// For state in several places, make it a tuple and [`Split`] it: an
/// `App<Control, (Game, Settings), Split>` runs with `app.run((&mut game, &mut settings), ..)`.
///
/// When a node connected with `ON(..)` emits its signal, `root` emits `__echo_rerun(force_memo)`.
/// Respond by calling `run(state, force_memo)` so the `ON` handler gets to see the event.
pub struct App<R: Inherits<Node>, S, M: StateRefs<S> = One> {
    inner: Rc<AppInner<R, S, M>>,
}

struct AppInner<R: Inherits<Node>, S, M: StateRefs<S>> {
    ctx: CtxRef,
    func: RefCell<Box<ViewFn<R, S, M>>>,
}

impl<R: Inherits<Node>, S, M: StateRefs<S>> Clone for App<R, S, M> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<R: Inherits<Node>, S: 'static> App<R, S> {
    /// `func` builds the tree, usually by calling a tree function:
    /// `App::new(root, |b, s| ui(b, s, &mut |_, _| {}))`. What it returns is ignored.
    pub fn new<Ret, F: FnMut(Builder<R>, &mut S) -> Ret + 'static>(
        root: Gd<R>,
        mut func: F,
    ) -> Self {
        Self::with_view(
            root,
            Box::new(move |b, s| {
                func(b, s);
            }),
        )
    }
}

impl<R: Inherits<Node>, S: 'static> App<R, S, Split>
where
    Split: StateRefs<S>,
{
    /// Like [`new`](App::new), for a tuple state taken as separate borrows:
    /// `App::new_split(root, |b, (game, settings)| ui(b, game, settings, &mut |_, _| {}))`.
    pub fn new_split<Ret, F>(root: Gd<R>, mut func: F) -> Self
    where
        F: for<'a> FnMut(Builder<R>, <Split as StateRefs<S>>::Refs<'a>) -> Ret + 'static,
    {
        Self::with_view(
            root,
            Box::new(move |b, s| {
                func(b, s);
            }),
        )
    }
}

impl<R: Inherits<Node>, S, M: StateRefs<S>> App<R, S, M> {
    fn with_view(mut root: Gd<R>, func: Box<ViewFn<R, S, M>>) -> Self {
        root.upcast_mut()
            .add_user_signal_ex("__echo_rerun")
            .arguments(
                &varray![&vdict! { "name" => "force_memo", "type" => VariantType::BOOL }]
                    .upcast_any_array(),
            )
            .done();
        Self {
            inner: Rc::new(AppInner {
                ctx: Ctx::new(root.upcast()),
                func: RefCell::new(func),
            }),
        }
    }

    /// Runs the view function, creating, updating, moving and freeing nodes to match it.
    ///
    /// `state` is `&mut S`, or with [`Split`], a tuple of borrows of the parts of `S`.
    /// `force_memo` re-runs every `MEMO` even if its value didn't change. Calling `run` again
    /// while it's already running (e.g. from a signal echo caused) does nothing.
    pub fn run(&self, state: M::Refs<'_>, force_memo: bool) {
        let ctx = &self.inner.ctx;
        match ctx.try_borrow_mut() {
            Ok(mut ctx) if !ctx.running() => ctx.begin_run(force_memo),
            _ => {
                godot_warn!("echo: `App::run` called while the app is already running; ignored");
                return;
            }
        }
        struct Finish<'a>(&'a CtxRef);
        impl Drop for Finish<'_> {
            fn drop(&mut self) {
                if let Ok(mut ctx) = self.0.try_borrow_mut() {
                    if std::thread::panicking() {
                        ctx.abort_run();
                    } else {
                        ctx.end_run();
                    }
                }
            }
        }
        let finish = Finish(ctx);
        let mut func = self.inner.func.borrow_mut();
        func(Builder::new(ctx.clone()), state);
        drop(func);
        drop(finish);
    }
}
