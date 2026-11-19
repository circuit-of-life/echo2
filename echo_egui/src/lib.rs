//! An experimental [egui](https://docs.rs/egui) backend for echo: Godot's Control nodes, kept
//! in a tree and drawn with egui every frame.
//!
//! ```ignore
//! #[tree]
//! fn counter(clicks: &mut u32) {
//!     VBoxContainer..{
//!         Label..{ UPDATE(text = format!("{clicks} clicks")) };
//!         Button..{
//!             INIT(text = "Click");
//!             if ON(pressed) {
//!                 *clicks += 1;
//!             }
//!         };
//!     };
//! }
//!
//! // once
//! let app = EguiApp::new();
//! // every frame, in some egui `ui`
//! app.show(ui, |b| counter(b.upcast(), &mut clicks, &mut |_, _| {}));
//! ```
//!
//! # How it fits egui
//!
//! egui draws widgets as code runs and reports what happened to them right away; echo keeps a
//! tree of nodes and tells tree code about events through `ON(..)`. So each frame,
//! [`EguiApp::show`]:
//!
//! 1. runs the tree code, which updates the tree (as with any backend);
//! 2. draws the tree with egui, one widget or layout per node, from its properties, turning
//!    what egui reports into Godot's events (`pressed`, `text_changed(text)`,
//!    `value_changed(value)`, `mouse_entered`, ..);
//! 3. if any `ON(..)` waits for one of those events, runs the tree code again, with the events,
//!    and asks egui for another frame, which shows what the handlers changed.
//!
//! Widgets keep their state in their node's properties, as in Godot: a slider drags the node's
//! `value`, a line edit edits its `text`. egui's own state (scrolling, focus, an open drop-down)
//! is keyed by the node, which echo keeps for as long as its element runs.
//!
//! # What's there
//!
//! The node types in this crate's root, with Godot's names, properties and signals, as far as
//! they mean something in egui. Layout is egui's: rows, columns, wraps, grids and frames, with
//! `custom_minimum_size` and filling the width (`SizeFlags::Fill`/`ExpandFill`) on top. There's
//! no anchoring or Godot themes, and no 2D nodes (`Node2D`, `Sprite2D`, `Camera2D`, ..).

mod backend;
mod elements;
mod handle;
mod render;
mod value;

use std::rc::Rc;

use echo::{App, Builder};

pub use backend::Egui;
pub use elements::*;
pub use handle::{Handle, HasColor, HasText};
pub use value::{FromValue, ScrollMode, SizeFlags, Value};

/// An echo app drawn with egui. Make one, then call [`show`](Self::show) every frame.
pub struct EguiApp {
    egui: Rc<Egui>,
    app: App<Node>,
}

impl Default for EguiApp {
    fn default() -> Self {
        Self::new()
    }
}

impl EguiApp {
    pub fn new() -> Self {
        let egui = Egui::new();
        let app = App::new(egui.clone(), egui.root);
        Self { egui, app }
    }

    /// The root node, which `view`'s builder builds in.
    pub fn root(&self) -> Handle<Node> {
        Handle::new(self.egui.clone(), self.egui.root)
    }

    /// Runs `view` and draws the tree into `ui` (top to bottom, in `ui`'s layout).
    ///
    /// Events from this frame are handled by running `view` again right after (so the `ON(..)`s
    /// waiting for them see them); what that changes shows from the next frame, which is
    /// requested.
    pub fn show<R>(&self, ui: &mut egui::Ui, mut view: impl FnMut(Builder<Node>) -> R) {
        self.app.run(false, &mut view);
        let events = self.egui.render(ui);
        if !events.is_empty() {
            self.egui.deliver(events);
        }
        if self.egui.take_rerun() {
            self.app.run(true, &mut view);
            ui.ctx().request_repaint();
        }
    }
}
