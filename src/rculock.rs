use std::cell::UnsafeCell;
use core::sync::atomic::Ordering;
use crate::qsbr::QsbrGuard;
use core::ops::Deref;
use core::sync::atomic::AtomicU32;

//reader writer style lock for use with rcu
//useful for structs that require a mutex for updating,
//but can be traversed within an rcu critical section


pub struct RcuRwLock<T> {
    state: AtomicU32,
    value: UnsafeCell<T>,
}

unsafe impl<T> Sync for RcuRwLock<T> where T: Send + Sync {}

impl<T> RcuRwLock<T> {
    pub const fn new(v: T) -> Self {
        Self {
            state: AtomicU32::new(0),
            value: UnsafeCell::new(v),
        }
    }
    pub fn read<'lock, 'guard>(&'lock self, guard: &'guard QsbrGuard<'_>) -> ReadGuard<'guard, T>
        where 'lock: 'guard
    {
        ReadGuard { lock: self, guard:&guard }
    }
    pub fn write<'lock, 'guard>(&'lock self, _guard: &'guard QsbrGuard<'_>) -> WriteGuard<'guard, T>
        where 'lock: 'guard
    {
        while let Err(s) = self.state.compare_exchange(0, u32::MAX, Ordering::Acquire, Ordering::Relaxed) {
            atomic_wait::wait(&self.state, s);
        }
        WriteGuard{ lock: self }
    }
}
pub struct ReadGuard<'a, T> {
    lock: &'a RcuRwLock<T>,
    guard: &'a QsbrGuard<'a>,
}
pub struct WriteGuard<'a, T> {
    lock: &'a RcuRwLock<T>,
}

impl<'a, T> ReadGuard<'a, T> {
    pub fn write(self) -> WriteGuard<'a, T> {
        self.lock.write(self.guard)
    }
}

impl<T> Deref for WriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe {&*self.lock.value.get() }
    }
}

impl <T> Deref for ReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe {&*self.lock.value.get() }
    }
}

impl<T> Drop for WriteGuard<'_, T> {
    fn drop(&mut self) {
        if self.lock.state.fetch_sub(1, Ordering::Release) == 1 {
            atomic_wait::wake_one(&self.lock.state);
        }
    }
}
