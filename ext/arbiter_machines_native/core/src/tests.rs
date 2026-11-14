extern crate std;

use super::*;
use core::fmt::Debug;

mod model;

/// The call answers `expected` (a refusal or an honoured repeat) and leaves
/// `arbiter` exactly as it was.
fn unchanged<const T: usize, const R: usize, O: Copy + Debug + PartialEq>(
    arbiter: &mut Arbiter<T, R>,
    expected: Result<O, Refusal>,
    call: impl FnOnce(&mut Arbiter<T, R>) -> Result<O, Refusal>,
) {
    let before = arbiter.clone();
    assert_eq!(call(arbiter), expected);
    assert_eq!(*arbiter, before, "{expected:?} changed the arbiter");
}

#[test]
fn inheritance_is_transitive_and_restores_after_release() {
    let mut arbiter = Arbiter::new([1, 2, 9], [9; 2], Protocol::Inheritance);
    assert_eq!(arbiter.acquire(0, 0), Ok(Acquisition::Granted));
    assert_eq!(arbiter.acquire(1, 1), Ok(Acquisition::Granted));
    assert_eq!(arbiter.acquire(0, 1), Ok(Acquisition::Blocked(1)));
    assert_eq!(arbiter.acquire(2, 0), Ok(Acquisition::Blocked(0)));
    assert_eq!(arbiter.effective_priorities(), [9, 9, 9]);
    assert_eq!(
        (arbiter.owner(0), arbiter.waiting_on(2)),
        (Some(0), Some(0))
    );
    assert_eq!(arbiter.release(1, 1), Ok(()));
    assert_eq!(
        arbiter.waiting_on(0),
        None,
        "release clears the waits on it"
    );
    assert_eq!(
        arbiter.owner(1),
        None,
        "release hands the resource to nobody"
    );
    assert_eq!(arbiter.effective_priorities(), [9, 2, 9]);
    assert_eq!(arbiter.cancel_wait(2), Ok(Cancelling::Cancelled));
    assert_eq!(arbiter.effective_priorities(), [1, 2, 9]);
    // A task that asks is running, not waiting: a grant elsewhere drops its wait.
    assert_eq!(arbiter.acquire(2, 0), Ok(Acquisition::Blocked(0)));
    assert_eq!(arbiter.acquire(2, 1), Ok(Acquisition::Granted));
    assert_eq!(arbiter.waiting_on(2), None);
    assert_eq!(arbiter.effective_priorities(), [1, 2, 9]);
}

#[test]
fn every_refusal_changes_nothing() {
    let mut arbiter = Arbiter::new([1, 2], [2; 2], Protocol::Inheritance);
    assert_eq!(arbiter.acquire(0, 0), Ok(Acquisition::Granted));
    assert_eq!(arbiter.acquire(1, 1), Ok(Acquisition::Granted));
    assert_eq!(arbiter.acquire(0, 1), Ok(Acquisition::Blocked(1)));
    unchanged(&mut arbiter, Err(Refusal::Deadlock), |a| a.acquire(1, 0));
    unchanged(&mut arbiter, Err(Refusal::UnknownTask), |a| a.acquire(2, 0));
    unchanged(&mut arbiter, Err(Refusal::UnknownResource), |a| {
        a.acquire(0, 2)
    });
    unchanged(&mut arbiter, Err(Refusal::UnknownTask), |a| a.acquire(2, 2));
    unchanged(&mut arbiter, Err(Refusal::WrongOwner), |a| a.release(0, 1));
    unchanged(&mut arbiter, Err(Refusal::UnknownTask), |a| a.release(2, 0));
    unchanged(&mut arbiter, Err(Refusal::UnknownResource), |a| {
        a.release(0, 2)
    });
    unchanged(&mut arbiter, Err(Refusal::UnknownTask), |a| {
        a.cancel_wait(2)
    });
    assert_eq!(arbiter.release(1, 1), Ok(()));
    // No history is kept, so a repeated release cannot be proven.
    unchanged(&mut arbiter, Err(Refusal::Unowned), |a| a.release(1, 1));
    unchanged(&mut arbiter, Err(Refusal::Unowned), |a| a.release(0, 1));
}

#[test]
fn repeats_are_honoured_and_change_nothing() {
    let mut arbiter = Arbiter::new([1, 2], [2; 2], Protocol::Inheritance);
    assert_eq!(arbiter.acquire(0, 0), Ok(Acquisition::Granted));
    unchanged(&mut arbiter, Ok(Acquisition::AlreadyOwned), |a| {
        a.acquire(0, 0)
    });
    assert_eq!(arbiter.acquire(1, 0), Ok(Acquisition::Blocked(0)));
    unchanged(&mut arbiter, Ok(Acquisition::Blocked(0)), |a| {
        a.acquire(1, 0)
    });
    assert_eq!(arbiter.cancel_wait(1), Ok(Cancelling::Cancelled));
    unchanged(&mut arbiter, Ok(Cancelling::NotWaiting), |a| {
        a.cancel_wait(1)
    });
    unchanged(&mut arbiter, Ok(Cancelling::NotWaiting), |a| {
        a.cancel_wait(0)
    });
}

#[test]
fn ceiling_admission_blocks_even_free_resources() {
    let mut arbiter = Arbiter::new([1, 2], [3, 3], Protocol::ImmediateCeiling);
    assert_eq!(arbiter.acquire(0, 0), Ok(Acquisition::Granted));
    assert_eq!(arbiter.effective_priorities(), [3, 2]);
    assert_eq!(arbiter.acquire(1, 1), Ok(Acquisition::Blocked(0)));
    assert_eq!(arbiter.release(0, 0), Ok(()));
    assert_eq!(arbiter.acquire(1, 1), Ok(Acquisition::Granted));
    unchanged(&mut arbiter, Ok(Acquisition::AlreadyOwned), |a| {
        a.acquire(1, 1)
    });
    let mut misdeclared = Arbiter::new([4], [3], Protocol::ImmediateCeiling);
    unchanged(&mut misdeclared, Err(Refusal::AboveCeiling), |a| {
        a.acquire(0, 0)
    });
}

#[test]
fn readers_and_empty_shapes_are_well_defined() {
    let mut none = Arbiter::<0, 0>::new([], [], Protocol::Inheritance);
    unchanged(&mut none, Err(Refusal::UnknownTask), |a| a.acquire(0, 0));
    unchanged(&mut none, Err(Refusal::UnknownTask), |a| a.release(0, 0));
    unchanged(&mut none, Err(Refusal::UnknownTask), |a| a.cancel_wait(0));
    assert_eq!((none.task_count(), none.resource_count()), (0, 0));
    assert_eq!(none.effective_priorities(), [0_u32; 0]);
    let arbiter = Arbiter::new([1, 2, 3], [9], Protocol::ImmediateCeiling);
    assert_eq!((arbiter.task_count(), arbiter.resource_count()), (3, 1));
    assert_eq!(arbiter.protocol(), Protocol::ImmediateCeiling);
    assert_eq!((arbiter.owner(0), arbiter.owner(1)), (None, None));
    assert_eq!((arbiter.waiting_on(0), arbiter.waiting_on(3)), (None, None));
}

#[test]
fn refusals_describe_themselves() {
    use std::string::ToString;
    let all = [
        Refusal::UnknownTask,
        Refusal::UnknownResource,
        Refusal::AboveCeiling,
        Refusal::Deadlock,
        Refusal::Unowned,
        Refusal::WrongOwner,
    ];
    for (index, refusal) in all.iter().enumerate() {
        let text = refusal.to_string();
        assert_ne!(text, "");
        assert!(all[..index].iter().all(|other| other.to_string() != text));
        let _: &dyn core::error::Error = refusal;
    }
}
