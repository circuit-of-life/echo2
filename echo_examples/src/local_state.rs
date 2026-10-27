//! `STATE`: state that belongs to a place in the tree instead of to your state struct.
//!
//! `let mut x = STATE(init);` creates it the first time it runs (that's the only time `init` is
//! evaluated), and keeps it while it keeps running. When it stops running (its `if` branch goes
//! away, its `for` item is removed, ..) the state is dropped, and comes back fresh.
//!
//! `x` works like a `&mut` to the value: `*x += 1`, `x.push(..)`, `f(&mut x)`, `format!("{x}")`.

use echo::{Body, tree};
use godot::{
    classes::{Button, CheckButton, Label, MarginContainer, VBoxContainer},
    global::HorizontalAlignment,
};

/// A foldable section. Whether it's open is its own business: nothing outside tracks it.
///
/// Folded, its body doesn't run, so everything in it (nodes and `STATE`) goes away. To keep it,
/// keep the body running and hide it instead: `UPDATE(visible = *open)`.
#[tree]
fn collapsible(title: &str, body: Body<VBoxContainer>) {
    VBoxContainer..{
        let mut open = STATE(true);
        Button..{
            INIT(flat = true, alignment = HorizontalAlignment::LEFT);
            if ON(pressed) {
                *open = !*open
            }
            UPDATE(text = format!("{} {title}", if *open { "[-]" } else { "[+]" }));
        };
        if *open {
            MarginContainer..{
                INIT(theme(constant, margin_left) = 24);
                VBoxContainer..{ body() };
            };
        }
    };
}

/// A button counting its own clicks.
#[tree]
fn click_counter() {
    let mut clicks = STATE(0);
    Button..{
        if ON(pressed) {
            *clicks += 1
        }
        UPDATE(text = format!("Clicked {clicks} times"));
    };
}

#[derive(Default)]
pub struct State {
    show_third: bool,
}

#[tree]
pub fn view(s: &mut State) {
    VBoxContainer..{
        collapsible("First section")..{
            Label..{
                INIT(text = "Fold me. Each section keeps its own open/closed state.");
            };
        };
        collapsible("Second section")..{
            click_counter()..{};
            click_counter()..{};
            collapsible("Nested section")..{
                Label..{
                    INIT(text = "Folding the section around this one drops everything in it:");
                };
                Label..{
                    INIT(text = "the counters' clicks, and whether this section was folded.");
                };
                click_counter()..{};
            };
        };
        CheckButton..{
            INIT(text = "Show a third section");
            if let Some(args) = ON(toggled) {
                s.show_third = args[0].to()
            }
            UPDATE(button_pressed = s.show_third);
        };
        if s.show_third {
            collapsible("Third section")..{
                Label..{
                    INIT(text = "Hiding this section drops its state; it comes back fresh.");
                };
                click_counter()..{};
            };
        }
    };
}
