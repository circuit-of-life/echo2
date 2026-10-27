//! A todo list.
//!
//! A `for` loop's body starts with a `KEY(..)`, which ties each item to its nodes: when items
//! are reordered or removed, their nodes follow them, instead of each row being reused for
//! whatever item ends up at that index.

use echo::tree;
use godot::{
    classes::{
        Button, CheckBox, HBoxContainer, Label, LineEdit, VBoxContainer, control::SizeFlags,
    },
    prelude::*,
};

struct Todo {
    id: u64,
    text: String,
    done: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
enum Filter {
    #[default]
    All,
    Open,
    Done,
}

impl Filter {
    const ALL: [Filter; 3] = [Filter::All, Filter::Open, Filter::Done];
    fn name(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Open => "Open",
            Filter::Done => "Done",
        }
    }
    fn shows(self, todo: &Todo) -> bool {
        match self {
            Filter::All => true,
            Filter::Open => !todo.done,
            Filter::Done => todo.done,
        }
    }
}

pub struct State {
    todos: Vec<Todo>,
    next_id: u64,
    input: String,
    filter: Filter,
}

impl Default for State {
    fn default() -> Self {
        let mut s = Self {
            todos: vec![],
            next_id: 0,
            input: String::new(),
            filter: Filter::All,
        };
        for text in ["Try the filters", "Reorder something", "Check this one off"] {
            s.input = text.into();
            s.add();
        }
        s
    }
}

impl State {
    fn add(&mut self) {
        let text = self.input.trim();
        if !text.is_empty() {
            self.todos.push(Todo {
                id: self.next_id,
                text: text.to_string(),
                done: false,
            });
            self.next_id += 1;
        }
        self.input.clear();
    }
}

#[tree]
pub fn view(s: &mut State) {
    VBoxContainer..{
        INIT(theme(constant, separation) = 8);
        HBoxContainer..{
            // `let` gives the code after it the node itself, here a `Gd<LineEdit>`
            let mut input = LineEdit..{
                INIT(
                    placeholder_text = "What needs doing?",
                    size_flags_horizontal = SizeFlags::EXPAND_FILL,
                );
                if let Some(args) = ON(text_changed) {
                    s.input = args[0].to()
                }
                if ON(text_submitted) {
                    s.add()
                }
                // The text lives in the state (so `add` can clear it). It's only set when it
                // differs, because setting a LineEdit's text also moves its caret to the start.
                // That's also why the events come first: when typing, `s.input` already has the
                // new text by now, so nothing is set.
                if __builder.node().get_text() != s.input.as_str() {
                    UPDATE(text = s.input.clone());
                }
            };
            Button..{
                INIT(text = "Add");
                if ON(pressed) {
                    s.add();
                    // clicking took the focus away from the input, so type on from there
                    input.grab_focus()
                }
                UPDATE(disabled = s.input.trim().is_empty());
            };
        };

        // The list can't change while it's being iterated, so buttons in it only note what to
        // do, and it happens after the loop.
        let mut move_up = None;
        let mut remove = None;
        VBoxContainer..{
            for (i, todo) in s
                .todos
                .iter_mut()
                .enumerate()
                .filter(|(_, t)| s.filter.shows(t))
            {
                KEY(todo.id);
                HBoxContainer..{
                    CheckBox..{
                        if let Some(args) = ON(toggled) {
                            todo.done = args[0].to()
                        }
                        UPDATE(button_pressed = todo.done);
                    };
                    Label..{
                        INIT(size_flags_horizontal = SizeFlags::EXPAND_FILL);
                        UPDATE(
                            text = todo.text.clone(),
                            modulate = if todo.done {
                                Color::from_rgba(1.0, 1.0, 1.0, 0.45)
                            } else {
                                Color::WHITE
                            },
                        );
                    };
                    Button..{
                        INIT(text = "Up");
                        if ON(pressed) {
                            move_up = Some(i)
                        }
                        UPDATE(disabled = i == 0);
                    };
                    Button..{
                        INIT(text = "Remove");
                        if ON(pressed) {
                            remove = Some(i)
                        }
                    };
                };
            }
        };
        if let Some(i) = move_up {
            s.todos.swap(i - 1, i)
        }
        if let Some(i) = remove {
            s.todos.remove(i);
        }

        if !s.todos.iter().any(|t| s.filter.shows(t)) {
            Label..{
                UPDATE(
                    text = match s.filter {
                        Filter::All => "Nothing to do.",
                        Filter::Open => "All done!",
                        Filter::Done => "Nothing done yet.",
                    },
                );
            };
        }

        HBoxContainer..{
            Label..{
                INIT(size_flags_horizontal = SizeFlags::EXPAND_FILL);
                UPDATE(text = format!("{} left", s.todos.iter().filter(|t| !t.done).count()));
            };
            for filter in Filter::ALL {
                KEY(filter);
                Button..{
                    INIT(text = filter.name(), toggle_mode = true);
                    if ON(pressed) {
                        s.filter = filter
                    }
                    UPDATE(button_pressed = s.filter == filter);
                };
            }
            Button..{
                INIT(text = "Clear done");
                if ON(pressed) {
                    s.todos.retain(|t| !t.done)
                }
                UPDATE(disabled = !s.todos.iter().any(|t| t.done));
            };
        };
    };
}
