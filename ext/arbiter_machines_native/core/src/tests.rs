use super::*;
#[test]
fn inheritance_is_transitive_and_restores_after_release() {
    let mut arbiter = Arbiter::new([1, 2, 9], [9; 2], Protocol::Inheritance);
    assert_eq!(arbiter.acquire(0, 0), Acquisition::Granted);
    assert_eq!(arbiter.acquire(1, 1), Acquisition::Granted);
    assert_eq!(arbiter.acquire(0, 1), Acquisition::Blocked(1));
    assert_eq!(arbiter.acquire(2, 0), Acquisition::Blocked(0));
    assert_eq!(arbiter.effective_priorities(), [9, 9, 9]);
    assert!(arbiter.release(1, 1));
    assert_eq!(arbiter.effective_priorities(), [9, 2, 9]);
    assert!(arbiter.cancel_wait(2));
    assert_eq!(arbiter.effective_priorities(), [1, 2, 9]);
}
#[test]
fn prospective_deadlocks_do_not_insert_cycles() {
    let mut arbiter = Arbiter::new([1, 2], [2; 2], Protocol::Inheritance);
    assert_eq!(arbiter.acquire(0, 0), Acquisition::Granted);
    assert_eq!(arbiter.acquire(1, 1), Acquisition::Granted);
    assert_eq!(arbiter.acquire(0, 1), Acquisition::Blocked(1));
    let before = arbiter.clone();
    assert_eq!(arbiter.acquire(1, 0), Acquisition::Deadlock);
    assert_eq!(arbiter, before);
    assert!(!arbiter.release(0, 1));
    assert_eq!(arbiter.acquire(2, 0), Acquisition::Invalid);
}
#[test]
fn ceiling_admission_blocks_even_free_resources() {
    let mut arbiter = Arbiter::new([1, 2], [3, 3], Protocol::ImmediateCeiling);
    assert_eq!(arbiter.acquire(0, 0), Acquisition::Granted);
    assert_eq!(arbiter.effective_priorities(), [3, 2]);
    assert_eq!(arbiter.acquire(1, 1), Acquisition::Blocked(0));
    assert!(arbiter.release(0, 0));
    assert_eq!(arbiter.acquire(1, 1), Acquisition::Granted);
    assert_eq!(arbiter.acquire(1, 1), Acquisition::AlreadyOwned);
    let mut invalid = Arbiter::new([4], [3], Protocol::ImmediateCeiling);
    assert_eq!(invalid.acquire(0, 0), Acquisition::Invalid);
}
