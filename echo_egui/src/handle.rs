use std::{fmt, marker::PhantomData, rc::Rc};

use echo::{Is, NodeId};
use egui::{Align, Color32, ImageSource, Pos2, Vec2};

use crate::{
    Egui,
    backend::Props,
    elements::*,
    value::{ScrollMode, SizeFlags},
};

/// A node: what `node()` and `let x = Label..{}` give. Its setters are the properties
/// `INIT(..)`/`UPDATE(..)` can set; each node type has the ones of its Godot class and base
/// classes that mean something in egui.
pub struct Handle<T> {
    egui: Rc<Egui>,
    id: NodeId,
    _t: PhantomData<fn() -> T>,
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Handle::new(self.egui.clone(), self.id)
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.egui, &other.egui) && self.id == other.id
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.alive() {
            write!(f, "{}#{}", self.kind_name(), self.id.0)
        } else {
            write!(f, "removed#{}", self.id.0)
        }
    }
}

impl<T> Handle<T> {
    pub(crate) fn new(egui: Rc<Egui>, id: NodeId) -> Self {
        Handle {
            egui,
            id,
            _t: PhantomData,
        }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The node type's name: `"Label"`.
    pub fn kind_name(&self) -> &'static str {
        self.egui.with(self.id, |n| n.kind.name())
    }

    pub fn alive(&self) -> bool {
        echo::Backend::alive(&*self.egui, self.id)
    }

    pub fn children(&self) -> Vec<Handle<Node>> {
        self.egui
            .with(self.id, |n| n.children.clone())
            .into_iter()
            .map(|c| Handle::new(self.egui.clone(), c))
            .collect()
    }

    /// The `i`th child (panics if there's none).
    pub fn child(&self, i: usize) -> Handle<Node> {
        let children = self.children();
        children
            .get(i)
            .cloned()
            .unwrap_or_else(|| panic!("echo_egui: {self:?} has no child #{i}"))
    }

    /// The same node as a `U` (not checked).
    pub fn cast<U>(self) -> Handle<U> {
        Handle::new(self.egui, self.id)
    }

    fn props<R>(&self, f: impl FnOnce(&mut Props) -> R) -> R {
        self.egui.with(self.id, |n| f(&mut n.props))
    }
}

impl<T: Is<Control>> Handle<T> {
    pub fn set_visible(&self, visible: bool) {
        self.props(|p| p.visible = visible);
    }
    pub fn is_visible(&self) -> bool {
        self.props(|p| p.visible)
    }
    /// Where it was drawn last frame, in egui's coordinates (nothing before it's drawn).
    pub fn get_global_rect(&self) -> egui::Rect {
        self.egui.with(self.id, |n| n.rect)
    }
    /// Its alpha fades the node (and what's in it); labels and textures are also tinted.
    pub fn set_modulate(&self, color: Color32) {
        self.props(|p| p.modulate = Some(color));
    }
    pub fn set_custom_minimum_size(&self, size: impl Into<Vec2>) {
        let size = size.into();
        self.props(|p| p.min_size = size);
    }
    /// (Only scroll containers use it, to cap how big they get.)
    pub fn set_custom_maximum_size(&self, size: impl Into<Vec2>) {
        let size = size.into();
        self.props(|p| p.max_size = Some(size));
    }
    pub fn set_size_flags_horizontal(&self, flags: SizeFlags) {
        self.props(|p| p.h_flags = flags);
    }
    pub fn set_size_flags_vertical(&self, flags: SizeFlags) {
        self.props(|p| p.v_flags = flags);
    }
    pub fn set_tooltip_text(&self, text: impl fmt::Display) {
        let text = text.to_string();
        self.props(|p| p.tooltip = (!text.is_empty()).then_some(text));
    }
    /// `"font_size"` sets the size of its text. (Other names are ignored.)
    pub fn set_theme_font_size(&self, name: &str, size: f32) {
        if name == "font_size" {
            self.props(|p| p.font_size = Some(size));
        }
    }
    /// `"font_color"` sets the color of its text. (Other names are ignored.)
    pub fn set_theme_color(&self, name: &str, color: Color32) {
        if name == "font_color" {
            self.props(|p| p.font_color = Some(color));
        }
    }
    /// `"separation"` (boxes), `"h_separation"`/`"v_separation"` (flow and grid containers),
    /// `"margin_left"`/`"margin_top"`/`"margin_right"`/`"margin_bottom"` (margin containers).
    /// (Other names are ignored.)
    pub fn set_theme_constant(&self, name: &str, value: i32) {
        let v = value as f32;
        self.props(|p| match name {
            "separation" => p.separation = Some(v),
            "h_separation" => p.h_separation = Some(v),
            "v_separation" => p.v_separation = Some(v),
            "margin_left" => p.margins[0] = v,
            "margin_top" => p.margins[1] = v,
            "margin_right" => p.margins[2] = v,
            "margin_bottom" => p.margins[3] = v,
            _ => {}
        });
    }
}

/// Node types with a `text`.
pub trait HasText {}
impl HasText for Label {}
impl HasText for Button {}
impl HasText for CheckBox {}
impl HasText for OptionButton {}
impl HasText for ColorPickerButton {}
impl HasText for LineEdit {}
impl HasText for TextEdit {}

impl<T: HasText> Handle<T> {
    pub fn set_text(&self, text: impl fmt::Display) {
        let text = text.to_string();
        self.props(|p| p.text = text);
    }
    pub fn get_text(&self) -> String {
        self.props(|p| p.text.clone())
    }
}

impl Handle<Label> {
    pub fn set_horizontal_alignment(&self, align: Align) {
        self.props(|p| p.align = Some(align));
    }
    /// Whether long text wraps (Godot's `autowrap_mode`, as on or off).
    pub fn set_autowrap_mode(&self, wrap: bool) {
        self.props(|p| p.autowrap = wrap);
    }
}

impl<T: Is<BaseButton>> Handle<T> {
    pub fn set_disabled(&self, disabled: bool) {
        self.props(|p| p.disabled = disabled);
    }
    /// Whether a toggle button or check box is on.
    pub fn set_button_pressed(&self, pressed: bool) {
        self.props(|p| p.pressed = pressed);
    }
    pub fn is_pressed(&self) -> bool {
        self.props(|p| p.pressed)
    }
    pub fn set_toggle_mode(&self, toggle: bool) {
        self.props(|p| p.toggle_mode = toggle);
    }
}

impl Handle<Button> {
    /// Without a frame until hovered.
    pub fn set_flat(&self, flat: bool) {
        self.props(|p| p.flat = flat);
    }
    pub fn set_icon(&self, icon: impl Into<ImageSource<'static>>) {
        let icon = icon.into();
        self.props(|p| p.icon = Some(icon));
    }
}

impl<T: Is<Range>> Handle<T> {
    pub fn set_value(&self, value: impl Into<f64>) {
        let value = value.into();
        self.props(|p| p.value = value.clamp(p.min, p.max.max(p.min)));
    }
    pub fn get_value(&self) -> f64 {
        self.props(|p| p.value)
    }
    pub fn set_min_value(&self, min: impl Into<f64>) {
        let min = min.into();
        self.props(|p| p.min = min);
    }
    pub fn set_max_value(&self, max: impl Into<f64>) {
        let max = max.into();
        self.props(|p| p.max = max);
    }
    /// 0 for no step.
    pub fn set_step(&self, step: impl Into<f64>) {
        let step = step.into();
        self.props(|p| p.step = step);
    }
}

impl Handle<HSlider> {
    /// A logarithmic slider.
    pub fn set_exp_edit(&self, exp: bool) {
        self.props(|p| p.exp_edit = exp);
    }
}

impl Handle<ProgressBar> {
    pub fn set_show_percentage(&self, show: bool) {
        self.props(|p| p.show_percentage = show);
    }
}

impl Handle<SpinBox> {
    pub fn set_suffix(&self, suffix: impl fmt::Display) {
        let suffix = suffix.to_string();
        self.props(|p| p.suffix = suffix);
    }
}

impl Handle<LineEdit> {
    pub fn set_placeholder_text(&self, text: impl fmt::Display) {
        let text = text.to_string();
        self.props(|p| p.placeholder = text);
    }
    pub fn set_max_length(&self, max: usize) {
        self.props(|p| p.max_length = (max > 0).then_some(max));
    }
}

impl Handle<TextEdit> {
    pub fn set_placeholder_text(&self, text: impl fmt::Display) {
        let text = text.to_string();
        self.props(|p| p.placeholder = text);
    }
}

impl Handle<GridContainer> {
    pub fn set_columns(&self, columns: usize) {
        self.props(|p| p.columns = columns.max(1));
    }
}

impl Handle<FoldableContainer> {
    pub fn set_title(&self, title: impl fmt::Display) {
        let title = title.to_string();
        self.props(|p| p.title = title);
    }
    pub fn set_folded(&self, folded: bool) {
        self.props(|p| p.folded = folded);
    }
    pub fn is_folded(&self) -> bool {
        self.props(|p| p.folded)
    }
}

impl Handle<ScrollContainer> {
    pub fn set_horizontal_scroll_mode(&self, mode: ScrollMode) {
        self.props(|p| p.h_scroll = mode);
    }
    pub fn set_vertical_scroll_mode(&self, mode: ScrollMode) {
        self.props(|p| p.v_scroll = mode);
    }
}

/// Node types with a `color`.
pub trait HasColor {}
impl HasColor for ColorRect {}
impl HasColor for ColorPickerButton {}

impl<T: HasColor> Handle<T> {
    pub fn set_color(&self, color: Color32) {
        self.props(|p| p.color = color);
    }
    pub fn get_color(&self) -> Color32 {
        self.props(|p| p.color)
    }
}

impl Handle<TextureRect> {
    pub fn set_texture(&self, texture: impl Into<ImageSource<'static>>) {
        let texture = texture.into();
        self.props(|p| p.texture = Some(texture));
    }
}

impl Handle<OptionButton> {
    pub fn add_item(&self, text: impl fmt::Display) {
        let text = text.to_string();
        self.props(|p| p.items.push(text));
    }
    pub fn clear(&self) {
        self.props(|p| {
            p.items.clear();
            p.selected = -1;
        });
    }
    /// Replaces the items (keeping the selection if it's still in range).
    pub fn set_items<S: fmt::Display>(&self, items: impl IntoIterator<Item = S>) {
        let items: Vec<String> = items.into_iter().map(|s| s.to_string()).collect();
        self.props(|p| {
            if p.selected >= items.len() as i64 {
                p.selected = -1;
            }
            p.items = items;
        });
    }
    /// The selected item's index (-1 for none).
    pub fn set_selected(&self, index: i64) {
        self.props(|p| p.selected = index);
    }
    pub fn get_selected(&self) -> i64 {
        self.props(|p| p.selected)
    }
}

impl Handle<PopupPanel> {
    pub fn set_visible(&self, visible: bool) {
        self.props(|p| p.visible = visible);
    }
    pub fn is_visible(&self) -> bool {
        self.props(|p| p.visible)
    }
    /// Its minimum size.
    pub fn set_size(&self, size: impl Into<Vec2>) {
        let size = size.into();
        self.props(|p| p.min_size = size);
    }
    pub fn set_position(&self, position: impl Into<Pos2>) {
        let position = position.into();
        self.props(|p| p.position = Some(position));
    }
    /// Shows it at `position` (its top left corner).
    pub fn popup(&self, position: impl Into<Pos2>) {
        self.set_position(position);
        self.set_visible(true);
    }
    pub fn hide(&self) {
        self.set_visible(false);
    }
}
