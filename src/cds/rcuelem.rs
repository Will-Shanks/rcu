use crate::qsbr::QsbrGuard;
use crate::utils::Futex;
use crate::utils::Lock;
use std::ops::Deref;
use std::ptr::null_mut;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering;

pub struct RcuElem<T> {
    lock: Futex,
    value: AtomicPtr<T>,
}

impl<T> RcuElem<T> {
    pub fn new(mut v: T) -> Self {
        Self {
            lock: Futex::new(),
            value: AtomicPtr::new(&mut v),
        }
    }
    pub fn get<'guard, L>(&self, _guard: &'guard QsbrGuard<'_, L>) -> &'guard T
    where
        L: for<'a> Lock<'a>,
    {
        unsafe { &*self.value.load(Ordering::Relaxed) }
    }
    pub fn write(&self) -> WriteGuard<'_, T> {
        WriteGuard {
            elem: self,
            _lock: self.lock.lock(),
        }
    }
}

impl<T> Drop for RcuElem<T> {
    fn drop(&mut self) {
        let e = self.value.swap(null_mut(), Ordering::Relaxed);
        if !e.is_null() {
            let _ = unsafe { Box::from_raw(e) };
        }
    }
}

impl<'a, T> WriteGuard<'a, T> {
    pub fn compare_exchange(&mut self, old: &T, new: Box<T>) -> Result<Option<Box<T>>, Box<T>> {
        match self.elem.value.compare_exchange(
            old as *const T as *mut T,
            Box::leak(new),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Err(e) => Err(unsafe { Box::from_raw(e) }),
            Ok(e) => {
                if e.is_null() {
                    Ok(None)
                } else {
                    Ok(Some(unsafe { Box::from_raw(e) }))
                }
            }
        }
    }
    pub fn swap(&mut self, new: Box<T>) -> Option<Box<T>> {
        let ret = self.elem.value.swap(Box::leak(new), Ordering::Relaxed);
        if ret.is_null() {
            None
        } else {
            Some(unsafe { Box::from_raw(ret) })
        }
    }
}

pub struct WriteGuard<'a, T> {
    elem: &'a RcuElem<T>,
    _lock: <Futex as Lock<'a>>::Guard,
}
impl<T> Deref for WriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.elem.value.load(Ordering::Relaxed) }
    }
}
