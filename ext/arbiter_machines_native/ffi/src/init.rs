use magnus::{Error, Ruby};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    crate::arbiter::define(ruby, ruby.define_module("ArbiterMachines")?)
}
