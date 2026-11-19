//! The node types: Godot's Control classes that have an egui counterpart, with Godot's class
//! hierarchy as [`Is`] relations (so `#[tree(in Container)]` and `Body<BoxContainer, ..>` work).

use std::rc::Rc;

use echo::{Create, Element, Is, NodeId};

use crate::{Egui, Handle};

macro_rules! elements {
    ($($(#[$doc:meta])* $name:ident $(: $($base:ident),+)?;)*) => {
        /// What a node is, for drawing it.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum Kind {
            $($name,)*
        }

        impl Kind {
            pub(crate) fn name(self) -> &'static str {
                match self {
                    $(Kind::$name => stringify!($name),)*
                }
            }
        }

        $(
            $(#[$doc])*
            pub enum $name {}

            impl Element for $name {
                type Backend = Egui;
                type Handle = Handle<$name>;
                fn handle(egui: &Rc<Egui>, node: NodeId) -> Handle<$name> {
                    Handle::new(egui.clone(), node)
                }
            }

            impl Create for $name {
                fn create(egui: &Rc<Egui>) -> NodeId {
                    egui.create(Kind::$name)
                }
            }

            $($(impl Is<$base> for $name {})+)?
        )*
    };
}

elements! {
    /// A plain node: its children are drawn one after another, as if it wasn't there.
    Node;
    /// A plain control: its children are drawn in a column, at least `custom_minimum_size` big.
    Control: Node;
    Label: Control, Node;
    /// Common to buttons: `disabled`, `button_pressed`, `toggle_mode`; fires `pressed`,
    /// `button_down`, `button_up`, `toggled(on)`.
    BaseButton: Control, Node;
    /// `text`, `icon`, `flat`.
    Button: BaseButton, Control, Node;
    /// Fires `toggled(on)` and `pressed`.
    CheckBox: Button, BaseButton, Control, Node;
    /// A drop-down of `items`; fires `item_selected(index)`.
    OptionButton: Button, BaseButton, Control, Node;
    /// Fires `color_changed(color)`.
    ColorPickerButton: Button, BaseButton, Control, Node;
    Container: Control, Node;
    BoxContainer: Container, Control, Node;
    /// A row; `theme_constant("separation")` sets the spacing.
    HBoxContainer: BoxContainer, Container, Control, Node;
    /// A column; `theme_constant("separation")` sets the spacing.
    VBoxContainer: BoxContainer, Container, Control, Node;
    /// A row that wraps; `theme_constant("h_separation" / "v_separation")`.
    HFlowContainer: Container, Control, Node;
    /// `columns` columns, filled row by row.
    GridContainer: Container, Control, Node;
    /// `theme_constant("margin_left" / "margin_top" / "margin_right" / "margin_bottom")`.
    MarginContainer: Container, Control, Node;
    /// A framed group.
    PanelContainer: Container, Control, Node;
    CenterContainer: Container, Control, Node;
    /// `horizontal_scroll_mode`, `vertical_scroll_mode`; `custom_maximum_size` caps its size.
    ScrollContainer: Container, Control, Node;
    /// A collapsing section with a `title`; `folded`; fires `folding_changed(folded)`.
    FoldableContainer: Container, Control, Node;
    HSeparator: Control, Node;
    VSeparator: Control, Node;
    /// Common to ranges: `value`, `min_value`, `max_value`, `step`.
    Range: Control, Node;
    /// Fires `value_changed(value)` and `drag_ended(true)`.
    HSlider: Range, Control, Node;
    /// `show_percentage`.
    ProgressBar: Range, Control, Node;
    /// `suffix`; fires `value_changed(value)`.
    SpinBox: Range, Control, Node;
    /// `placeholder_text`, `max_length`; fires `text_changed(text)`, `text_submitted(text)`.
    LineEdit: Control, Node;
    /// Multi-line; fires `text_changed` (no arguments, as in Godot: read `get_text()`).
    TextEdit: Control, Node;
    /// Shows `texture` (any egui `ImageSource`).
    TextureRect: Control, Node;
    /// A rectangle of `color`.
    ColorRect: Control, Node;
    /// A panel floating above everything else while `visible`, at its `position` (see
    /// `Handle::popup`); clicking elsewhere hides it and fires `popup_hide`.
    PopupPanel: Node;
}
