//! An in-memory [`Backend`]: a plain tree of nodes with a few element types, properties and
//! events, for testing tree code (echo's own included) without an engine.
//!
//! ```
//! use echo::{App, memory::{Button, Label, Memory, Node}, tree};
//! # macro_rules! __echo_deny { ($($t:tt)*) => {}; }
//! # pub(crate) use __echo_deny;
//!
//! #[tree]
//! fn counter(clicks: &mut u32) {
//!     Label..{ UPDATE(text = *clicks) };
//!     Button..{
//!         if ON(pressed) {
//!             *clicks += 1;
//!         }
//!     };
//! }
//!
//! # fn main() {
//! let memory = Memory::new();
//! let root = memory.root();
//! let app = App::<Node>::new(memory.clone(), root.id());
//! let mut clicks = 0;
//! app.run(false, |b| counter(b.upcast(), &mut clicks, &mut |_, _| {}));
//! assert_eq!(root.describe(), ["Label:0", "Button:"]);
//!
//! root.child(1).emit_signal("pressed", &[]);
//! assert!(memory.take_rerun());
//! app.run(true, |b| counter(b.upcast(), &mut clicks, &mut |_, _| {}));
//! app.run(false, |b| counter(b.upcast(), &mut clicks, &mut |_, _| {}));
//! assert_eq!(root.describe(), ["Label:1", "Button:"]);
//! # }
//! ```

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fmt,
    marker::PhantomData,
    rc::Rc,
};

use crate::backend::{Backend, ConnId, Create, Element, Is, NodeId};

/// The tree. Make one with [`Memory::new`], which also makes its root node.
pub struct Memory {
    nodes: RefCell<Vec<Option<NodeData>>>,
    root: NodeId,
    next_conn: Cell<u64>,
    rerun: Cell<bool>,
}

type Fire = Rc<dyn Fn(Vec<Value>)>;

struct NodeData {
    kind: &'static str,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    props: BTreeMap<String, Value>,
    conns: Vec<(ConnId, &'static str, Fire)>,
    watchers: Vec<Rc<Cell<bool>>>,
}

impl Memory {
    pub fn new() -> Rc<Self> {
        let memory = Memory {
            nodes: RefCell::new(Vec::new()),
            root: NodeId(0),
            next_conn: Cell::new(0),
            rerun: Cell::new(false),
        };
        memory.create("Node");
        Rc::new(memory)
    }

    /// The root node, which echo never removes.
    pub fn root(self: &Rc<Self>) -> Handle<Node> {
        Handle::new(self.clone(), self.root)
    }

    /// A new node, not in the tree, as something other than echo would make one.
    pub fn new_node<E: Create<Backend = Memory>>(self: &Rc<Self>) -> Handle<E> {
        Handle::new(self.clone(), E::create(self))
    }

    /// Whether a rerun was requested (an event an `ON` waits for fired) since the last call.
    pub fn take_rerun(&self) -> bool {
        self.rerun.replace(false)
    }

    /// The number of nodes in the tree, the root included.
    pub fn len(&self) -> usize {
        self.nodes.borrow().iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn create(&self, kind: &'static str) -> NodeId {
        let mut nodes = self.nodes.borrow_mut();
        let id = NodeId(nodes.len() as u64);
        nodes.push(Some(NodeData {
            kind,
            parent: None,
            children: Vec::new(),
            props: BTreeMap::new(),
            conns: Vec::new(),
            watchers: Vec::new(),
        }));
        id
    }

    fn with<R>(&self, node: NodeId, f: impl FnOnce(&mut NodeData) -> R) -> R {
        let mut nodes = self.nodes.borrow_mut();
        let data = nodes
            .get_mut(node.0 as usize)
            .and_then(Option::as_mut)
            .unwrap_or_else(|| panic!("memory: node {} was removed", node.0));
        f(data)
    }

    fn detach(&self, node: NodeId) {
        let Some(parent) = self.with(node, |n| n.parent.take()) else {
            return;
        };
        self.with(parent, |p| {
            p.children.retain(|&c| c != node);
            p.watchers.iter().for_each(|w| w.set(true));
        });
    }

    /// Fires `event` on `node`, calling what's connected to it (with nothing borrowed).
    fn emit(&self, node: NodeId, event: &str, args: &[Value]) {
        let fires: Vec<_> = self.with(node, |n| {
            n.conns
                .iter()
                .filter(|(_, e, _)| *e == event)
                .map(|(_, _, f)| f.clone())
                .collect()
        });
        for fire in fires {
            fire(args.to_vec());
        }
    }
}

impl Backend for Memory {
    /// The event's arguments.
    type Event = Vec<Value>;

    fn place(&self, parent: NodeId, child: NodeId, index: usize) {
        if self.with(child, |c| c.parent) != Some(parent) {
            self.detach(child);
            self.with(child, |c| c.parent = Some(parent));
        }
        self.with(parent, |p| {
            p.children.retain(|&c| c != child);
            let index = index.min(p.children.len());
            p.children.insert(index, child);
            p.watchers.iter().for_each(|w| w.set(true));
        });
    }

    fn index(&self, node: NodeId) -> Option<usize> {
        let parent = self.with(node, |n| n.parent)?;
        self.with(parent, |p| p.children.iter().position(|&c| c == node))
    }

    fn remove(&self, node: NodeId) {
        self.detach(node);
        let mut nodes = self.nodes.borrow_mut();
        let mut work = vec![node];
        while let Some(n) = work.pop() {
            if let Some(data) = nodes.get_mut(n.0 as usize).and_then(Option::take) {
                work.extend(data.children);
            }
        }
    }

    fn alive(&self, node: NodeId) -> bool {
        matches!(self.nodes.borrow().get(node.0 as usize), Some(Some(_)))
    }

    fn connect(&self, node: NodeId, event: &'static str, fire: Box<dyn Fn(Vec<Value>)>) -> ConnId {
        let id = ConnId(self.next_conn.get());
        self.next_conn.set(id.0 + 1);
        self.with(node, |n| n.conns.push((id, event, fire.into())));
        id
    }

    fn disconnect(&self, node: NodeId, _event: &'static str, conn: ConnId) {
        self.with(node, |n| n.conns.retain(|(id, _, _)| *id != conn));
    }

    fn request_rerun(&self, _root: NodeId) {
        self.rerun.set(true);
    }

    fn watch_children(&self, parent: NodeId, changed: Rc<Cell<bool>>) -> bool {
        self.with(parent, |p| p.watchers.push(changed));
        true
    }
}

/// A property value or an event argument.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl Value {
    /// Converts to `T` (panicking if it's another kind of value).
    pub fn to<T: FromValue>(&self) -> T {
        T::from_value(self)
            .unwrap_or_else(|| panic!("memory: {self:?} isn't a {}", std::any::type_name::<T>()))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => Ok(()),
            Value::Bool(v) => v.fmt(f),
            Value::Int(v) => v.fmt(f),
            Value::Float(v) => v.fmt(f),
            Value::Text(v) => v.fmt(f),
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v.into())
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.into())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}

/// What [`Value::to`] converts to.
pub trait FromValue: Sized {
    fn from_value(v: &Value) -> Option<Self>;
}
impl FromValue for bool {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
}
impl FromValue for i64 {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }
}
impl FromValue for f64 {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }
}
impl FromValue for String {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Text(v) => Some(v.clone()),
            Value::Nil => Some(String::new()),
            _ => None,
        }
    }
}

/// A handle to a node: what `node()` and `let x = Label..{}` give. Properties are set with
/// typed setters (`set_text`, `set_value`, ..), which is what `INIT`/`UPDATE` call.
pub struct Handle<T> {
    memory: Rc<Memory>,
    id: NodeId,
    _t: PhantomData<fn() -> T>,
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle::new(self.memory.clone(), self.id)
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.memory, &other.memory) && self.id == other.id
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.alive() {
            write!(f, "{}#{}", self.kind(), self.id.0)
        } else {
            write!(f, "removed#{}", self.id.0)
        }
    }
}

impl<T> Handle<T> {
    fn new(memory: Rc<Memory>, id: NodeId) -> Self {
        Handle {
            memory,
            id,
            _t: PhantomData,
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The element type's name: `"Label"`.
    pub fn kind(&self) -> &'static str {
        self.memory.with(self.id, |n| n.kind)
    }

    /// Whether it's still in the tree (removed nodes are gone for good).
    pub fn alive(&self) -> bool {
        self.memory.alive(self.id)
    }

    /// The same node, as a plain [`Node`].
    pub fn untyped(&self) -> Handle<Node> {
        Handle::new(self.memory.clone(), self.id)
    }

    /// The same node, as a `U` (not checked).
    pub fn cast<U>(self) -> Handle<U> {
        Handle::new(self.memory, self.id)
    }

    pub fn children(&self) -> Vec<Handle<Node>> {
        self.memory
            .with(self.id, |n| n.children.clone())
            .into_iter()
            .map(|c| Handle::new(self.memory.clone(), c))
            .collect()
    }

    /// The `i`th child (panics if there's none).
    pub fn child(&self, i: usize) -> Handle<Node> {
        let children = self.children();
        children
            .get(i)
            .cloned()
            .unwrap_or_else(|| panic!("memory: {self:?} has no child #{i}"))
    }

    /// `Kind` or, for elements with text, `Kind:text`, for each child.
    pub fn describe(&self) -> Vec<String> {
        self.children()
            .iter()
            .map(|c| {
                let kind = c.kind();
                if has_text(kind) {
                    format!("{kind}:{}", c.get("text"))
                } else {
                    kind.to_string()
                }
            })
            .collect()
    }

    /// A property's value ([`Value::Nil`] if it wasn't set).
    pub fn get(&self, prop: &str) -> Value {
        self.memory
            .with(self.id, |n| n.props.get(prop).cloned())
            .unwrap_or(Value::Nil)
    }

    /// Sets a property by name, as something other than echo would (not firing events).
    pub fn set(&self, prop: &str, value: impl Into<Value>) {
        let value = value.into();
        self.memory.with(self.id, |n| {
            n.props.insert(prop.to_string(), value);
        });
    }

    /// Fires `event`, as input would.
    pub fn emit_signal(&self, event: &str, args: &[Value]) {
        self.memory.emit(self.id, event, args);
    }

    /// Removes it (and what's under it) from the tree, as something other than echo would.
    pub fn free(&self) {
        self.memory.remove(self.id);
    }

    /// Detaches it from its parent, as something other than echo would.
    pub fn detach(&self) {
        self.memory.detach(self.id);
    }

    /// Puts `child` at `index` among its children, as something other than echo would.
    pub fn move_child<C>(&self, child: &Handle<C>, index: usize) {
        Backend::place(&*self.memory, self.id, child.id, index);
    }

    /// Adds `child` as its last child, as something other than echo would.
    pub fn add_child<C>(&self, child: &Handle<C>) {
        let n = self.memory.with(self.id, |n| n.children.len());
        Backend::place(&*self.memory, self.id, child.id, n);
    }

    fn put(&self, prop: &str, value: Value) {
        self.set(prop, value);
    }
}

/// Elements with a `text`.
pub trait HasText {}

fn has_text(kind: &str) -> bool {
    matches!(kind, "Label" | "Button" | "LineEdit")
}

impl<T: HasText> Handle<T> {
    /// (On a [`LineEdit`], this puts the caret at the start, as an engine might.)
    pub fn set_text(&self, text: impl fmt::Display) {
        self.put("text", Value::Text(text.to_string()));
        if self.kind() == "LineEdit" {
            self.put("caret", Value::Int(0));
        }
    }
    pub fn get_text(&self) -> String {
        self.get("text").to()
    }
}

impl Handle<LineEdit> {
    pub fn get_caret_column(&self) -> i64 {
        match self.get("caret") {
            Value::Int(c) => c,
            _ => 0,
        }
    }
    pub fn set_caret_column(&self, column: i64) {
        self.put("caret", Value::Int(column));
    }
    /// Types `text` at the caret, as a user would: fires `text_changed(text)`.
    pub fn insert_text_at_caret(&self, text: &str) {
        let mut all = self.get_text();
        let at = (self.get_caret_column() as usize).min(all.len());
        all.insert_str(at, text);
        self.put("text", Value::Text(all.clone()));
        self.set_caret_column((at + text.len()) as i64);
        self.emit_signal("text_changed", &[Value::Text(all)]);
    }
}

impl<T: Is<Control>> Handle<T> {
    pub fn set_visible(&self, visible: bool) {
        self.put("visible", Value::Bool(visible));
    }
    pub fn set_theme_color(&self, name: &str, color: impl fmt::Display) {
        self.put(
            &format!("theme_color/{name}"),
            Value::Text(color.to_string()),
        );
    }
    pub fn set_theme_constant(&self, name: &str, value: impl Into<i64>) {
        self.put(&format!("theme_constant/{name}"), Value::Int(value.into()));
    }
    pub fn set_theme_font_size(&self, name: &str, size: impl Into<i64>) {
        self.put(&format!("theme_font_size/{name}"), Value::Int(size.into()));
    }
}

impl Handle<HSlider> {
    /// Fires `value_changed` if it changed, like a slider does.
    pub fn set_value(&self, value: impl Into<f64>) {
        let value = value.into();
        if self.get("value") != Value::Float(value) {
            self.put("value", Value::Float(value));
            self.emit_signal("value_changed", &[Value::Float(value)]);
        }
    }
    pub fn set_max_value(&self, value: impl Into<f64>) {
        self.put("max_value", Value::Float(value.into()));
    }
}

macro_rules! elements {
    ($($(#[$doc:meta])* $name:ident $(: $($base:ident),+)?;)*) => {$(
        $(#[$doc])*
        pub enum $name {}

        impl Element for $name {
            type Backend = Memory;
            type Handle = Handle<$name>;
            fn handle(memory: &Rc<Memory>, node: NodeId) -> Handle<$name> {
                Handle::new(memory.clone(), node)
            }
        }

        impl Create for $name {
            fn create(memory: &Rc<Memory>) -> NodeId {
                memory.create(stringify!($name))
            }
        }

        $($(impl Is<$base> for $name {})+)?
    )*};
}

elements! {
    /// A plain node.
    Node;
    /// Something visible (`visible`, theme overrides).
    Control: Node;
    Label: Control, Node;
    /// Fires `pressed`.
    Button: Control, Node;
    /// Fires `text_changed(text)`.
    LineEdit: Control, Node;
    BoxContainer: Control, Node;
    VBoxContainer: BoxContainer, Control, Node;
    HBoxContainer: BoxContainer, Control, Node;
    PanelContainer: Control, Node;
    /// `value` and `max_value`; fires `value_changed(value)`.
    HSlider: Control, Node;
}

impl HasText for Label {}
impl HasText for Button {}
impl HasText for LineEdit {}
