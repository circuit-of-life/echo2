//! Event arguments and the property types that aren't egui's own.

use std::fmt;

use egui::Color32;

/// An event argument (`args[0]` in `if let Some(args) = ON(value_changed)`).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Color(Color32),
}

impl Value {
    /// Converts to `T` (panicking if it's another kind of value): `args[0].to::<f64>()`.
    pub fn to<T: FromValue>(&self) -> T {
        T::from_value(self)
            .unwrap_or_else(|| panic!("echo_egui: {self:?} isn't a {}", std::any::type_name::<T>()))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(v) => v.fmt(f),
            Value::Int(v) => v.fmt(f),
            Value::Float(v) => v.fmt(f),
            Value::Text(v) => v.fmt(f),
            Value::Color(v) => write!(f, "{v:?}"),
        }
    }
}

/// What [`Value::to`] converts to.
pub trait FromValue: Sized {
    fn from_value(v: &Value) -> Option<Self>;
}

impl FromValue for bool {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
}
impl FromValue for i64 {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }
}
impl FromValue for f64 {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }
}
impl FromValue for String {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Text(v) => Some(v.clone()),
            _ => None,
        }
    }
}
impl FromValue for Color32 {
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Color(v) => Some(*v),
            _ => None,
        }
    }
}

/// How a control uses the space its container gives it along one axis (Godot's size flags).
///
/// egui has no flexbox: `Fill`/`ExpandFill` make a control as wide (or tall) as what's left,
/// which works for the last control in a row, or anything in a column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SizeFlags {
    #[default]
    ShrinkBegin,
    ShrinkCenter,
    ShrinkEnd,
    Fill,
    Expand,
    ExpandFill,
}

impl SizeFlags {
    pub(crate) fn fills(self) -> bool {
        matches!(self, SizeFlags::Fill | SizeFlags::ExpandFill)
    }
}

/// A [`ScrollContainer`](crate::ScrollContainer)'s scrolling along one axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollMode {
    Disabled,
    #[default]
    Auto,
    AlwaysShow,
    ShowNever,
}
