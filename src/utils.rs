use zbus::zvariant::{Str, Value};

#[must_use]
/// Converts a [`Value`] into [`Str`], or return [`None`] if it's not `str`.
pub const fn extract_str<'a, 'b>(v: &'a Value<'b>) -> Option<&'a Str<'b>> {
    if let Value::Str(v) = v {
        Some(v)
    } else {
        None
    }
}
