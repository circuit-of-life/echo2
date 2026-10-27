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

type ViewFn<R, S> = dyn FnMut(Builder<R>, &mut S);

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
/// When a node connected with `ON(..)` emits its signal, `root` emits `__echo_rerun(force_memo)`.
/// Respond by calling `run(state, force_memo)` so the `ON` handler gets to see the event.
pub struct App<R: Inherits<Node>, S> {
    inner: Rc<AppInner<R, S>>,
}

struct AppInner<R: Inherits<Node>, S> {
    ctx: CtxRef,
    func: RefCell<Box<ViewFn<R, S>>>,
}

impl<R: Inherits<Node>, S> Clone for App<R, S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<R: Inherits<Node>, S> App<R, S> {
    /// `func` builds the tree, usually by calling a tree function:
    /// `App::new(root, |b, s| ui(b, s, &mut |_, _| {}))`. What it returns is ignored.
    pub fn new<Ret, F: FnMut(Builder<R>, &mut S) -> Ret + 'static>(
        mut root: Gd<R>,
        mut func: F,
    ) -> Self {
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
                func: RefCell::new(Box::new(move |b, s| {
                    func(b, s);
                })),
            }),
        }
    }

    /// Runs the view function, creating, updating, moving and freeing nodes to match it.
    ///
    /// `force_memo` re-runs every `MEMO` even if its value didn't change. Calling `run` again
    /// while it's already running (e.g. from a signal echo caused) does nothing.
    pub fn run(&self, state: &mut S, force_memo: bool) {
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
