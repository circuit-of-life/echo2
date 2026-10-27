//! `run.sh --smoke`: goes through every example headlessly, poking its buttons, sliders and text
//! fields, so a panic or an example that builds nothing fails the run.

use godot::{
    classes::{BaseButton, LineEdit, Range},
    prelude::*,
};

use crate::{Example, Gallery};

/// Frames spent on each example: select, let it build, poke, let it settle.
const FRAMES: u32 = 4;
/// Pokes per example, so the 1000-row example doesn't take forever in a debug build.
const MAX_POKES: usize = 60;

#[derive(GodotClass)]
#[class(no_init, base = Node)]
pub struct SmokeDriver {
    gallery: Gd<Gallery>,
    frame: u32,
    pokes: usize,
    base: Base<Node>,
}

impl SmokeDriver {
    pub fn new(gallery: Gd<Gallery>) -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            gallery,
            frame: 0,
            pokes: 0,
            base,
        })
    }
}

fn descendants(node: &Gd<Node>, out: &mut Vec<Gd<Node>>) {
    for child in node.get_children().iter_shared() {
        if !child.is_queued_for_deletion() {
            out.push(child.clone());
            descendants(&child, out);
        }
    }
}

#[godot_api]
impl INode for SmokeDriver {
    fn process(&mut self, _delta: f64) {
        // twice through, so examples are also visited with state left from the first time
        let visits = Example::ALL.len() as u32 * 2;
        let (step, phase) = (self.frame / FRAMES, self.frame % FRAMES);
        self.frame += 1;
        if step >= visits {
            println!(
                "smoke test passed: {visits} example visits, {} pokes",
                self.pokes
            );
            self.base().get_tree().quit();
            return;
        }
        let example = Example::ALL[step as usize % Example::ALL.len()];
        match phase {
            0 => self.gallery.bind_mut().state.selected = example,
            2 => {
                // `owned(false)`: nodes made at runtime have no owner
                let content = self
                    .gallery
                    .find_child_ex("Content")
                    .owned(false)
                    .done()
                    .expect("gallery has a Content node");
                let mut nodes = vec![];
                descendants(&content, &mut nodes);
                if nodes.len() < 3 {
                    godot_error!("smoke: `{}` built nothing", example.title());
                }
                // the first child is the description, then the example
                for node in nodes.into_iter().take(MAX_POKES) {
                    // an earlier poke may have removed it
                    if !node.is_instance_valid() || node.is_queued_for_deletion() {
                        continue;
                    }
                    self.pokes += poke(node);
                }
            }
            _ => {}
        }
    }
}

/// Acts like a user on whatever kind of control `node` is. Each signal makes the gallery rerun
/// right away, like real input does.
fn poke(node: Gd<Node>) -> usize {
    if let Ok(mut button) = node.clone().try_cast::<BaseButton>() {
        if button.is_disabled() {
            return 0;
        }
        if button.is_toggle_mode() {
            let on = !button.is_pressed();
            button.set_pressed(on);
        }
        button.emit_signal("pressed", &[]);
        1
    } else if let Ok(mut range) = node.clone().try_cast::<Range>() {
        let mid = (range.get_min() + range.get_max()) / 2.0;
        let value = if range.get_value() == mid {
            range.get_max()
        } else {
            mid
        };
        range.set_value(value);
        1
    } else if let Ok(mut edit) = node.try_cast::<LineEdit>() {
        let text = "smoke test".to_variant();
        edit.emit_signal("text_changed", std::slice::from_ref(&text));
        edit.emit_signal("text_submitted", &[text]);
        1
    } else {
        0
    }
}
