//! Drawing the tree with egui, turning what egui reports into Godot-like events.

use std::mem;

use echo::NodeId;
use egui::{
    Id, Key, Layout, Margin, Order, Response, RichText, Sense, TextWrapMode, Ui,
    scroll_area::ScrollBarVisibility, vec2,
};

use crate::{
    backend::{NodeData, Props},
    elements::Kind,
    value::{ScrollMode, Value},
};

pub(crate) type Event = (NodeId, &'static str, Vec<Value>);

pub(crate) struct Render<'a> {
    pub nodes: &'a mut Vec<Option<NodeData>>,
    pub events: Vec<Event>,
    /// Distinguishes this app's egui ids from another's.
    pub salt: Id,
}

impl Render<'_> {
    fn data(&mut self, id: NodeId) -> &mut NodeData {
        self.nodes[id.0 as usize]
            .as_mut()
            .expect("echo_egui: drawing a removed node")
    }

    fn emit(&mut self, id: NodeId, event: &'static str, args: Vec<Value>) {
        self.events.push((id, event, args));
    }

    /// A stable egui id for a node: echo keeps a node for as long as its element runs, so
    /// egui's state for it (scrolling, focus, an open drop-down) lasts as long too.
    fn egui_id(&self, id: NodeId) -> Id {
        self.salt.with(id.0)
    }

    pub fn children(&mut self, ui: &mut Ui, id: NodeId) {
        let children = self.data(id).children.clone();
        for child in children {
            self.node(ui, child);
        }
    }

    pub fn node(&mut self, ui: &mut Ui, id: NodeId) {
        let data = self.data(id);
        let kind = data.kind;
        // taken out while it's drawn, so widgets can change it (and children can be drawn)
        let mut p = mem::take(&mut data.props);
        if !p.visible {
            self.data(id).props = p;
            return;
        }
        let response = ui
            .scope(|ui| {
                if let Some(m) = p.modulate {
                    ui.multiply_opacity(m.a() as f32 / 255.0);
                }
                self.kind(ui, id, kind, &mut p)
            })
            .inner;
        let tooltip = p.tooltip.clone();
        self.data(id).props = p;

        let Some(mut response) = response else {
            return;
        };
        if let Some(tooltip) = tooltip {
            response = response.on_hover_text(tooltip);
        }
        self.data(id).rect = response.rect;
        let hovered = response.hovered();
        if mem::replace(&mut self.data(id).hovered, hovered) != hovered {
            let event = if hovered {
                "mouse_entered"
            } else {
                "mouse_exited"
            };
            self.emit(id, event, vec![]);
        }
        if response.gained_focus() {
            self.emit(id, "focus_entered", vec![]);
        }
        if response.lost_focus() {
            self.emit(id, "focus_exited", vec![]);
        }
    }

    fn kind(&mut self, ui: &mut Ui, id: NodeId, kind: Kind, p: &mut Props) -> Option<Response> {
        match kind {
            Kind::Node => {
                self.children(ui, id);
                None
            }
            Kind::Control
            | Kind::Container
            | Kind::BoxContainer
            | Kind::BaseButton
            | Kind::Range => Some(self.column(ui, id, p, p.separation)),
            Kind::VBoxContainer => Some(self.column(ui, id, p, p.separation)),
            Kind::HBoxContainer => Some(
                ui.horizontal(|ui| {
                    size(ui, p);
                    if let Some(s) = p.separation {
                        ui.spacing_mut().item_spacing.x = s;
                    }
                    self.children(ui, id);
                })
                .response,
            ),
            Kind::HFlowContainer => Some(
                ui.horizontal_wrapped(|ui| {
                    size(ui, p);
                    let spacing = &mut ui.spacing_mut().item_spacing;
                    spacing.x = p.h_separation.unwrap_or(spacing.x);
                    spacing.y = p.v_separation.unwrap_or(spacing.y);
                    self.children(ui, id);
                })
                .response,
            ),
            Kind::GridContainer => {
                let children = self.data(id).children.clone();
                let spacing = ui.spacing().item_spacing;
                let spacing = vec2(
                    p.h_separation.unwrap_or(spacing.x),
                    p.v_separation.unwrap_or(spacing.y),
                );
                let columns = p.columns;
                Some(
                    egui::Grid::new(self.egui_id(id))
                        .num_columns(columns)
                        .spacing(spacing)
                        .min_col_width(0.0)
                        .show(ui, |ui| {
                            size(ui, p);
                            for (i, child) in children.into_iter().enumerate() {
                                self.node(ui, child);
                                if (i + 1) % columns == 0 {
                                    ui.end_row();
                                }
                            }
                        })
                        .response,
                )
            }
            Kind::MarginContainer => {
                let m = p.margins.map(|v| v.clamp(-128.0, 127.0) as i8);
                let margin = Margin {
                    left: m[0],
                    top: m[1],
                    right: m[2],
                    bottom: m[3],
                };
                Some(
                    egui::Frame::new()
                        .inner_margin(margin)
                        .show(ui, |ui| self.column(ui, id, p, p.separation))
                        .response,
                )
            }
            Kind::PanelContainer => Some(
                egui::Frame::group(ui.style())
                    .show(ui, |ui| self.column(ui, id, p, p.separation))
                    .response,
            ),
            Kind::CenterContainer => Some(
                ui.vertical_centered(|ui| {
                    size(ui, p);
                    self.children(ui, id);
                })
                .response,
            ),
            Kind::ScrollContainer => {
                let mut area = egui::ScrollArea::new([
                    p.h_scroll != ScrollMode::Disabled,
                    p.v_scroll != ScrollMode::Disabled,
                ])
                .id_salt(self.egui_id(id))
                .auto_shrink([!p.h_flags.fills(), !p.v_flags.fills()])
                .scroll_bar_visibility(match p.v_scroll {
                    ScrollMode::AlwaysShow => ScrollBarVisibility::AlwaysVisible,
                    ScrollMode::ShowNever => ScrollBarVisibility::AlwaysHidden,
                    _ => ScrollBarVisibility::VisibleWhenNeeded,
                });
                if let Some(max) = p.max_size {
                    if max.x > 0.0 {
                        area = area.max_width(max.x);
                    }
                    if max.y > 0.0 {
                        area = area.max_height(max.y);
                    }
                }
                if p.min_size.x > 0.0 {
                    area = area.min_scrolled_width(p.min_size.x);
                }
                if p.min_size.y > 0.0 {
                    area = area.min_scrolled_height(p.min_size.y);
                }
                let out = area.show(ui, |ui| {
                    ui.vertical(|ui| self.children(ui, id));
                });
                Some(ui.interact(out.inner_rect, self.egui_id(id), Sense::hover()))
            }
            Kind::FoldableContainer => {
                let folded = p.folded;
                let out = egui::CollapsingHeader::new(text(p, &p.title))
                    .id_salt(self.egui_id(id))
                    .open(Some(!folded))
                    .show(ui, |ui| self.column(ui, id, p, p.separation));
                if out.header_response.clicked() {
                    p.folded = !folded;
                    self.emit(id, "folding_changed", vec![Value::Bool(p.folded)]);
                }
                Some(out.header_response)
            }
            Kind::PopupPanel => {
                let pos = p
                    .position
                    .unwrap_or_else(|| ui.ctx().content_rect().center());
                let out = egui::Area::new(self.egui_id(id))
                    .order(Order::Foreground)
                    .fixed_pos(pos)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.set_min_size(p.min_size);
                            ui.vertical(|ui| self.children(ui, id));
                        })
                    });
                if out.response.clicked_elsewhere() {
                    p.visible = false;
                    self.emit(id, "popup_hide", vec![]);
                }
                Some(out.response)
            }
            Kind::Label => {
                let label = egui::Label::new(text(p, &p.text)).wrap_mode(if p.autowrap {
                    TextWrapMode::Wrap
                } else {
                    TextWrapMode::Extend
                });
                Some(match p.align {
                    Some(align) => {
                        ui.with_layout(Layout::top_down(align), |ui| ui.add(label))
                            .inner
                    }
                    None => ui.add(label),
                })
            }
            Kind::Button => {
                let label = text(p, &p.text);
                let mut button = match (&p.icon, p.text.is_empty()) {
                    (Some(icon), true) => egui::Button::image(icon_image(icon, p)),
                    (Some(icon), false) => egui::Button::image_and_text(icon_image(icon, p), label),
                    (None, _) => egui::Button::new(label),
                };
                button = button
                    .frame(!p.flat)
                    .selected(p.toggle_mode && p.pressed)
                    .min_size(vec2(width(ui, p), p.min_size.y));
                let r = ui.add_enabled(!p.disabled, button);
                self.button_events(id, &r, p);
                Some(r)
            }
            Kind::CheckBox => {
                let mut on = p.pressed;
                let r = ui.add_enabled(!p.disabled, egui::Checkbox::new(&mut on, text(p, &p.text)));
                self.press_events(id, &r);
                if r.changed() {
                    p.pressed = on;
                    self.emit(id, "toggled", vec![Value::Bool(on)]);
                    self.emit(id, "pressed", vec![]);
                }
                Some(r)
            }
            Kind::OptionButton => {
                let mut selected = p.selected;
                let current = usize::try_from(selected)
                    .ok()
                    .and_then(|i| p.items.get(i))
                    .cloned()
                    .unwrap_or_default();
                let mut combo =
                    egui::ComboBox::from_id_salt(self.egui_id(id)).selected_text(text(p, &current));
                let w = width(ui, p);
                if w > 0.0 {
                    combo = combo.width(w);
                }
                let items = &p.items;
                let r = ui
                    .add_enabled_ui(!p.disabled, |ui| {
                        combo
                            .show_ui(ui, |ui| {
                                for (i, item) in items.iter().enumerate() {
                                    let i = i as i64;
                                    if ui.selectable_label(selected == i, item).clicked() {
                                        selected = i;
                                    }
                                }
                            })
                            .response
                    })
                    .inner;
                if selected != p.selected {
                    p.selected = selected;
                    self.emit(id, "item_selected", vec![Value::Int(selected)]);
                }
                Some(r)
            }
            Kind::ColorPickerButton => {
                let mut color = p.color;
                let r = ui
                    .add_enabled_ui(!p.disabled, |ui| ui.color_edit_button_srgba(&mut color))
                    .inner;
                if r.clicked() {
                    self.emit(id, "pressed", vec![]);
                }
                if r.changed() {
                    p.color = color;
                    self.emit(id, "color_changed", vec![Value::Color(color)]);
                }
                Some(r)
            }
            Kind::HSeparator => Some(ui.add(egui::Separator::default().horizontal())),
            Kind::VSeparator => Some(ui.add(egui::Separator::default().vertical())),
            Kind::HSlider => {
                let mut value = p.value;
                let mut slider = egui::Slider::new(&mut value, p.min..=p.max)
                    .show_value(false)
                    .logarithmic(p.exp_edit);
                if p.step > 0.0 {
                    slider = slider.step_by(p.step);
                }
                let w = width(ui, p);
                if w > 0.0 {
                    ui.spacing_mut().slider_width = w;
                }
                let r = ui.add(slider);
                if r.changed() {
                    p.value = value;
                    self.emit(id, "value_changed", vec![Value::Float(value)]);
                }
                if r.drag_stopped() {
                    self.emit(id, "drag_ended", vec![Value::Bool(true)]);
                }
                Some(r)
            }
            Kind::ProgressBar => {
                let span = p.max - p.min;
                let fraction = if span > 0.0 {
                    ((p.value - p.min) / span).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let mut bar = egui::ProgressBar::new(fraction as f32);
                if p.show_percentage {
                    bar = bar.show_percentage();
                }
                let w = width(ui, p);
                if w > 0.0 {
                    bar = bar.desired_width(w);
                }
                Some(ui.add(bar))
            }
            Kind::SpinBox => {
                let mut value = p.value;
                let mut drag = egui::DragValue::new(&mut value)
                    .range(p.min..=p.max)
                    .speed(if p.step > 0.0 { p.step } else { 0.1 })
                    .suffix(p.suffix.clone());
                if p.step >= 1.0 {
                    drag = drag.fixed_decimals(0);
                }
                let r = ui.add(drag);
                if r.changed() {
                    p.value = value;
                    self.emit(id, "value_changed", vec![Value::Float(value)]);
                }
                Some(r)
            }
            Kind::LineEdit => {
                let (w, hint, max, font) = (
                    width(ui, p),
                    p.placeholder.clone(),
                    p.max_length,
                    p.font_size,
                );
                let mut edit = egui::TextEdit::singleline(&mut p.text)
                    .hint_text(hint)
                    .id(self.egui_id(id));
                if let Some(max) = max {
                    edit = edit.char_limit(max);
                }
                if let Some(size) = font {
                    edit = edit.font(egui::FontId::proportional(size));
                }
                if w > 0.0 {
                    edit = edit.desired_width(w);
                }
                let r = ui.add(edit);
                if r.changed() {
                    self.emit(id, "text_changed", vec![Value::Text(p.text.clone())]);
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.emit(id, "text_submitted", vec![Value::Text(p.text.clone())]);
                }
                Some(r)
            }
            Kind::TextEdit => {
                let (w, hint, min_height) = (width(ui, p), p.placeholder.clone(), p.min_size.y);
                let row = ui.text_style_height(&egui::TextStyle::Body);
                let mut edit = egui::TextEdit::multiline(&mut p.text)
                    .hint_text(hint)
                    .id(self.egui_id(id));
                if w > 0.0 {
                    edit = edit.desired_width(w);
                }
                if min_height > 0.0 {
                    edit = edit.desired_rows((min_height / row).max(1.0) as usize);
                }
                let r = ui.add(edit);
                if r.changed() {
                    self.emit(id, "text_changed", vec![]);
                }
                Some(r)
            }
            Kind::TextureRect => {
                let size = vec2(width(ui, p), p.min_size.y);
                Some(match &p.texture {
                    Some(texture) => {
                        let mut image = egui::Image::new(texture.clone());
                        if size.x > 0.0 && size.y > 0.0 {
                            image = image.fit_to_exact_size(size);
                        }
                        if let Some(m) = p.modulate {
                            image = image.tint(m);
                        }
                        ui.add(image)
                    }
                    None => ui.allocate_exact_size(size, Sense::hover()).1,
                })
            }
            Kind::ColorRect => {
                let size = vec2(width(ui, p), p.min_size.y);
                let (rect, r) = ui.allocate_exact_size(size, Sense::hover());
                ui.painter().rect_filled(rect, 0.0, p.color);
                Some(r)
            }
        }
    }

    /// Children in a column, at least the node's minimum size.
    fn column(&mut self, ui: &mut Ui, id: NodeId, p: &Props, separation: Option<f32>) -> Response {
        ui.vertical(|ui| {
            size(ui, p);
            if let Some(s) = separation {
                ui.spacing_mut().item_spacing.y = s;
            }
            self.children(ui, id);
        })
        .response
    }

    /// `pressed`, plus toggling and `toggled(on)` in toggle mode.
    fn button_events(&mut self, id: NodeId, r: &Response, p: &mut Props) {
        self.press_events(id, r);
        if r.clicked() {
            if p.toggle_mode {
                p.pressed = !p.pressed;
                self.emit(id, "toggled", vec![Value::Bool(p.pressed)]);
            }
            self.emit(id, "pressed", vec![]);
        }
    }

    /// `button_down` and `button_up`.
    fn press_events(&mut self, id: NodeId, r: &Response) {
        let down = r.is_pointer_button_down_on();
        if mem::replace(&mut self.data(id).down, down) != down {
            let event = if down { "button_down" } else { "button_up" };
            self.emit(id, event, vec![]);
        }
    }
}

/// The node's minimum size, and filling the width (or height) it's given.
fn size(ui: &mut Ui, p: &Props) {
    ui.set_min_size(p.min_size);
    if p.h_flags.fills() {
        ui.set_min_width(ui.available_width());
    }
    if p.v_flags.fills() && ui.available_height().is_finite() {
        ui.set_min_height(ui.available_height());
    }
}

/// The width a widget should take: what's left if it fills, else its minimum (0 = natural).
fn width(ui: &Ui, p: &Props) -> f32 {
    if p.h_flags.fills() {
        ui.available_width()
    } else {
        p.min_size.x
    }
}

fn text(p: &Props, text: &str) -> RichText {
    let mut rich = RichText::new(text);
    if let Some(size) = p.font_size {
        rich = rich.size(size);
    }
    if let Some(color) = p.font_color {
        rich = rich.color(color);
    }
    rich
}

fn icon_image<'a>(icon: &egui::ImageSource<'a>, p: &Props) -> egui::Image<'a> {
    egui::Image::new(icon.clone()).max_height(p.font_size.unwrap_or(14.0) + 4.0)
}
