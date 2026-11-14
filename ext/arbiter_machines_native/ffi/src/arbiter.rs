//! Erase exact task/resource const-generic counts without adding participants.

use arbiter_machines::{Acquisition, Arbiter, Cancelling, Protocol, Refusal};
use magnus::{
    Error, IntoValue, RModule, Ruby, Value, function, method, prelude::*, typed_data::Obj,
};
use matryona_machines::ruby::{Class, Registry, Symbols, exact_array, integer, keep, symbol};
use std::any::Any;
use std::cell::RefCell;

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

/// An arbiter of any compiled counts, as clone-then-keep holds it: a copy to
/// decide on, and equality with the state it was copied from.
struct Core(Box<dyn Decisions>);

impl Clone for Core {
    fn clone(&self) -> Self {
        Self(self.0.boxed())
    }
}

impl PartialEq for Core {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_ref())
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

/// Every element of `value`, exactly an Array (no subclass), as a priority;
/// `name` is the argument.
fn priorities(ruby: &Ruby, value: Value, name: &'static str) -> Result<Vec<u32>, Error> {
    let message = if name == "base" {
        "base must be an Array of Integers"
    } else {
        "ceilings must be an Array of Integers"
    };
    exact_array(ruby, value, message)?
        .into_iter()
        .map(|v| integer(v, name))
        .collect()
}

matryona_machines::symbols! {
    /// The core's protocols as Symbols.
    Protocols for Protocol as "protocol" {
        Inheritance => "inheritance",
        ImmediateCeiling => "immediate_ceiling",
    }
}

matryona_machines::symbols! {
    /// The core's refusals as Symbols: the snake case of each variant.
    Refusals for Refusal as "a refusal" {
        UnknownTask => "unknown_task",
        UnknownResource => "unknown_resource",
        AboveCeiling => "above_ceiling",
        Deadlock => "deadlock",
        Unowned => "unowned",
        WrongOwner => "wrong_owner",
    }
}

matryona_machines::symbols! {
    /// The core's honoured `cancel_wait` answers as Symbols.
    Cancellings for Cancelling as "a cancellation" {
        Cancelled => "cancelled",
        NotWaiting => "not_waiting",
    }
}

/// Ruby symbol of a refusal: the snake case of the core variant.
fn refusal(ruby: &Ruby, refused: Refusal) -> Value {
    Refusals::symbol(ruby, refused).as_value()
}

/// `ArbiterMachines::Blocked`, resolved once at init: swapping the constant
/// later changes nothing the arbiter builds.
struct Classes {
    blocked: Class,
}

static CLASSES: Registry<Classes> = Registry::new("arbiter_machines");

/// What `Blocked.new` stores: the resource index, validated as the core's `usize`.
fn checked_blocked(resource: Value) -> Result<usize, Error> {
    integer(resource, "resource")
}

#[magnus::wrap(class = "ArbiterMachines::Arbiter", free_immediately, size)]
struct Wrapper {
    core: RefCell<Core>,
}

impl Wrapper {
    fn new(ruby: &Ruby, base: Value, ceilings: Value, protocol: Value) -> Result<Self, Error> {
        let base = priorities(ruby, base, "base")?;
        let ceilings = priorities(ruby, ceilings, "ceilings")?;
        let protocol = Protocols::parse(ruby, protocol)?;
        let core = by_count!(
            base.len(),
            N => sized::<N>(ruby, std::array::from_fn(|i| base[i]), &ceilings, protocol)?,
            return Err(range_error(ruby, "at most 8 tasks (base priorities)"))
        );
        Ok(Self {
            core: RefCell::new(Core(core)),
        })
    }

    fn acquire(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        task: Value,
        resource: Value,
    ) -> Result<Value, Error> {
        // Frozen first, then the arguments: everything that can raise
        // happens before the core decides.
        rb_self.check_frozen()?;
        let (task, resource) = (integer(task, "task")?, integer(resource, "resource")?);
        let blocked = CLASSES.get(ruby)?.blocked.get(ruby);
        // Clone-then-keep: the core decides on a copy, under no borrow while
        // Ruby runs, and the copy is stored only once the answer exists.
        keep(
            ruby,
            rb_self,
            &rb_self.core,
            |core| core.0.acquire(task, resource),
            |acquired| match acquired {
                Ok(Acquisition::Granted) => Ok(symbol(ruby, "granted").as_value()),
                Ok(Acquisition::AlreadyOwned) => Ok(symbol(ruby, "already_owned").as_value()),
                Ok(Acquisition::Blocked(index)) => blocked.funcall("new", (index,)),
                Err(refused) => Ok(refusal(ruby, refused)),
            },
        )
    }

    fn release(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        task: Value,
        resource: Value,
    ) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let (task, resource) = (integer(task, "task")?, integer(resource, "resource")?);
        let released = rb_self.core.borrow_mut().0.release(task, resource);
        Ok(match released {
            Ok(()) => symbol(ruby, "released").as_value(),
            Err(refused) => refusal(ruby, refused),
        })
    }

    fn cancel_wait(ruby: &Ruby, rb_self: Obj<Self>, task: Value) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let task = integer(task, "task")?;
        let cancelled = rb_self.core.borrow_mut().0.cancel_wait(task);
        Ok(match cancelled {
            Ok(cancelling) => Cancellings::symbol(ruby, cancelling).as_value(),
            Err(refused) => refusal(ruby, refused),
        })
    }

    fn effective_priorities(&self) -> Vec<u32> {
        self.core.borrow().0.effective_priorities()
    }

    fn owner(&self, resource: Value) -> Result<Option<usize>, Error> {
        let resource = integer(resource, "resource")?;
        Ok(self.core.borrow().0.owner(resource))
    }

    fn waiting_on(&self, task: Value) -> Result<Option<usize>, Error> {
        let task = integer(task, "task")?;
        Ok(self.core.borrow().0.waiting_on(task))
    }

    fn protocol(ruby: &Ruby, rb_self: &Self) -> Value {
        let protocol = rb_self.core.borrow().0.protocol();
        Protocols::symbol(ruby, protocol).into_value_with(ruby)
    }

    fn task_count(&self) -> usize {
        self.core.borrow().0.task_count()
    }

    fn resource_count(&self) -> usize {
        self.core.borrow().0.resource_count()
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    // Held for good: removing the constant must not let the GC take it.
    let blocked = Class::resolve(ruby, module, "Blocked")?;
    CLASSES.set(ruby, Classes { blocked })?;
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
