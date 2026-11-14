//! Thin host adapters for priority inheritance and immediate ceiling decisions.

mod arbiter;
mod init;

use magnus::{Error, Integer, Ruby, TryConvert, Value};

/// `value` as an Integer converted to `T`: `TypeError` naming `what` for a
/// non-Integer, `RangeError` with `range` when it does not fit.
fn integer<T: TryConvert>(value: Value, what: &str, range: &str) -> Result<T, Error> {
    let ruby = Ruby::get_with(value);
    if Integer::from_value(value).is_none() {
        return Err(Error::new(
            ruby.exception_type_error(),
            format!("{what} must be Integers"),
        ));
    }
    T::try_convert(value).map_err(|_| {
        Error::new(
            ruby.exception_range_error(),
            format!("{what} must be {range}"),
        )
    })
}

/// A priority or a ceiling: an Integer in `0..2**32`.
#[derive(Clone, Copy)]
pub(crate) struct Priority(pub(crate) u32);

impl TryConvert for Priority {
    fn try_convert(value: Value) -> Result<Self, Error> {
        integer(value, "priorities and ceilings", "in 0..2**32").map(Self)
    }
}

/// A task or resource index: a non-negative Integer that fits a `usize`.
/// An index past the arbiter's tasks or resources is the core's to refuse.
#[derive(Clone, Copy)]
pub(crate) struct Index(pub(crate) usize);

impl TryConvert for Index {
    fn try_convert(value: Value) -> Result<Self, Error> {
        integer(
            value,
            "task and resource indices",
            "non-negative and fit a usize",
        )
        .map(Self)
    }
}
