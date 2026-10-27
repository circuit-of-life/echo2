//! The basics.
//!
//! A tree function describes the nodes for the current state, and runs every frame. echo keeps
//! the actual nodes in sync with it: nodes are created the first time and then reused, `INIT`
//! sets properties when a node is created, `UPDATE` on every run.
//!
//! When a signal fires, the tree runs once more, and its `ON(..)` is true at the place it's
//! checked. Code runs top to bottom, so check a node's events *before* the `UPDATE`s showing
//! what they change: otherwise that run first sets the node back to the old value.

use echo::tree;
use godot::{
    classes::{Button, HBoxContainer, Label, SpinBox, VBoxContainer},
    prelude::*,
};

pub struct State {
    count: i32,
    step: i32,
}

impl Default for State {
    fn default() -> Self {
        Self { count: 0, step: 1 }
    }
}

#[tree]
pub fn view(s: &mut State) {
    VBoxContainer..{
        INIT(theme(constant, separation) = 8);
        Label..{
            INIT(theme(font_size, font_size) = 32);
            UPDATE(text = format!("Count: {}", s.count));
        };
        HBoxContainer..{
            Button..{
                INIT(text = " - ");
                // True during the run caused by this button's `pressed` signal. echo connects
                // the signal while this `ON` keeps being checked.
                if ON(pressed) {
                    s.count -= s.step
                }
            };
            Button..{
                INIT(text = " + ");
                if ON(pressed) {
                    s.count += s.step
                }
            };
            Button..{
                INIT(text = "Reset");
                if ON(pressed) {
                    s.count = 0
                }
                UPDATE(disabled = s.count == 0);
            };
        };
        HBoxContainer..{
            Label..{
                INIT(text = "Step");
            };
            SpinBox..{
                INIT(min_value = 1.0, max_value = 10.0);
                // `ON(signal)` is `Some(args)` when it fired, for signals with arguments
                if let Some(args) = ON(value_changed) {
                    s.step = args[0].to::<f64>() as i32
                }
                // Setting `value` makes the SpinBox emit `value_changed` itself. echo ignores
                // signals caused by its own runs, so this doesn't feed back into the handler.
                UPDATE(value = s.step);
            };
        };
        // Nodes in an `if` exist while it's true. Each branch is its own place in the tree,
        // so this coming and going doesn't disturb the nodes after it.
        if s.count < 0 {
            Label..{
                INIT(
                    text = "Below zero!",
                    theme(color, font_color) = Color::from_rgb(1.0, 0.45, 0.45),
                );
            };
        }
        Label..{
            UPDATE(
                text = format!(
                    "{} is {}",
                    s.count,
                    if s.count % 2 == 0 { "even" } else { "odd" }
                ),
            );
        };
    };
}
