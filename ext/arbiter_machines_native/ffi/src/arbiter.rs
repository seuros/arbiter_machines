//! Erase exact task/resource const-generic counts without adding participants.

use crate::Count;
use arbiter_machines::{Acquisition, Arbiter, Protocol};
use magnus::{
    Error, RArray, RHash, RModule, Ruby, Symbol, Value, function, method,
    prelude::*,
    scan_args::{get_kwargs, scan_args},
    typed_data::Obj,
};
use std::cell::RefCell;

trait Decisions: Send {
    fn acquire(&mut self, task: usize, resource: usize) -> Acquisition;
    fn release(&mut self, task: usize, resource: usize) -> bool;
    fn cancel_wait(&mut self, task: usize) -> bool;
    fn effective_priorities(&self) -> Vec<u32>;
}

impl<const T: usize, const R: usize> Decisions for Arbiter<T, R> {
    fn acquire(&mut self, task: usize, resource: usize) -> Acquisition {
        self.acquire(task, resource)
    }
    fn release(&mut self, task: usize, resource: usize) -> bool {
        self.release(task, resource)
    }
    fn cancel_wait(&mut self, task: usize) -> bool {
        self.cancel_wait(task)
    }
    fn effective_priorities(&self) -> Vec<u32> {
        self.effective_priorities().to_vec()
    }
}

fn resources<const T: usize>(
    ruby: &Ruby,
    base: [u32; T],
    ceilings: &[u32],
    protocol: Protocol,
) -> Result<Box<dyn Decisions>, Error> {
    macro_rules! sized {
        ($($n:literal),*) => {
            match ceilings.len() {
                $($n => Ok(Box::new(Arbiter::<T, $n>::new(
                    base,
                    std::array::from_fn(|i| ceilings[i]),
                    protocol,
                )) as Box<dyn Decisions>),)*
                _ => Err(Error::new(ruby.exception_arg_error(), "resource count must be 1..8")),
            }
        };
    }
    sized!(1, 2, 3, 4, 5, 6, 7, 8)
}

#[magnus::wrap(class = "ArbiterMachines::Arbiter", free_immediately, size)]
struct Wrapper {
    core: RefCell<Box<dyn Decisions>>,
    tasks: usize,
    resources: usize,
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (priorities, ceilings, protocol) = get_kwargs::<_, (RArray, RArray, Symbol), (), ()>(
            args.keywords,
            &["priorities", "ceilings", "protocol"],
            &[],
        )?
        .required;
        let protocol = match protocol.name()?.as_ref() {
            "inheritance" => Protocol::Inheritance,
            "immediate_ceiling" => Protocol::ImmediateCeiling,
            _ => {
                return Err(Error::new(
                    ruby.exception_arg_error(),
                    "protocol must be :inheritance or :immediate_ceiling",
                ));
            }
        };
        let base = priorities
            .into_iter()
            .map(|v| Count::try_convert(v).map(|v| v.0))
            .collect::<Result<Vec<_>, _>>()?;
        let ceilings = ceilings
            .into_iter()
            .map(|v| Count::try_convert(v).map(|v| v.0))
            .collect::<Result<Vec<_>, _>>()?;
        macro_rules! sized {
            ($($n:literal),*) => {
                match base.len() {
                    $($n => resources::<$n>(
                        ruby,
                        std::array::from_fn(|i| base[i]),
                        &ceilings,
                        protocol,
                    )?,)*
                    _ => {
                        let message = "task count must be 1..8";
                        return Err(Error::new(ruby.exception_arg_error(), message));
                    }
                }
            };
        }
        let core = sized!(1, 2, 3, 4, 5, 6, 7, 8);
        Ok(Self {
            core: RefCell::new(core),
            tasks: base.len(),
            resources: ceilings.len(),
        })
    }

    fn indices(args: &[Value]) -> Result<(usize, usize), Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (task, resource) =
            get_kwargs::<_, (Count, Count), (), ()>(args.keywords, &["task", "resource"], &[])?
                .required;
        Ok((task.0 as usize, resource.0 as usize))
    }

    fn acquire(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        args: &[Value],
    ) -> Result<(Symbol, Option<usize>), Error> {
        rb_self.check_frozen()?;
        let (task, resource) = Self::indices(args)?;
        let decision = rb_self.core.borrow_mut().acquire(task, resource);
        let (status, blocker) = match decision {
            Acquisition::Granted => ("granted", None),
            Acquisition::AlreadyOwned => ("already_owned", None),
            Acquisition::Blocked(index) => ("blocked", Some(index)),
            Acquisition::Deadlock => ("deadlock", None),
            _ => ("invalid", None),
        };
        // Ruby allocation only after the RefCell borrow has ended.
        Ok((ruby.to_symbol(status), blocker))
    }

    fn release(rb_self: Obj<Self>, args: &[Value]) -> Result<bool, Error> {
        rb_self.check_frozen()?;
        let (task, resource) = Self::indices(args)?;
        let released = rb_self.core.borrow_mut().release(task, resource);
        Ok(released)
    }

    fn cancel_wait(rb_self: Obj<Self>, args: &[Value]) -> Result<bool, Error> {
        rb_self.check_frozen()?;
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (task,) = get_kwargs::<_, (Count,), (), ()>(args.keywords, &["task"], &[])?.required;
        let canceled = rb_self.core.borrow_mut().cancel_wait(task.0 as usize);
        Ok(canceled)
    }

    fn effective_priorities(&self) -> Vec<u32> {
        self.core.borrow().effective_priorities()
    }

    const fn task_count(&self) -> usize {
        self.tasks
    }
    const fn resource_count(&self) -> usize {
        self.resources
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("Arbiter", ruby.class_object())?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_method("acquire", method!(Wrapper::acquire, -1))?;
    class.define_method("release", method!(Wrapper::release, -1))?;
    class.define_method("cancel_wait", method!(Wrapper::cancel_wait, -1))?;
    class.define_method(
        "effective_priorities",
        method!(Wrapper::effective_priorities, 0),
    )?;
    class.define_method("task_count", method!(Wrapper::task_count, 0))?;
    class.define_method("resource_count", method!(Wrapper::resource_count, 0))
}
