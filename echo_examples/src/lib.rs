//! Runnable examples for echo. `echo_examples/run.sh` opens a window with all of them.
//!
//! Start with `counter.rs`. This file is the app around them, and shows how to hook echo up to
//! a Godot node.

use std::time::{Duration, Instant};

use echo::{App, tree};
use godot::{
    classes::{
        Button, Control, HBoxContainer, HSeparator, IControl, Label, MarginContainer, Os,
        PanelContainer, VBoxContainer, control::SizeFlags, text_server::AutowrapMode,
    },
    global::HorizontalAlignment,
    prelude::*,
};

mod components;
mod counter;
mod local_state;
mod memo;
mod smoke;
mod todo;

// Every node a tree function creates goes through `crate::__echo_deny!(Type)` first. Nothing is
// denied here; a project could use it to reject some node types with `compile_error!`.
macro_rules! __echo_deny {
    ($($t:tt)*) => {};
}
pub(crate) use __echo_deny;

struct Examples;

#[gdextension]
unsafe impl ExtensionLibrary for Examples {}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
enum Example {
    #[default]
    Counter,
    Todo,
    Components,
    LocalState,
    Memo,
}

impl Example {
    const ALL: [Example; 5] = [
        Example::Counter,
        Example::Todo,
        Example::Components,
        Example::LocalState,
        Example::Memo,
    ];
    fn title(self) -> &'static str {
        match self {
            Example::Counter => "Counter",
            Example::Todo => "Todo list",
            Example::Components => "Components",
            Example::LocalState => "Local state",
            Example::Memo => "Memo",
        }
    }
    fn blurb(self) -> &'static str {
        match self {
            Example::Counter => {
                "The basics: nodes, INIT and UPDATE properties, and reacting to signals with `if ON(..)`."
            }
            Example::Todo => {
                "A keyed `for` loop, text input, and changing a list while it's being shown."
            }
            Example::Components => {
                "Splitting a UI into tree functions and closures, which take a body and return values."
            }
            Example::LocalState => {
                "STATE: state that lives at a place in the tree. Each section remembers whether it's open, and switching examples resets it."
            }
            Example::Memo => {
                "MEMO skips parts of the tree whose inputs didn't change. Compare the run time with and without it (it's much bigger in a debug build)."
            }
        }
    }
}

/// Everything the gallery shows. Each example keeps its state here, so it survives switching
/// between examples (unlike `STATE`, which belongs to the nodes).
#[derive(Default)]
pub struct GalleryState {
    selected: Example,
    last_run: Duration,
    counter: counter::State,
    todo: todo::State,
    components: components::State,
    local_state: local_state::State,
    memo: memo::State,
}

#[tree]
fn gallery(s: &mut GalleryState) {
    HBoxContainer..{
        INIT(anchor_right = 1.0, anchor_bottom = 1.0);
        PanelContainer..{
            VBoxContainer..{
                INIT(custom_minimum_size = Vector2::new(200.0, 0.0));
                Label..{
                    INIT(text = "echo examples", theme(font_size, font_size) = 20);
                };
                HSeparator..{};
                for example in Example::ALL {
                    KEY(example);
                    Button..{
                        INIT(
                            text = example.title(),
                            toggle_mode = true,
                            alignment = HorizontalAlignment::LEFT,
                        );
                        if ON(pressed) {
                            s.selected = example
                        }
                        UPDATE(button_pressed = s.selected == example);
                    };
                }
                Control..{
                    INIT(size_flags_vertical = SizeFlags::EXPAND_FILL);
                };
                Label..{
                    UPDATE(text = format!("last run: {:.2} ms", s.last_run.as_secs_f64() * 1000.0));
                };
            };
        };
        MarginContainer..{
            INIT(
                name = "Content",
                size_flags_horizontal = SizeFlags::EXPAND_FILL,
                theme(constant, margin_left) = 16,
                theme(constant, margin_right) = 16,
                theme(constant, margin_top) = 12,
                theme(constant, margin_bottom) = 12,
            );
            VBoxContainer..{
                INIT(theme(constant, separation) = 12);
                Label..{
                    INIT(autowrap_mode = AutowrapMode::WORD_SMART);
                    UPDATE(text = s.selected.blurb());
                };
                HSeparator..{};
                // each arm is its own scope: switching examples frees the old one's nodes
                match s.selected {
                    Example::Counter => {
                        counter::view(&mut s.counter)..{};
                    }
                    Example::Todo => {
                        todo::view(&mut s.todo)..{};
                    }
                    Example::Components => {
                        components::view(&mut s.components)..{};
                    }
                    Example::LocalState => {
                        local_state::view(&mut s.local_state)..{};
                    }
                    Example::Memo => {
                        memo::view(&mut s.memo)..{};
                    }
                }
            };
        };
    };
}

/// The node the UI lives under. Hooking echo up takes three things: an `App` made in `ready`,
/// running it every frame, and running it again when it asks to (after a signal it listens
/// to fired), via `__echo_rerun`.
#[derive(GodotClass)]
#[class(init, base = Control)]
struct Gallery {
    app: Option<App<Node>>,
    state: GalleryState,
    base: Base<Control>,
}

#[godot_api]
impl IControl for Gallery {
    fn ready(&mut self) {
        let root = self.to_gd().upcast::<Node>();
        self.app = Some(App::new(root));

        // When a signal used with `ON(..)` fires, the root emits `__echo_rerun(force_memo)`.
        // Running the app then is what lets `ON(..)` see the event.
        let this = self.to_gd();
        self.base_mut().connect(
            "__echo_rerun",
            &Callable::from_fn("rerun", move |args| {
                this.clone().bind_mut().run(args[0].to::<bool>());
            }),
        );

        if Os::singleton().get_cmdline_user_args().contains("--smoke") {
            let driver = smoke::SmokeDriver::new(self.to_gd());
            self.base_mut()
                .call_deferred("add_sibling", &[driver.to_variant()]);
        }
    }

    fn process(&mut self, _delta: f64) {
        self.run(false);
    }
}

impl Gallery {
    fn run(&mut self, force_memo: bool) {
        let Some(app) = &self.app else {
            return;
        };
        let start = Instant::now();
        // The view can borrow anything; here, the state next to the app.
        app.run(force_memo, |b| gallery(b, &mut self.state, &mut |_, _| {}));
        self.state.last_run = start.elapsed();
    }
}
