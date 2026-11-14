use std::{marker::PhantomData, rc::Rc};

use crate::{
    backend::{Element, NodeId},
    builder::Builder,
    ctx::{Ctx, CtxRef},
};

/// A tree of nodes under `root`, kept in sync with a view by [`run`](Self::run).
///
/// The view is passed to each `run`, so it can borrow whatever it needs for that run, from as many
/// places as it likes:
///
/// ```ignore
/// app.run(false, |b| ui(b, &mut self.state, &mut *game.bind_mut(), &mut |_, _| {}));
/// ```
///
/// `App` is a cheap handle: clones share the same tree. That lets the app live inside the state it
/// runs with (`let app = self.app.clone().unwrap(); app.run(false, |b| ui(b, self, ..))`).
///
/// When an event an `ON(..)` waits for fires, the backend's
/// [`request_rerun`](crate::Backend::request_rerun) is called. Respond by calling `run(true, ..)` with
/// the same view, so the `ON` handler gets to see the event.
pub struct App<R: Element> {
    ctx: CtxRef,
    _root: PhantomData<fn() -> R>,
}

impl<R: Element> Clone for App<R> {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            _root: PhantomData,
        }
    }
}

impl<R: Element> App<R> {
    /// An app that builds its views under `root`, a node of `backend`'s.
    pub fn new(backend: Rc<R::Backend>, root: NodeId) -> Self {
        Self {
            ctx: Ctx::new(backend, root),
            _root: PhantomData,
        }
    }

    /// Runs `view`, creating, updating, moving and freeing nodes to match it, and returns what
    /// it returned.
    ///
    /// `force_memo` re-runs every `MEMO` even if its value didn't change. Calling `run` again
    /// while it's already running (e.g. from a signal echo caused) does nothing and returns `None`.
    pub fn run<T>(&self, force_memo: bool, view: impl FnOnce(Builder<R>) -> T) -> Option<T> {
        if !self.begin(force_memo) {
            return None;
        }
        let finish = Finish(&self.ctx);
        let out = view(Builder::new(self.ctx.clone()));
        drop(finish);
        Some(out)
    }

    fn begin(&self, force_memo: bool) -> bool {
        match self.ctx.try_borrow_mut() {
            Ok(mut ctx) if !ctx.running() => {
                ctx.begin_run(force_memo);
                true
            }
            Ok(ctx) => {
                ctx.warn("echo: `App::run` called while the app is already running; ignored");
                false
            }
            Err(_) => false,
        }
    }
}

/// Ends a run, also when the view panics.
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
