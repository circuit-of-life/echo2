//! `MEMO`: skipping parts of the tree whose inputs didn't change.
//!
//! The whole tree function runs every frame. That's cheap for small UIs but adds up for big
//! ones, like these 1000 rows. `MEMO(value) >> { .. }` skips its block, leaving its nodes as
//! they are, while `value` stays equal, so only rows that changed do any work. The value has
//! to cover everything the block shows that can change, or the block goes stale.
//!
//! Signals inside a memo still work: the rerun a signal causes re-runs every memo.

use echo::tree;
use godot::{
    classes::{
        Button, CheckButton, HBoxContainer, Label, ProgressBar, ScrollContainer, VBoxContainer,
        control::SizeFlags,
    },
    prelude::*,
};

struct Row {
    id: u32,
    name: String,
    value: i32,
}

pub struct State {
    rows: Vec<Row>,
    use_memo: bool,
    runs: u64,
    rng: u64,
}

impl Default for State {
    fn default() -> Self {
        let mut s = Self {
            rows: (0..1000)
                .map(|id| Row {
                    id,
                    name: format!("Row {id}"),
                    value: 0,
                })
                .collect(),
            use_memo: true,
            runs: 0,
            rng: 0x9E37_79B9_7F4A_7C15,
        };
        for i in 0..s.rows.len() {
            s.rows[i].value = (s.rng_next() % 101) as i32;
        }
        s
    }
}

impl State {
    fn rng_next(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
    fn change_some(&mut self) {
        for _ in 0..10 {
            let i = (self.rng_next() % self.rows.len() as u64) as usize;
            self.rows[i].value = (self.rng_next() % 101) as i32;
        }
    }
    fn shuffle(&mut self) {
        for i in (1..self.rows.len()).rev() {
            let j = (self.rng_next() % (i as u64 + 1)) as usize;
            self.rows.swap(i, j);
        }
    }
}

#[tree]
pub fn view(s: &mut State) {
    s.runs += 1;
    // With memos "off", give them a value that differs every run, so they always re-run.
    let every_run = if s.use_memo { 0 } else { s.runs };
    VBoxContainer..{
        INIT(size_flags_vertical = SizeFlags::EXPAND_FILL);
        HBoxContainer..{
            CheckButton..{
                INIT(text = "Use MEMO");
                if let Some(args) = ON(toggled) {
                    s.use_memo = args[0].to()
                }
                UPDATE(button_pressed = s.use_memo);
            };
            Button..{
                INIT(text = "Change 10 rows");
                if ON(pressed) {
                    s.change_some()
                }
            };
            Button..{
                INIT(text = "Shuffle");
                if ON(pressed) {
                    s.shuffle()
                }
            };
        };
        ScrollContainer..{
            INIT(size_flags_vertical = SizeFlags::EXPAND_FILL);
            VBoxContainer..{
                INIT(size_flags_horizontal = SizeFlags::EXPAND_FILL);
                for row in s.rows.iter_mut() {
                    KEY(row.id);
                    // Re-runs when this row's value changes. Skipped rows keep their nodes, and
                    // still move along when the list is shuffled.
                    MEMO((row.value, every_run)) >> {
                        HBoxContainer..{
                            Label..{
                                INIT(
                                    text = row.name.clone(),
                                    custom_minimum_size = Vector2::new(90.0, 0.0),
                                );
                            };
                            ProgressBar..{
                                INIT(
                                    max_value = 100.0,
                                    show_percentage = false,
                                    size_flags_horizontal = SizeFlags::EXPAND_FILL,
                                    size_flags_vertical = SizeFlags::SHRINK_CENTER,
                                );
                                UPDATE(value = row.value);
                            };
                            Button..{
                                INIT(text = "+10");
                                if ON(pressed) {
                                    row.value = (row.value + 10).min(100)
                                }
                            };
                        };
                    };
                }
            };
        };
    };
}
