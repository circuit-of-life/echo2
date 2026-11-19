use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use echo::{Backend, ConnId, NodeId};
use egui::{Align, Color32, ImageSource, Pos2, Vec2};

use crate::{
    elements::Kind,
    render::{Event, Render},
    value::{ScrollMode, SizeFlags, Value},
};

type Fire = Rc<dyn Fn(Vec<Value>)>;

/// The backend: a tree of nodes with Godot-like properties, which [`EguiApp`](crate::EguiApp)
/// draws with egui every frame.
pub struct Egui {
    pub(crate) nodes: RefCell<Vec<Option<NodeData>>>,
    pub(crate) root: NodeId,
    next_conn: Cell<u64>,
    rerun: Cell<bool>,
    /// Keeps this tree's egui ids apart from another's.
    salt: egui::Id,
}

pub(crate) struct NodeData {
    pub kind: Kind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub props: Props,
    conns: Vec<(ConnId, &'static str, Fire)>,
    watchers: Vec<Rc<Cell<bool>>>,
    /// Last frame's pointer state, to turn egui's per-frame answers into enter/exit and
    /// down/up events.
    pub hovered: bool,
    pub down: bool,
    /// Where it was drawn last frame.
    pub rect: egui::Rect,
}

/// Every property any node has (each node uses the ones its kind draws with).
pub(crate) struct Props {
    // Control
    pub visible: bool,
    pub modulate: Option<Color32>,
    pub min_size: Vec2,
    pub max_size: Option<Vec2>,
    pub h_flags: SizeFlags,
    pub v_flags: SizeFlags,
    pub tooltip: Option<String>,
    pub font_size: Option<f32>,
    pub font_color: Option<Color32>,
    // text and buttons
    pub text: String,
    pub placeholder: String,
    pub disabled: bool,
    pub pressed: bool,
    pub toggle_mode: bool,
    pub flat: bool,
    pub icon: Option<ImageSource<'static>>,
    pub align: Option<Align>,
    pub autowrap: bool,
    pub max_length: Option<usize>,
    // ranges
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub exp_edit: bool,
    pub show_percentage: bool,
    pub suffix: String,
    // the rest
    pub color: Color32,
    pub separation: Option<f32>,
    pub h_separation: Option<f32>,
    pub v_separation: Option<f32>,
    /// left, top, right, bottom
    pub margins: [f32; 4],
    pub columns: usize,
    pub title: String,
    pub folded: bool,
    pub texture: Option<ImageSource<'static>>,
    pub items: Vec<String>,
    pub selected: i64,
    pub position: Option<Pos2>,
    pub h_scroll: ScrollMode,
    pub v_scroll: ScrollMode,
}

impl Default for Props {
    fn default() -> Self {
        Props {
            visible: true,
            modulate: None,
            min_size: Vec2::ZERO,
            max_size: None,
            h_flags: SizeFlags::default(),
            v_flags: SizeFlags::default(),
            tooltip: None,
            font_size: None,
            font_color: None,
            text: String::new(),
            placeholder: String::new(),
            disabled: false,
            pressed: false,
            toggle_mode: false,
            flat: false,
            icon: None,
            align: None,
            autowrap: false,
            max_length: None,
            value: 0.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
            exp_edit: false,
            show_percentage: true,
            suffix: String::new(),
            color: Color32::WHITE,
            separation: None,
            h_separation: None,
            v_separation: None,
            margins: [0.0; 4],
            columns: 1,
            title: String::new(),
            folded: false,
            texture: None,
            items: Vec::new(),
            selected: -1,
            position: None,
            h_scroll: ScrollMode::Auto,
            v_scroll: ScrollMode::Auto,
        }
    }
}

impl Egui {
    pub(crate) fn new() -> Rc<Self> {
        static TREES: AtomicU64 = AtomicU64::new(0);
        let egui = Egui {
            nodes: RefCell::new(Vec::new()),
            root: NodeId(0),
            next_conn: Cell::new(0),
            rerun: Cell::new(false),
            salt: egui::Id::new(("echo_egui", TREES.fetch_add(1, Ordering::Relaxed))),
        };
        egui.create(Kind::Node);
        Rc::new(egui)
    }

    pub(crate) fn create(&self, kind: Kind) -> NodeId {
        let mut nodes = self.nodes.borrow_mut();
        let id = NodeId(nodes.len() as u64);
        nodes.push(Some(NodeData {
            kind,
            parent: None,
            children: Vec::new(),
            props: Props {
                // popups are hidden until they pop up
                visible: kind != Kind::PopupPanel,
                ..Props::default()
            },
            conns: Vec::new(),
            watchers: Vec::new(),
            hovered: false,
            down: false,
            rect: egui::Rect::NOTHING,
        }));
        id
    }

    pub(crate) fn with<R>(&self, node: NodeId, f: impl FnOnce(&mut NodeData) -> R) -> R {
        let mut nodes = self.nodes.borrow_mut();
        let data = nodes
            .get_mut(node.0 as usize)
            .and_then(Option::as_mut)
            .unwrap_or_else(|| panic!("echo_egui: node {} was removed", node.0));
        f(data)
    }

    /// Draws the tree into `ui`, returning the events that drawing it caused.
    pub(crate) fn render(&self, ui: &mut egui::Ui) -> Vec<Event> {
        let mut nodes = self.nodes.borrow_mut();
        let mut render = Render {
            nodes: &mut nodes,
            events: Vec::new(),
            salt: self.salt,
        };
        render.children(ui, self.root);
        render.events
    }

    pub(crate) fn take_rerun(&self) -> bool {
        self.rerun.replace(false)
    }

    /// Calls what's connected to these events (with nothing borrowed: this runs echo code).
    pub(crate) fn deliver(&self, events: Vec<Event>) {
        for (node, event, args) in events {
            let fires: Vec<Fire> = {
                let nodes = self.nodes.borrow();
                let Some(Some(data)) = nodes.get(node.0 as usize) else {
                    continue;
                };
                data.conns
                    .iter()
                    .filter(|(_, e, _)| *e == event)
                    .map(|(_, _, f)| f.clone())
                    .collect()
            };
            for fire in fires {
                fire(args.clone());
            }
        }
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
}

impl Backend for Egui {
    /// The event's arguments, as in Godot (`text_changed(text)`, `value_changed(value)`, ..).
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
