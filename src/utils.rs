use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) struct Futex<'a> {
    state: &'a AtomicU32,
}

impl Drop for Futex<'_> {
    fn drop(&mut self) {
        self.state.store(0, Ordering::Release);
        atomic_wait::wake_one(self.state);
    }
}

impl<'a> Futex<'a> {
    pub fn lock(state: &'a AtomicU32) -> Self {
        while state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            atomic_wait::wait(state, 1);
        }
        Futex { state }
    }
}
