#![allow(dead_code)]

use std::rc::Rc;

use echo::{
    App, Builder,
    memory::{Handle, Memory, Node},
};

type View<S> = Box<dyn FnMut(Builder<Node>, &mut S)>;

/// An `App` on a fresh in-memory tree. Reruns that events ask for aren't done right away, so
/// tests control exactly when runs happen (`pump`).
pub struct Harness<S> {
    pub memory: Rc<Memory>,
    pub root: Handle<Node>,
    pub app: App<Node>,
    pub state: S,
    view: View<S>,
}

impl<S> Harness<S> {
    pub fn new<R>(state: S, mut view: impl FnMut(Builder<Node>, &mut S) -> R + 'static) -> Self {
        let memory = Memory::new();
        let root = memory.root();
        let app = App::new(memory.clone(), root.id());
        Self {
            memory,
            root,
            app,
            state,
            view: Box::new(move |b, s| {
                view(b, s);
            }),
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
    /// Performs the rerun an event asked for, if any. Returns whether one was requested.
    pub fn pump(&mut self) -> bool {
        let requested = self.memory.take_rerun();
        if requested {
            self.run_with(true);
        }
        requested
    }
}

pub fn kids(n: &Handle<Node>) -> Vec<Handle<Node>> {
    n.children()
}

pub fn kid(n: &Handle<Node>, i: usize) -> Handle<Node> {
    n.child(i)
}

/// `Kind` or `Kind:text` for each child.
pub fn descs(n: &Handle<Node>) -> Vec<String> {
    n.describe()
}

pub fn ids(n: &Handle<Node>) -> Vec<u64> {
    kids(n).iter().map(|c| c.id().0).collect()
}

pub fn id(n: &Handle<Node>) -> u64 {
    n.id().0
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
