//! Value conversion between the engines' parameter systems.
//!
//! The editing engine's [`ParamValue`] and the compositing engine's
//! [`Value`] cover different ground; everything both share converts here, in
//! one place, so no other code depends on the details. What has no equivalent
//! yet converts to `None` and is reported by the caller, never silently
//! dropped into a default.

use effectcraft_keyframe::Value;
use filmcraft_project::keyframe::ParamValue;

/// Convert a parameter value of the editing engine into the compositing
/// engine's value type. `None` for the kinds that are not bridged yet
/// (`Text`, `Curve`, `Path`): the effect keeps its default for that parameter.
pub fn param_to_value(p: &ParamValue) -> Option<Value> {
    match p {
        ParamValue::Float(v) => Some(Value::Scalar(*v)),
        ParamValue::Vec2(v) => Some(Value::Vec2([v.x, v.y])),
        ParamValue::Color(c) => Some(Value::Color([f64::from(c[0]), f64::from(c[1]), f64::from(c[2]), f64::from(c[3])])),
        ParamValue::Bool(b) => Some(Value::Bool(*b)),
        ParamValue::Choice(c) => Some(Value::Enum(*c)),
        ParamValue::Text(_) | ParamValue::Curve(_) | ParamValue::Path(_) => None,
    }
}
