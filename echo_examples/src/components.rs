//! Splitting a UI into reusable tree functions.
//!
//! A `#[tree]` function is called like a node: `card("Title")..{ body };`. It gets the caller's
//! body as its last parameter, `body: Body<N, Args>`, and places it with `body(args)` inside an
//! `N` node (the caller takes the arguments with `card(..)..|a, b| { .. }`). It returns its own
//! value, which the caller keeps with `let v = slider(..)..{};`.
//!
//! `#[tree(in Parent)]` only lets a function be used inside a `Parent` node, checked at compile
//! time. And `#[tree] let f = |..| { .. };` makes a closure that works like a tree function, and
//! can use the variables around it.

use echo::{Body, tree};
use godot::{
    classes::{
        ColorRect, HBoxContainer, HSeparator, HSlider, Label, MarginContainer, PanelContainer,
        VBoxContainer, control::SizeFlags,
    },
    prelude::*,
};

/// A panel with a title. Whatever the caller writes in its body goes under the title.
#[tree]
fn card(title: &str, body: Body<VBoxContainer>) {
    PanelContainer..{
        MarginContainer..{
            INIT(
                theme(constant, margin_left) = 12,
                theme(constant, margin_right) = 12,
                theme(constant, margin_top) = 8,
                theme(constant, margin_bottom) = 12,
            );
            VBoxContainer..{
                Label..{
                    INIT(theme(font_size, font_size) = 18);
                    UPDATE(text = title.to_string());
                };
                HSeparator..{};
                body()
            };
        };
    };
}

/// A labelled slider editing `value`. Returns whether the value changed this run.
#[tree]
fn slider(label: &str, value: &mut f64) -> bool {
    let mut changed = false;
    HBoxContainer..{
        Label..{
            INIT(custom_minimum_size = Vector2::new(60.0, 0.0));
            UPDATE(text = label.to_string());
        };
        HSlider..{
            INIT(
                max_value = 255.0,
                size_flags_horizontal = SizeFlags::EXPAND_FILL,
                size_flags_vertical = SizeFlags::SHRINK_CENTER,
            );
            if let Some(args) = ON(value_changed) {
                *value = args[0].to();
                changed = true
            }
            UPDATE(value = *value);
        };
        Label..{
            INIT(custom_minimum_size = Vector2::new(36.0, 0.0));
            UPDATE(text = format!("{:.0}", *value));
        };
    };
    changed
}

/// Can only be used inside a `PanelContainer` (try using it elsewhere: it doesn't compile).
#[tree(in PanelContainer)]
fn hint(text: &str) {
    MarginContainer..{
        INIT(
            theme(constant, margin_left) = 8,
            theme(constant, margin_right) = 8,
        );
        Label..{
            INIT(modulate = Color::from_rgba(1.0, 1.0, 1.0, 0.6));
            UPDATE(text = text.to_string());
        };
    };
}

pub struct State {
    rgb: [f64; 3],
    last_changed: Option<&'static str>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            rgb: [90.0, 140.0, 220.0],
            last_changed: None,
        }
    }
}

#[tree]
pub fn view(s: &mut State) {
    VBoxContainer..{
        INIT(theme(constant, separation) = 12);
        card("Color")..{
            // a tree closure: like a tree function, but it can use `s` from out here
            #[tree]
            let mut channel = |name: &'static str, value: &mut f64| {
                let changed = slider(name, value)..{};
                if changed {
                    s.last_changed = Some(name)
                }
            };
            let [r, g, b] = &mut s.rgb;
            channel("Red", r)..{};
            channel("Green", g)..{};
            channel("Blue", b)..{};
        };
        card("Preview")..{
            ColorRect..{
                INIT(custom_minimum_size = Vector2::new(0.0, 80.0));
                UPDATE(
                    color = Color::from_rgb(
                        s.rgb[0] as f32 / 255.0,
                        s.rgb[1] as f32 / 255.0,
                        s.rgb[2] as f32 / 255.0,
                    ),
                );
            };
            let text = match s.last_changed {
                Some(c) => format!("You last changed {c}."),
                None => "Move a slider.".to_string(),
            };
            PanelContainer..{
                hint(&text)..{};
            };
        };
    };
}
