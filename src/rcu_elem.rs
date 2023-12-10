use crate::qsbr::QsbrGuard;
use core::ops::Deref;
use core::sync::atomic::Ordering;
use core::sync::atomic::{AtomicPtr, AtomicU32};

pub struct RcuElem<T> {
    state: AtomicU32,
    value: AtomicPtr<T>,
}

impl<T> RcuElem<T> {
    pub fn new(mut v: T) -> Self {
        Self {
            state: AtomicU32::new(0),
            value: AtomicPtr::new(&mut v),
        }
    }
    pub fn get<'guard>(&self, _guard: &'guard QsbrGuard<'_>) -> &'guard T {
        unsafe { &*self.value.load(Ordering::Relaxed) }
    }
    pub fn lock(&self) -> WriteGuard<'_, T> {
        while let Err(s) = self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        {
            atomic_wait::wait(&self.state, s);
        }
        WriteGuard { lock: self }
    }
}

impl<'a, T> WriteGuard<'a, T> {
    pub fn compare_exchange(&mut self, old: &T, new: &mut T) -> Result<*mut T, *mut T> {
        self.lock.value.compare_exchange(
            old as *const T as *mut T,
            new,
            Ordering::Relaxed,
            Ordering::Relaxed,
        )
    }
    pub fn swap(&mut self, new: &mut T) -> *mut T {
        self.lock.value.swap(new, Ordering::Relaxed)
    }
}

pub struct WriteGuard<'a, T> {
    lock: &'a RcuElem<T>,
}
impl<T> Deref for WriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.load(Ordering::Relaxed) }
    }
}

impl<T> Drop for WriteGuard<'_, T> {
    fn drop(&mut self) {
        if self.lock.state.fetch_sub(1, Ordering::Release) == 1 {
            atomic_wait::wake_one(&self.lock.state);
        }
    }
}
