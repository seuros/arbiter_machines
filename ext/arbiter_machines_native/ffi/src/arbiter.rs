//! Erase exact task/resource const-generic counts without adding participants.

use crate::{Index, Priority};
use arbiter_machines::{Acquisition, Arbiter, Cancelling, Protocol, Refusal};
use magnus::{
    Error, IntoValue, RArray, RClass, RModule, Ruby, Symbol, TryConvert, Value, function, method,
    prelude::*, typed_data::Obj, value::Opaque,
};
use std::any::Any;
use std::cell::RefCell;
use std::sync::OnceLock;

/// The most tasks, and the most resources, an arbiter built from Ruby has:
/// every count in `0..=CAPACITY` is compiled.
const CAPACITY: usize = 8;

trait Decisions: Send + Any {
    /// A copy to decide on: kept only once the answer exists.
    fn boxed(&self) -> Box<dyn Decisions>;
    /// Whether `other` is this very state (same counts, same everything).
    fn same(&self, other: &dyn Decisions) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn acquire(&mut self, task: usize, resource: usize) -> Result<Acquisition, Refusal>;
    fn release(&mut self, task: usize, resource: usize) -> Result<(), Refusal>;
    fn cancel_wait(&mut self, task: usize) -> Result<Cancelling, Refusal>;
    fn effective_priorities(&self) -> Vec<u32>;
    fn owner(&self, resource: usize) -> Option<usize>;
    fn waiting_on(&self, task: usize) -> Option<usize>;
    fn protocol(&self) -> Protocol;
    fn task_count(&self) -> usize;
    fn resource_count(&self) -> usize;
}

impl<const T: usize, const R: usize> Decisions for Arbiter<T, R> {
    fn boxed(&self) -> Box<dyn Decisions> {
        Box::new(self.clone())
    }
    fn same(&self, other: &dyn Decisions) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn acquire(&mut self, task: usize, resource: usize) -> Result<Acquisition, Refusal> {
        self.acquire(task, resource)
    }
    fn release(&mut self, task: usize, resource: usize) -> Result<(), Refusal> {
        self.release(task, resource)
    }
    fn cancel_wait(&mut self, task: usize) -> Result<Cancelling, Refusal> {
        self.cancel_wait(task)
    }
    fn effective_priorities(&self) -> Vec<u32> {
        self.effective_priorities().to_vec()
    }
    fn owner(&self, resource: usize) -> Option<usize> {
        self.owner(resource)
    }
    fn waiting_on(&self, task: usize) -> Option<usize> {
        self.waiting_on(task)
    }
    fn protocol(&self) -> Protocol {
        self.protocol()
    }
    fn task_count(&self) -> usize {
        self.task_count()
    }
    fn resource_count(&self) -> usize {
        self.resource_count()
    }
}

fn range_error(ruby: &Ruby, message: &'static str) -> Error {
    Error::new(ruby.exception_range_error(), message)
}

/// Runs `$body` with `$n` bound to `$len` as a const, for every count in
/// `0..=CAPACITY`; `$over` when `$len` is larger.
macro_rules! by_count {
    ($len:expr, $n:ident => $body:expr, $over:expr) => {
        by_count!(@ $len, $n => $body, $over; 0 1 2 3 4 5 6 7 8)
    };
    (@ $len:expr, $n:ident => $body:expr, $over:expr; $($k:literal)*) => {
        match $len {
            $($k => {
                const $n: usize = $k;
                $body
            })*
            _ => $over,
        }
    };
}

fn sized<const T: usize>(
    ruby: &Ruby,
    base: [u32; T],
    ceilings: &[u32],
    protocol: Protocol,
) -> Result<Box<dyn Decisions>, Error> {
    by_count!(
        ceilings.len(),
        N => Ok(Box::new(Arbiter::<T, N>::new(
            base,
            std::array::from_fn(|i| ceilings[i]),
            protocol,
        )) as Box<dyn Decisions>),
        Err(range_error(ruby, "at most 8 resources (ceilings)"))
    )
}

/// Every element of `value`, an Array, as a priority.
fn priorities(ruby: &Ruby, value: Value) -> Result<Vec<u32>, Error> {
    let Some(array) = RArray::from_value(value) else {
        return Err(Error::new(
            ruby.exception_type_error(),
            "base priorities and ceilings must be Arrays of Integers",
        ));
    };
    array
        .into_iter()
        .map(|v| Priority::try_convert(v).map(|v| v.0))
        .collect()
}

fn protocol_from(ruby: &Ruby, value: Value) -> Result<Protocol, Error> {
    let Some(symbol) = Symbol::from_value(value) else {
        return Err(Error::new(
            ruby.exception_type_error(),
            "protocol must be a Symbol",
        ));
    };
    match symbol.name()?.as_ref() {
        "inheritance" => Ok(Protocol::Inheritance),
        "immediate_ceiling" => Ok(Protocol::ImmediateCeiling),
        _ => Err(range_error(
            ruby,
            "protocol must be :inheritance or :immediate_ceiling",
        )),
    }
}

const fn protocol_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Inheritance => "inheritance",
        Protocol::ImmediateCeiling => "immediate_ceiling",
    }
}

/// Ruby symbol of a refusal: the snake case of the core variant.
fn refusal(ruby: &Ruby, refused: Refusal) -> Value {
    ruby.to_symbol(match refused {
        Refusal::UnknownTask => "unknown_task",
        Refusal::UnknownResource => "unknown_resource",
        Refusal::AboveCeiling => "above_ceiling",
        Refusal::Deadlock => "deadlock",
        Refusal::Unowned => "unowned",
        Refusal::WrongOwner => "wrong_owner",
    })
    .as_value()
}

/// `ArbiterMachines::Blocked`, resolved once at init: swapping the constant
/// later changes nothing the arbiter builds.
static BLOCKED: OnceLock<Opaque<RClass>> = OnceLock::new();

fn blocked_class(ruby: &Ruby) -> Result<RClass, Error> {
    BLOCKED
        .get()
        .map(|class| ruby.get_inner(*class))
        .ok_or_else(|| {
            Error::new(
                ruby.exception_runtime_error(),
                "the arbiter_machines extension is not initialised",
            )
        })
}

/// What `Blocked.new` stores: the resource index, validated as the core's `usize`.
const fn checked_blocked(resource: Index) -> usize {
    resource.0
}

#[magnus::wrap(class = "ArbiterMachines::Arbiter", free_immediately, size)]
struct Wrapper {
    core: RefCell<Box<dyn Decisions>>,
}

impl Wrapper {
    fn new(ruby: &Ruby, base: Value, ceilings: Value, protocol: Value) -> Result<Self, Error> {
        let base = priorities(ruby, base)?;
        let ceilings = priorities(ruby, ceilings)?;
        let protocol = protocol_from(ruby, protocol)?;
        let core = by_count!(
            base.len(),
            N => sized::<N>(ruby, std::array::from_fn(|i| base[i]), &ceilings, protocol)?,
            return Err(range_error(ruby, "at most 8 tasks (base priorities)"))
        );
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    fn acquire(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        task: Index,
        resource: Index,
    ) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        // Everything that can raise happens before the core decides.
        let blocked = blocked_class(ruby)?;
        // Clone-then-keep: the core decides on a copy, under no borrow while
        // Ruby runs, and the copy is stored only once the answer exists.
        let before = rb_self.core.borrow().boxed();
        let mut after = before.boxed();
        let answer = match after.acquire(task.0, resource.0) {
            Ok(Acquisition::Granted) => ruby.to_symbol("granted").as_value(),
            Ok(Acquisition::AlreadyOwned) => ruby.to_symbol("already_owned").as_value(),
            Ok(Acquisition::Blocked(index)) => blocked.funcall("new", (index,))?,
            Err(refused) => return Ok(refusal(ruby, refused)),
        };
        // `Blocked.new` may have frozen or changed the arbiter: then nothing
        // is applied.
        rb_self.check_frozen()?;
        let mut held = rb_self.core.borrow_mut();
        if !held.same(before.as_ref()) {
            return Err(Error::new(
                ruby.exception_runtime_error(),
                "the arbiter changed while the answer was built; the acquire was not applied",
            ));
        }
        *held = after;
        drop(held);
        Ok(answer)
    }

    fn release(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        task: Index,
        resource: Index,
    ) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let released = rb_self.core.borrow_mut().release(task.0, resource.0);
        Ok(match released {
            Ok(()) => ruby.to_symbol("released").as_value(),
            Err(refused) => refusal(ruby, refused),
        })
    }

    fn cancel_wait(ruby: &Ruby, rb_self: Obj<Self>, task: Index) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let cancelled = rb_self.core.borrow_mut().cancel_wait(task.0);
        Ok(match cancelled {
            Ok(Cancelling::Cancelled) => ruby.to_symbol("cancelled").as_value(),
            Ok(Cancelling::NotWaiting) => ruby.to_symbol("not_waiting").as_value(),
            Err(refused) => refusal(ruby, refused),
        })
    }

    fn effective_priorities(&self) -> Vec<u32> {
        self.core.borrow().effective_priorities()
    }

    fn owner(&self, resource: Index) -> Option<usize> {
        self.core.borrow().owner(resource.0)
    }

    fn waiting_on(&self, task: Index) -> Option<usize> {
        self.core.borrow().waiting_on(task.0)
    }

    fn protocol(ruby: &Ruby, rb_self: &Self) -> Value {
        let protocol = rb_self.core.borrow().protocol();
        ruby.to_symbol(protocol_name(protocol))
            .into_value_with(ruby)
    }

    fn task_count(&self) -> usize {
        self.core.borrow().task_count()
    }

    fn resource_count(&self) -> usize {
        self.core.borrow().resource_count()
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let blocked: RClass = module.const_get("Blocked")?;
    // Held for good: removing the constant must not let the GC take it.
    ruby.gc_register_mark_object(blocked);
    BLOCKED.set(blocked.into()).map_err(|_| {
        Error::new(
            ruby.exception_runtime_error(),
            "the arbiter_machines extension was initialised twice",
        )
    })?;
    module.define_singleton_method("__blocked", function!(checked_blocked, 1))?;
    let class = module.define_class("Arbiter", ruby.class_object())?;
    class.const_set("TASK_CAPACITY", CAPACITY)?;
    class.const_set("RESOURCE_CAPACITY", CAPACITY)?;
    class.define_singleton_method("new", function!(Wrapper::new, 3))?;
    class.define_method("acquire", method!(Wrapper::acquire, 2))?;
    class.define_method("release", method!(Wrapper::release, 2))?;
    class.define_method("cancel_wait", method!(Wrapper::cancel_wait, 1))?;
    class.define_method(
        "effective_priorities",
        method!(Wrapper::effective_priorities, 0),
    )?;
    class.define_method("owner", method!(Wrapper::owner, 1))?;
    class.define_method("waiting_on", method!(Wrapper::waiting_on, 1))?;
    class.define_method("protocol", method!(Wrapper::protocol, 0))?;
    class.define_method("task_count", method!(Wrapper::task_count, 0))?;
    class.define_method("resource_count", method!(Wrapper::resource_count, 0))
}
