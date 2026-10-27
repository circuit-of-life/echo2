//! The retained state behind an [`App`](crate::App).
//!
//! Every element the tree code runs (a node, a `STATE`, a `MEMO`, an `ON`, a branch of an
//! `if`/`match`, an iteration of a `for`) has an [`Entry`], identified by a key derived from its
//! parent scope's key and its position (or user key) inside that scope. Entries form a tree:
//! each scope entry remembers which child entries it touched the last time it ran.
//!
//! While a scope runs, the keys it touches are collected in its [`Frame`]. When it exits, that
//! list replaces the old one and any old child that wasn't touched is destroyed, along with its
//! subtree. A skipped `MEMO` touches nothing inside it, so its subtree is kept as is, at O(1) cost
//! instead of re-marking every entry inside it.
//!
//! Child order is kept without asking Godot where each child is: a node is known to still be at
//! the index it was placed at last run as long as, in its parent, nothing was created or moved
//! earlier in this run, and nothing changed it from outside (watched through the parent's
//! `child_order_changed`).

use std::{
    any::Any,
    cell::{Cell, RefCell},
    hash::Hash,
    mem,
    rc::{Rc, Weak},
};

use ahash::AHashMap;
use godot::{classes::Node, global::godot_warn, prelude::*};
use nohash_hasher::IntMap;
use smallvec::SmallVec;

use crate::builder::PathElem;

pub(crate) type CtxRef = Rc<RefCell<Ctx>>;

const HASHER: ahash::RandomState = ahash::RandomState::with_seeds(1, 2, 3, 4);

pub(crate) fn hash_one<T: Hash>(v: T) -> u64 {
    HASHER.hash_one(v)
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum Tag {
    Pos,
    Keyed,
    Dup,
    Signal,
}

fn derive_key(parent: u64, tag: Tag, v: u64) -> u64 {
    hash_one((parent, tag as u8, v))
}

const ROOT_KEY: u64 = 0xEC40_0000_0000_0001;

struct Entry {
    /// Number of the last run that touched this entry.
    seen: u64,
    /// Entries touched by this scope the last time it ran (empty for leaves).
    children: Vec<u64>,
    kind: Kind,
}

enum Kind {
    /// A branch of an `if`/`match` or an iteration of a `for`.
    Scope,
    Node {
        node: Gd<Node>,
        conns: SmallVec<[Conn; 1]>,
        /// Index this node was placed at in its parent, last run.
        last_idx: i32,
        /// Set by `child_order_changed`; created once the node gets children.
        order_changed: Option<Rc<Cell<bool>>>,
    },
    State(Box<dyn Any>),
    Memo {
        value: Box<dyn Any>,
        /// Godot children of the enclosing node that this memo placed, in order.
        slots: Vec<u64>,
        /// Index of the first slot, last run.
        start: i32,
    },
    /// An `ON(..)`, holding a reference on its node's connection for `name`.
    Signal {
        node_key: u64,
        name: &'static str,
    },
}

/// One connection per (node, signal), shared by every `ON` for that signal.
struct Conn {
    name: &'static str,
    callable: Callable,
    refs: u32,
}

struct Frame {
    key: u64,
    elem: Option<PathElem>,
    /// Index of the innermost node frame on the stack (this one, for node frames).
    node_frame: usize,
    next_push: u64,
    /// Entries touched by this scope during this run.
    children: Vec<u64>,
    kind: FrameKind,
}

enum FrameKind {
    Node {
        next_idx: i32,
        new: bool,
        /// Nothing was created or moved in this node during this run, and its children didn't
        /// change since the last one: children not placed yet are where they were.
        clean: bool,
    },
    Scope,
    Memo {
        placed: Vec<u64>,
    },
}

struct Pending {
    node_key: u64,
    name: &'static str,
    args: SmallVec<[Variant; 4]>,
    consumed: bool,
}

pub(crate) struct Ctx {
    root: Gd<Node>,
    this: Weak<RefCell<Ctx>>,
    entries: IntMap<u64, Entry>,
    stack: Vec<Frame>,
    run: u64,
    force_memo: bool,
    running: bool,
    pending: Vec<Pending>,
    pool: Vec<Vec<u64>>,
    snames: AHashMap<(usize, usize), StringName>,
    warned_duplicate_key: bool,
}

impl Ctx {
    pub(crate) fn new(root: Gd<Node>) -> CtxRef {
        Rc::new_cyclic(|this| {
            RefCell::new(Ctx {
                root,
                this: this.clone(),
                entries: IntMap::default(),
                stack: Vec::new(),
                run: 0,
                force_memo: false,
                running: false,
                pending: Vec::new(),
                pool: Vec::new(),
                snames: AHashMap::default(),
                warned_duplicate_key: false,
            })
        })
    }

    fn take_vec(&mut self) -> Vec<u64> {
        self.pool.pop().unwrap_or_default()
    }
    fn recycle(&mut self, mut v: Vec<u64>) {
        if v.capacity() > 0 {
            v.clear();
            self.pool.push(v);
        }
    }

    fn top(&self) -> &Frame {
        self.stack
            .last()
            .expect("echo: builder used outside of a run")
    }
    fn top_mut(&mut self) -> &mut Frame {
        self.stack
            .last_mut()
            .expect("echo: builder used outside of a run")
    }
    fn node_of(&self, key: u64) -> &Gd<Node> {
        match &self.entries[&key].kind {
            Kind::Node { node, .. } => node,
            _ => unreachable!(),
        }
    }
    fn current_node_key(&self) -> u64 {
        self.stack[self.top().node_frame].key
    }

    pub(crate) fn running(&self) -> bool {
        self.running
    }
    pub(crate) fn force_memo(&self) -> bool {
        self.force_memo
    }
    pub(crate) fn current_node(&self) -> &Gd<Node> {
        self.node_of(self.current_node_key())
    }
    pub(crate) fn current_is_new(&self) -> bool {
        match &self.stack[self.top().node_frame].kind {
            FrameKind::Node { new, .. } => *new,
            _ => unreachable!(),
        }
    }
    pub(crate) fn path(&self) -> Vec<PathElem> {
        self.stack.iter().filter_map(|f| f.elem).collect()
    }
    /// The current node, and `name` as a (cached) `StringName`.
    pub(crate) fn node_and_name(&mut self, name: &'static str) -> (&mut Gd<Node>, &StringName) {
        let key = self.current_node_key();
        let Ctx {
            entries, snames, ..
        } = self;
        let Some(Entry {
            kind: Kind::Node { node, .. },
            ..
        }) = entries.get_mut(&key)
        else {
            unreachable!()
        };
        let sname = snames
            .entry((name.as_ptr() as usize, name.len()))
            .or_insert_with(|| StringName::from(name));
        (node, sname)
    }

    // ---- run lifecycle ------------------------------------------------------------------------

    pub(crate) fn begin_run(&mut self, force_memo: bool) {
        self.run += 1;
        self.force_memo = force_memo;
        self.running = true;
        self.unwind_to(0);

        let new = !self.entries.contains_key(&ROOT_KEY);
        if new {
            self.entries.insert(
                ROOT_KEY,
                Entry {
                    seen: 0,
                    children: Vec::new(),
                    kind: Kind::Node {
                        node: self.root.clone(),
                        conns: SmallVec::new(),
                        last_idx: -1,
                        order_changed: None,
                    },
                },
            );
        }
        self.entries.get_mut(&ROOT_KEY).unwrap().seen = self.run;
        let clean = self.take_clean(ROOT_KEY);
        let children = self.take_vec();
        self.stack.push(Frame {
            key: ROOT_KEY,
            elem: None,
            node_frame: 0,
            next_push: 0,
            children,
            kind: FrameKind::Node {
                next_idx: 0,
                new,
                clean,
            },
        });
    }

    pub(crate) fn end_run(&mut self) {
        self.exit_to(0);
        self.finish_run();
    }

    /// Cleanup after a panic inside the tree code: forget the half-built frames. Entries keep
    /// their previous children, so the next run picks up from a consistent state.
    pub(crate) fn abort_run(&mut self) {
        self.unwind_to(0);
        self.finish_run();
    }

    fn finish_run(&mut self) {
        self.running = false;
        // Events whose `ON` wasn't reached stay pending (it may sit in a skipped memo) until a
        // forced run, which reaches every `ON` there is.
        let force = self.force_memo;
        self.pending.retain(|p| !p.consumed && !force);
    }

    // ---- scopes -------------------------------------------------------------------------------

    fn next_pos(&mut self) -> (u64, PathElem) {
        let top = self.top_mut();
        let n = top.next_push;
        top.next_push += 1;
        (derive_key(top.key, Tag::Pos, n), PathElem::Inc(n))
    }

    /// Records that the current scope used `key` (which must exist) during this run.
    fn mark(&mut self, key: u64) {
        let run = self.run;
        let entry = self.entries.get_mut(&key).unwrap();
        if entry.seen != run {
            entry.seen = run;
            self.top_mut().children.push(key);
        }
    }

    fn insert(&mut self, key: u64, kind: Kind) {
        if self.entries.contains_key(&key) {
            self.destroy(key);
        }
        self.entries.insert(
            key,
            Entry {
                seen: 0,
                children: Vec::new(),
                kind,
            },
        );
    }

    fn push_frame(&mut self, key: u64, elem: PathElem, kind: FrameKind) -> usize {
        let depth = self.stack.len();
        let node_frame = match kind {
            FrameKind::Node { .. } => depth,
            _ => self.top().node_frame,
        };
        let children = self.take_vec();
        self.stack.push(Frame {
            key,
            elem: Some(elem),
            node_frame,
            next_push: 0,
            children,
            kind,
        });
        depth
    }

    /// Pops frames down to `depth`, removing whatever the popped scopes stopped using.
    pub(crate) fn exit_to(&mut self, depth: usize) {
        while self.stack.len() > depth {
            let frame = self.stack.pop().unwrap();
            let Some(entry) = self.entries.get_mut(&frame.key) else {
                self.recycle(frame.children);
                continue;
            };
            let old = mem::replace(&mut entry.children, frame.children);
            let changed = old != entry.children;
            if let FrameKind::Memo { placed } = frame.kind {
                let old_slots = match &mut entry.kind {
                    Kind::Memo { slots, .. } => mem::replace(slots, placed),
                    _ => placed,
                };
                self.recycle(old_slots);
            }
            if changed {
                for &k in &old {
                    if self.entries.get(&k).is_some_and(|e| e.seen != self.run) {
                        self.destroy(k);
                    }
                }
            }
            self.recycle(old);
        }
    }

    /// Pops frames without cleaning anything up (used while unwinding from a panic).
    pub(crate) fn unwind_to(&mut self, depth: usize) {
        while self.stack.len() > depth {
            let frame = self.stack.pop().unwrap();
            self.recycle(frame.children);
            if let FrameKind::Memo { placed } = frame.kind {
                self.recycle(placed);
            }
        }
    }

    /// Removes an entry and everything under it, freeing nodes and disconnecting signals.
    fn destroy(&mut self, key: u64) {
        // (key, whether an ancestor node is already being freed)
        let mut work: SmallVec<[(u64, bool); 16]> = smallvec::smallvec![(key, false)];
        while let Some((k, in_freed)) = work.pop() {
            let Some(entry) = self.entries.remove(&k) else {
                continue;
            };
            let mut children_in_freed = in_freed;
            match entry.kind {
                Kind::Node { mut node, .. } => {
                    // Descendants go away with their Godot ancestor.
                    if !in_freed && node.is_instance_valid() && !node.is_queued_for_deletion() {
                        node.queue_free();
                    }
                    children_in_freed = true;
                }
                Kind::Signal { node_key, name } if !in_freed => self.release_conn(node_key, name),
                Kind::Memo { slots, .. } => self.recycle(slots),
                _ => {}
            }
            work.extend(entry.children.iter().map(|&c| (c, children_in_freed)));
            self.recycle(entry.children);
        }
    }

    /// Whether `key`'s children are as echo left them last run (and starts watching again).
    fn take_clean(&mut self, key: u64) -> bool {
        match &self.entries[&key].kind {
            Kind::Node {
                order_changed: Some(c),
                ..
            } => !c.replace(false),
            _ => false,
        }
    }

    /// Whether the current node is still clean (see [`FrameKind::Node`]).
    fn is_clean(&self, nf: usize) -> bool {
        let FrameKind::Node { clean, .. } = self.stack[nf].kind else {
            unreachable!()
        };
        clean
            && match &self.entries[&self.stack[nf].key].kind {
                Kind::Node {
                    order_changed: Some(c),
                    ..
                } => !c.get(),
                _ => false,
            }
    }

    /// Records `key` as placed by every memo running inside the current node.
    fn record_slot(&mut self, nf: usize, key: u64) {
        for frame in &mut self.stack[nf + 1..] {
            if let FrameKind::Memo { placed } = &mut frame.kind {
                placed.push(key);
            }
        }
    }

    /// Puts child `key` at the current node's next index.
    fn place(&mut self, key: u64, created: bool) {
        let nf = self.top().node_frame;
        let parent_key = self.stack[nf].key;
        let FrameKind::Node { next_idx, .. } = &mut self.stack[nf].kind else {
            unreachable!()
        };
        let idx = *next_idx;
        *next_idx += 1;
        let clean = self.is_clean(nf);
        let Some(Entry {
            kind: Kind::Node { node, last_idx, .. },
            ..
        }) = self.entries.get_mut(&key)
        else {
            unreachable!()
        };
        let known = clean && !created && *last_idx == idx;
        *last_idx = idx;
        if !known {
            let node = node.clone();
            let current = node.get_index();
            if current != idx || created {
                let mut parent = self.node_of(parent_key).clone();
                if current != idx {
                    if current < 0 {
                        // removed from its parent behind our back
                        parent.add_child(&node);
                    }
                    parent.move_child(&node, idx);
                }
                if let FrameKind::Node { clean, .. } = &mut self.stack[nf].kind {
                    *clean = false;
                }
            }
            self.watch_order(parent_key);
        }
        self.record_slot(nf, key);
    }

    /// Makes sure outside changes to `key`'s children get noticed.
    fn watch_order(&mut self, key: u64) {
        let Some(Entry {
            kind:
                Kind::Node {
                    node,
                    order_changed: order_changed @ None,
                    ..
                },
            ..
        }) = self.entries.get_mut(&key)
        else {
            return;
        };
        let cell = Rc::new(Cell::new(false));
        let flag = cell.clone();
        node.connect(
            "child_order_changed",
            &Callable::from_fn("echo child order", move |_| flag.set(true)),
        );
        *order_changed = Some(cell);
    }

    pub(crate) fn enter_node(&mut self, create: fn() -> Gd<Node>) -> usize {
        let (key, elem) = self.next_pos();
        let exists = matches!(
            self.entries.get(&key),
            Some(Entry { kind: Kind::Node { node, .. }, .. }) if node.is_instance_valid()
        );
        if !exists {
            let node = create();
            let mut parent = self.current_node().clone();
            parent.add_child(&node);
            self.insert(
                key,
                Kind::Node {
                    node,
                    conns: SmallVec::new(),
                    last_idx: -1,
                    order_changed: None,
                },
            );
        }
        self.mark(key);
        self.place(key, !exists);
        let clean = self.take_clean(key);
        self.push_frame(
            key,
            elem,
            FrameKind::Node {
                next_idx: 0,
                new: !exists,
                clean,
            },
        )
    }

    /// A scope taking the next position, for a call or a body: whatever runs in it (however
    /// much, if it returns early) can't shift the positions after it.
    pub(crate) fn enter_scope(&mut self) -> usize {
        let (key, elem) = self.next_pos();
        if !matches!(
            self.entries.get(&key),
            Some(Entry {
                kind: Kind::Scope,
                ..
            })
        ) {
            self.insert(key, Kind::Scope);
        }
        self.mark(key);
        self.push_frame(key, elem, FrameKind::Scope)
    }

    /// Reserves a position for an `if`/`match`/`for`, whose branches/iterations are then
    /// entered with [`Self::enter_keyed`].
    pub(crate) fn slot(&mut self) -> u64 {
        self.next_pos().0
    }

    pub(crate) fn enter_keyed(&mut self, slot: u64, key_hash: u64) -> usize {
        let mut key = derive_key(slot, Tag::Keyed, key_hash);
        let mut dup = 0u64;
        while self.entries.get(&key).is_some_and(|e| e.seen == self.run) {
            // Same key twice in one loop: keep both, told apart by occurrence.
            if !self.warned_duplicate_key {
                self.warned_duplicate_key = true;
                godot_warn!(
                    "echo: duplicate KEY in a `for` loop at {:?}; items with equal keys are told apart by order, so they may be recreated when the list changes",
                    self.path()
                );
            }
            dup += 1;
            key = derive_key(slot, Tag::Dup, hash_one((key_hash, dup)));
        }
        if !matches!(
            self.entries.get(&key),
            Some(Entry {
                kind: Kind::Scope,
                ..
            })
        ) {
            self.insert(key, Kind::Scope);
        }
        self.mark(key);
        self.push_frame(key, PathElem::Hash(key_hash), FrameKind::Scope)
    }

    // ---- memo ---------------------------------------------------------------------------------

    /// Reserves the memo's position. Returns its key and previous value, if any.
    pub(crate) fn memo_key(&mut self) -> (u64, Option<&dyn Any>) {
        let (key, _) = self.next_pos();
        let old = match self.entries.get(&key) {
            Some(Entry {
                kind: Kind::Memo { value, .. },
                ..
            }) => Some(&**value),
            _ => None,
        };
        (key, old)
    }

    fn next_idx(&self) -> i32 {
        match self.stack[self.top().node_frame].kind {
            FrameKind::Node { next_idx, .. } => next_idx,
            _ => unreachable!(),
        }
    }

    pub(crate) fn enter_memo(&mut self, key: u64, value: Box<dyn Any>) -> usize {
        let idx = self.next_idx();
        match self.entries.get_mut(&key) {
            Some(Entry {
                kind: Kind::Memo {
                    value: old, start, ..
                },
                ..
            }) => {
                *old = value;
                *start = idx;
            }
            _ => self.insert(
                key,
                Kind::Memo {
                    value,
                    slots: Vec::new(),
                    start: idx,
                },
            ),
        }
        self.mark(key);
        let n = self.top().next_push - 1;
        let placed = self.take_vec();
        self.push_frame(key, PathElem::Inc(n), FrameKind::Memo { placed })
    }

    /// Keeps a memo's subtree as it is, re-placing the nodes it put in the current node.
    pub(crate) fn skip_memo(&mut self, key: u64) {
        self.mark(key);
        let nf = self.top().node_frame;
        let idx = self.next_idx();
        let (slots, unmoved) = match &mut self.entries.get_mut(&key).unwrap().kind {
            Kind::Memo { slots, start, .. } => {
                let unmoved = *start == idx;
                *start = idx;
                (mem::take(slots), unmoved)
            }
            _ => unreachable!(),
        };
        if unmoved && self.is_clean(nf) {
            // everything before it is as it was, so its nodes are too
            if let FrameKind::Node { next_idx, .. } = &mut self.stack[nf].kind {
                *next_idx += slots.len() as i32;
            }
            for &k in &slots {
                self.record_slot(nf, k);
            }
        } else {
            for &k in &slots {
                let alive = matches!(
                    self.entries.get(&k),
                    Some(Entry { kind: Kind::Node { node, .. }, .. }) if node.is_instance_valid()
                );
                if alive {
                    self.place(k, false);
                }
            }
        }
        if let Kind::Memo { slots: s, .. } = &mut self.entries.get_mut(&key).unwrap().kind {
            *s = slots;
        }
    }

    // ---- state --------------------------------------------------------------------------------

    pub(crate) fn state_key(&mut self) -> (u64, Option<&dyn Any>) {
        let (key, _) = self.next_pos();
        if !matches!(
            self.entries.get(&key),
            Some(Entry {
                kind: Kind::State(_),
                ..
            })
        ) {
            return (key, None);
        }
        self.mark(key);
        match &self.entries[&key].kind {
            Kind::State(v) => (key, Some(&**v)),
            _ => unreachable!(),
        }
    }

    pub(crate) fn set_state(&mut self, key: u64, value: Box<dyn Any>) {
        self.insert(key, Kind::State(value));
        self.mark(key);
    }

    // ---- signals ------------------------------------------------------------------------------

    /// Registers an `ON(name)` in the current scope for the current node. Returns the
    /// arguments of a pending emission of that signal, if there is one.
    pub(crate) fn signal(
        &mut self,
        name: &'static str,
        location: &'static std::panic::Location<'static>,
    ) -> Option<SmallVec<[Variant; 4]>> {
        let top = self.top();
        let node_key = self.stack[top.node_frame].key;
        let key = derive_key(top.key, Tag::Signal, hash_one(name));
        if !matches!(
            self.entries.get(&key),
            Some(Entry {
                kind: Kind::Signal { .. },
                ..
            })
        ) {
            self.insert(key, Kind::Signal { node_key, name });
            self.acquire_conn(node_key, name, location);
        }
        self.mark(key);

        if self.pending.is_empty() {
            return None;
        }
        let p = self
            .pending
            .iter_mut()
            .find(|p| p.node_key == node_key && p.name == name)?;
        p.consumed = true;
        Some(p.args.clone())
    }

    fn acquire_conn(
        &mut self,
        node_key: u64,
        name: &'static str,
        location: &'static std::panic::Location<'static>,
    ) {
        let this = self.this.clone();
        let Some(Entry {
            kind: Kind::Node { node, conns, .. },
            ..
        }) = self.entries.get_mut(&node_key)
        else {
            return;
        };
        if let Some(conn) = conns.iter_mut().find(|c| c.name == name) {
            conn.refs += 1;
            return;
        }
        let callable = Callable::from_fn(format!("echo ON({name}) at {location}"), move |args| {
            on_signal(&this, node_key, name, args)
        });
        node.connect(name, &callable);
        conns.push(Conn {
            name,
            callable,
            refs: 1,
        });
    }

    fn release_conn(&mut self, node_key: u64, name: &'static str) {
        let Some(Entry {
            kind: Kind::Node { node, conns, .. },
            ..
        }) = self.entries.get_mut(&node_key)
        else {
            return;
        };
        let Some(i) = conns.iter().position(|c| c.name == name) else {
            return;
        };
        conns[i].refs -= 1;
        if conns[i].refs == 0 {
            let conn = conns.swap_remove(i);
            if node.is_instance_valid() && node.is_connected(name, &conn.callable) {
                node.disconnect(name, &conn.callable);
            }
        }
    }
}

fn on_signal(this: &Weak<RefCell<Ctx>>, node_key: u64, name: &'static str, args: &[&Variant]) {
    let Some(ctx) = this.upgrade() else {
        return;
    };
    let mut root = {
        // Already borrowed or running means this was emitted by something echo itself did
        // during a run (e.g. `UPDATE(value = ..)` makes a Range emit `value_changed`). That's
        // not input, and re-entering the app from here would blow up, so it's dropped.
        let Ok(mut ctx) = ctx.try_borrow_mut() else {
            return;
        };
        if ctx.running {
            return;
        }
        let args: SmallVec<[Variant; 4]> = args.iter().map(|v| (*v).clone()).collect();
        match ctx
            .pending
            .iter_mut()
            .find(|p| p.node_key == node_key && p.name == name)
        {
            Some(p) => {
                p.args = args;
                p.consumed = false;
            }
            None => ctx.pending.push(Pending {
                node_key,
                name,
                args,
                consumed: false,
            }),
        }
        ctx.root.clone()
    };
    if root.is_instance_valid() {
        root.emit_signal("__echo_rerun", &[true.to_variant()]);
    }
}
