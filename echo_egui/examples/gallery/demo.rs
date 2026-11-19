//! A tour of the node types, written as for Godot. Shared by the gallery example and the tests.

use echo::tree;
use echo_egui::*;
use egui::Color32;

pub struct Demo {
    pub clicks: u32,
    pub name: String,
    pub greeting: Option<String>,
    pub volume: f64,
    pub fullscreen: bool,
    pub quality: i64,
    pub accent: Color32,
    pub todos: Vec<Todo>,
    pub next_id: u32,
    pub draft: String,
    pub notes: String,
    pub log: Vec<String>,
}

pub struct Todo {
    pub id: u32,
    pub text: String,
    pub done: bool,
}

impl Default for Demo {
    fn default() -> Self {
        Demo {
            clicks: 0,
            name: String::new(),
            greeting: None,
            volume: 40.0,
            fullscreen: false,
            quality: 1,
            accent: Color32::from_rgb(90, 140, 220),
            todos: ["Port the menus", "Try egui"]
                .iter()
                .enumerate()
                .map(|(i, t)| Todo {
                    id: i as u32,
                    text: t.to_string(),
                    done: i == 0,
                })
                .collect(),
            next_id: 2,
            draft: String::new(),
            notes: String::new(),
            log: Vec::new(),
        }
    }
}

impl Demo {
    fn log(&mut self, event: impl Into<String>) {
        self.log.push(event.into());
        if self.log.len() > 4 {
            self.log.remove(0);
        }
    }
}

#[tree]
pub fn demo(s: &mut Demo) {
    MarginContainer..{
        INIT(
            theme_constant("margin_left") = 12,
            theme_constant("margin_top") = 12,
            theme_constant("margin_right") = 12,
            theme_constant("margin_bottom") = 12,
        );
        VBoxContainer..{
            INIT(theme_constant("separation") = 8);
            Label..{
                INIT(
                    text = "echo, drawn by egui",
                    theme_font_size("font_size") = 22.0,
                );
            };
            header(s)..{};
            HSeparator..{};
            FoldableContainer..{
                INIT(title = "Settings");
                settings(s)..{};
            };
            FoldableContainer..{
                INIT(title = "Todo list");
                todo_list(s)..{};
            };
            FoldableContainer..{
                INIT(title = "Notes", folded = true);
                TextEdit..{
                    INIT(
                        placeholder_text = "Anything..",
                        custom_minimum_size = [0.0, 60.0],
                    );
                    INIT(size_flags_horizontal = SizeFlags::ExpandFill);
                    if ON(text_changed) {
                        s.notes = __builder.node().get_text();
                    }
                };
                Label..{
                    UPDATE(text = format!("{} characters", s.notes.chars().count()));
                };
            };
            HSeparator..{};
            for (i, line) in s.log.iter().enumerate() {
                KEY(i);
                Label..{
                    INIT(modulate = Color32::from_white_alpha(140));
                    UPDATE(text = line);
                };
            }
        };
    };
}

#[tree(in BoxContainer)]
fn header(s: &mut Demo) {
    HBoxContainer..{
        Button..{
            INIT(text = "Click me", tooltip_text = "Counts clicks");
            if ON(pressed) {
                s.clicks += 1;
            }
            if ON(mouse_entered) {
                s.log("hovered the button");
            }
        };
        Label..{
            UPDATE(text = format!("{} clicks", s.clicks));
        };
        let menu = PopupPanel..{
            VBoxContainer..{
                Label..{
                    INIT(text = "Menu");
                };
                Button..{
                    INIT(text = "Reset clicks", flat = true);
                    if ON(pressed) {
                        s.clicks = 0;
                        s.log("reset");
                    }
                };
            };
            if ON(popup_hide) {
                s.log("menu closed");
            }
        };
        Button..{
            INIT(text = "Menu");
            if ON(pressed) {
                let rect = __builder.node().get_global_rect();
                menu.popup(rect.left_bottom());
            }
        };
    };
}

#[tree(in Container)]
fn settings(s: &mut Demo) {
    GridContainer..{
        INIT(columns = 2, theme_constant("h_separation") = 16);
        Label..{
            INIT(text = "Name");
        };
        LineEdit..{
            INIT(placeholder_text = "Who's there?", max_length = 20);
            if let Some(args) = ON(text_changed) {
                s.name = args[0].to();
            }
            if let Some(args) = ON(text_submitted) {
                s.greeting = Some(format!("Hello, {}!", args[0].to::<String>()));
            }
        };
        Label..{
            INIT(text = "Volume");
        };
        HBoxContainer..{
            HSlider..{
                INIT(max_value = 100, custom_minimum_size = [140.0, 0.0]);
                if let Some(args) = ON(value_changed) {
                    s.volume = args[0].to();
                }
                UPDATE(value = s.volume);
            };
            SpinBox..{
                INIT(max_value = 100, suffix = " %");
                if let Some(args) = ON(value_changed) {
                    s.volume = args[0].to();
                }
                UPDATE(value = s.volume);
            };
        };
        Label..{
            INIT(text = "Level");
        };
        ProgressBar..{
            INIT(custom_minimum_size = [200.0, 0.0]);
            UPDATE(value = s.volume);
        };
        Label..{
            INIT(text = "Fullscreen");
        };
        CheckBox..{
            INIT(text = "On");
            if let Some(args) = ON(toggled) {
                s.fullscreen = args[0].to();
                s.log(format!("fullscreen: {}", s.fullscreen));
            }
            UPDATE(button_pressed = s.fullscreen);
        };
        Label..{
            INIT(text = "Quality");
        };
        OptionButton..{
            INIT(items = ["Low", "Medium", "High"]);
            if let Some(args) = ON(item_selected) {
                s.quality = args[0].to();
            }
            UPDATE(selected = s.quality);
        };
        Label..{
            INIT(text = "Accent");
        };
        HBoxContainer..{
            ColorPickerButton..{
                if let Some(args) = ON(color_changed) {
                    s.accent = args[0].to();
                }
                UPDATE(color = s.accent);
            };
            ColorRect..{
                INIT(custom_minimum_size = [60.0, 18.0]);
                UPDATE(color = s.accent);
            };
        };
    };
    if let Some(greeting) = &s.greeting {
        Label..{
            INIT(theme_color("font_color") = Color32::LIGHT_GREEN);
            UPDATE(text = greeting);
        };
    }
}

#[tree(in Container)]
fn todo_list(s: &mut Demo) {
    let mut remove = None;
    ScrollContainer..{
        INIT(
            custom_maximum_size = [0.0, 120.0],
            size_flags_horizontal = SizeFlags::ExpandFill,
        );
        VBoxContainer..{
            for todo in s.todos.iter_mut() {
                KEY(todo.id);
                HBoxContainer..{
                    CheckBox..{
                        INIT(text = &todo.text);
                        if let Some(args) = ON(toggled) {
                            todo.done = args[0].to();
                        }
                        UPDATE(button_pressed = todo.done);
                    };
                    Button..{
                        INIT(text = "×", flat = true, tooltip_text = "Remove");
                        if ON(pressed) {
                            remove = Some(todo.id);
                        }
                    };
                };
            }
        };
    };
    if let Some(id) = remove {
        s.todos.retain(|t| t.id != id);
    }
    HBoxContainer..{
        LineEdit..{
            INIT(
                placeholder_text = "New todo",
                custom_minimum_size = [160.0, 0.0],
            );
            if let Some(args) = ON(text_changed) {
                s.draft = args[0].to();
            }
            if ON(text_submitted) {
                add_todo(s);
            }
            UPDATE(text = &s.draft);
        };
        Button..{
            INIT(text = "Add");
            UPDATE(disabled = s.draft.trim().is_empty());
            if ON(pressed) {
                add_todo(s);
            }
        };
    };
    Label..{
        UPDATE(
            text = format!(
                "{} of {} done",
                s.todos.iter().filter(|t| t.done).count(),
                s.todos.len()
            ),
        );
    };
}

fn add_todo(s: &mut Demo) {
    let text = s.draft.trim().to_string();
    if !text.is_empty() {
        s.todos.push(Todo {
            id: s.next_id,
            text,
            done: false,
        });
        s.next_id += 1;
        s.draft.clear();
    }
}
