//! Thin host adapters for priority inheritance and immediate ceiling decisions.

mod arbiter;
mod init;

use magnus::{Error, Integer, Ruby, TryConvert, Value};

#[derive(Clone, Copy)]
pub(crate) struct Count(pub(crate) u32);

impl TryConvert for Count {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            return Err(Error::new(
                Ruby::get_with(value).exception_type_error(),
                "expected an Integer",
            ));
        }
        u32::try_convert(value).map(Self)
    }
}
