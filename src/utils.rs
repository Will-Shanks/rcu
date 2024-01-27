use std::sync::atomic::{AtomicU32, Ordering};

pub trait Lock<'a> {
    type Guard;
    fn lock(&'a self) -> Self::Guard;
    fn new() -> Self;
}

#[derive(Debug)]
pub struct Futex {
    state: AtomicU32,
}

pub struct FutexGuard<'a> {
    futex: &'a Futex,
}

impl Drop for FutexGuard<'_> {
    fn drop(&mut self) {
        self.futex.state.store(0, Ordering::Release);
        atomic_wait::wake_one(&self.futex.state);
    }
}

impl<'a> Lock<'a> for Futex {
    type Guard = FutexGuard<'a>;
    fn new() -> Self {
        Futex {
            state: AtomicU32::new(0),
        }
    }

    fn lock(&'a self) -> Self::Guard {
        while self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            atomic_wait::wait(&self.state, 1);
        }
        Self::Guard { futex: self }
    }
}

#[derive(Debug)]
pub struct SpinLock {
    state: AtomicU32,
}

pub struct SpinLockGuard<'a> {
    spin: &'a SpinLock,
}

impl Drop for SpinLockGuard<'_> {
    fn drop(&mut self) {
        self.spin.state.store(0, Ordering::Release);
    }
}

impl<'a> Lock<'a> for SpinLock {
    type Guard = SpinLockGuard<'a>;
    fn new() -> Self {
        SpinLock {
            state: AtomicU32::new(0),
        }
    }
    fn lock(&'a self) -> Self::Guard {
        while self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
        SpinLockGuard { spin: self }
    }
}
