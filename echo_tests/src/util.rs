use std::{cell::Cell, rc::Rc};

use echo::{App, Builder};
use godot::prelude::*;

type View<S> = Box<dyn FnMut(Builder<Node>, &mut S)>;

/// An `App` mounted under a fresh root node, plus a flag that records `__echo_rerun` requests
/// instead of re-running immediately (so tests control exactly when runs happen).
pub struct Harness<S> {
    pub root: Gd<Node>,
    pub app: App<Node>,
    pub state: S,
    view: View<S>,
    pub rerun_requested: Rc<Cell<bool>>,
}

impl<S> Harness<S> {
    pub fn new<R>(
        parent: &Gd<Node>,
        state: S,
        mut view: impl FnMut(Builder<Node>, &mut S) -> R + 'static,
    ) -> Self {
        let mut root = Node::new_alloc();
        parent.clone().add_child(&root);
        let app = App::new(root.clone());
        let rerun_requested = Rc::new(Cell::new(false));
        let flag = rerun_requested.clone();
        root.connect(
            "__echo_rerun",
            &Callable::from_fn("rerun", move |_| flag.set(true)),
        );
        Self {
            root,
            app,
            state,
            view: Box::new(move |b, s| {
                view(b, s);
            }),
            rerun_requested,
        }
    }
    pub fn run(&mut self) {
        self.run_with(false);
    }
    pub fn run_forced(&mut self) {
        self.run_with(true);
    }
    fn run_with(&mut self, force_memo: bool) {
        let (view, state) = (&mut self.view, &mut self.state);
        self.app.run(force_memo, |b| view(b, state));
    }
    /// Performs the rerun a signal asked for, if any. Returns whether one was requested.
    pub fn pump(&mut self) -> bool {
        let requested = self.rerun_requested.replace(false);
        if requested {
            self.run_with(true);
        }
        requested
    }
}

impl<S> Drop for Harness<S> {
    fn drop(&mut self) {
        if self.root.is_instance_valid() {
            self.root.queue_free();
        }
    }
}

/// Children that are not queued for deletion.
pub fn kids(n: &Gd<Node>) -> Vec<Gd<Node>> {
    n.get_children()
        .iter_shared()
        .filter(|c| !c.is_queued_for_deletion())
        .collect()
}

pub fn kid(n: &Gd<Node>, i: usize) -> Gd<Node> {
    kids(n)
        .get(i)
        .cloned()
        .unwrap_or_else(|| panic!("node has no live child #{i}"))
}

/// `Class` or `Class:text` for each live child.
pub fn descs(n: &Gd<Node>) -> Vec<String> {
    kids(n)
        .iter()
        .map(|c| {
            let class = c.get_class().to_string();
            if c.has_method("get_text") {
                format!("{class}:{}", c.get("text"))
            } else {
                class
            }
        })
        .collect()
}

pub fn ids(n: &Gd<Node>) -> Vec<i64> {
    kids(n).iter().map(|c| c.instance_id().to_i64()).collect()
}

pub fn id(n: &Gd<Node>) -> i64 {
    n.instance_id().to_i64()
}

pub fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

macro_rules! check {
    ($c:expr) => {
        if !$c {
            return Err(format!(
                "{}:{}: check failed: {}",
                file!(),
                line!(),
                stringify!($c)
            ));
        }
    };
}
macro_rules! check_eq {
    ($a:expr, $b:expr) => {{
        let (a, b) = (&$a, &$b);
        if a != b {
            return Err(format!(
                "{}:{}: `{}` was {:?}, expected {:?}",
                file!(),
                line!(),
                stringify!($a),
                a,
                b
            ));
        }
    }};
}
pub(crate) use {check, check_eq};
