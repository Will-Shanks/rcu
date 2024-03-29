use crate::utils::Lock;
use crate::{RcuHandle, RCU};
use std::cmp::{PartialEq, PartialOrd};
use std::marker::PhantomData;
use std::{
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
};

#[derive(Debug)]
pub struct RcuListElem<T>
where
    T: PartialEq,
    T: PartialOrd,
{
    next: AtomicPtr<RcuListElem<T>>,
    prev: AtomicPtr<RcuListElem<T>>,
    pub elem: T,
}

impl<T> PartialEq for RcuListElem<T>
where
    T: PartialEq,
    T: PartialOrd,
{
    fn eq(&self, other: &Self) -> bool {
        self.elem == other.elem
    }
}

impl<T> PartialOrd for RcuListElem<T>
where
    T: PartialOrd,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.elem.partial_cmp(&other.elem)
    }
}

#[derive(Debug)]
pub struct RcuList<T, R, L>
where
    RcuListElem<T>: PartialEq,
    RcuListElem<T>: PartialOrd,
    T: PartialEq,
    T: PartialOrd,
    R: RCU,
    L: for<'a> Lock<'a>,
{
    head: AtomicPtr<RcuListElem<T>>,
    // used for locking
    lock: L,
    _rcu: PhantomData<R>,
}

impl<T, R, L> Drop for RcuList<T, R, L>
where
    RcuListElem<T>: PartialEq,
    RcuListElem<T>: PartialOrd,
    T: PartialEq,
    T: PartialOrd,
    R: RCU,
    L: for<'a> Lock<'a>,
{
    fn drop(&mut self) {
        let mut tmp = self.head.load(Ordering::Relaxed);
        while !tmp.is_null() {
            let next = unsafe { (*tmp).next.load(Ordering::Relaxed) };
            let _ = unsafe { Box::from_raw(tmp) };
            tmp = next;
        }
        self.head.store(null_mut(), Ordering::Relaxed);
    }
}

pub struct RcuListIterator<'a, T, R>
where
    RcuListElem<T>: PartialEq,
    RcuListElem<T>: PartialOrd,
    T: PartialEq,
    T: PartialOrd,
    R: RCU + 'a,
{
    _guard: PhantomData<R>,
    next: Option<&'a RcuListElem<T>>,
}

impl<'a, T, R> RcuListIterator<'a, T, R>
where
    RcuListElem<T>: PartialEq,
    RcuListElem<T>: PartialOrd,
    T: PartialEq,
    T: PartialOrd,
    R: RCU + 'a,
{
    pub fn new<'b, L>(
        _guard: &'a <<R as RCU>::Handle<'b> as RcuHandle<'b>>::Guard<'b>,
        list: &'a RcuList<T, R, L>,
    ) -> Self
    where
        'b: 'a,
        L: for<'c> Lock<'c>,
    {
        let tmp = list.head.load(Ordering::Relaxed);
        Self {
            next: unsafe { tmp.as_ref() },
            _guard: PhantomData,
        }
    }
}

impl<'a, T, R> Iterator for RcuListIterator<'a, T, R>
where
    RcuListElem<T>: PartialEq,
    RcuListElem<T>: PartialOrd,
    T: PartialEq,
    T: PartialOrd,
    R: RCU,
{
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(e) = self.next {
            self.next = unsafe { e.next.load(Ordering::Relaxed).as_ref() };
            return Some(&e.elem);
        }
        None
    }
}

impl<T, R, L> Default for RcuList<T, R, L>
where
    RcuListElem<T>: PartialEq,
    RcuListElem<T>: PartialOrd,
    T: PartialEq,
    T: PartialOrd,
    R: RCU,
    L: for<'a> Lock<'a>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, R, L> RcuList<T, R, L>
where
    RcuListElem<T>: PartialEq,
    RcuListElem<T>: PartialOrd,
    T: PartialEq,
    T: PartialOrd,
    R: RCU,
    L: for<'a> Lock<'a>,
{
    pub fn new() -> Self {
        Self {
            head: AtomicPtr::new(null_mut()),
            lock: L::new(),
            _rcu: PhantomData,
        }
    }

    fn lock(&self) -> <L as Lock>::Guard {
        self.lock.lock()
    }

    pub fn insert(&self, elem: T) -> &T {
        let new_elem = RcuListElem {
            next: AtomicPtr::new(null_mut()),
            prev: AtomicPtr::new(null_mut()),
            elem,
        };
        //TODO UNOPTIMIZED create new_elem on the heap directly, instead of copying from stack
        let new_elem: *mut RcuListElem<T> = Box::leak(Box::new(new_elem));

        let guard = self.lock();
        let mut prev = self.head.load(Ordering::Relaxed);
        if prev.is_null() {
            self.head.store(new_elem, Ordering::Relaxed);
        } else {
            unsafe {
                let mut next = (*prev).next.load(Ordering::Relaxed);
                while !next.is_null() && (*next < *new_elem) {
                    prev = next;
                    next = (*next).next.load(Ordering::Relaxed);
                }
                (*new_elem).next.store(next, Ordering::Relaxed);
                (*new_elem).prev.store(prev, Ordering::Relaxed);
                (*prev).next.store(new_elem, Ordering::Relaxed);
                if !next.is_null() {
                    (*next).prev.store(new_elem, Ordering::Relaxed);
                }
            }
        }
        drop(guard);

        unsafe { &(*new_elem).elem }
    }

    /// Safely remove an element from the list
    ///
    /// WARNING: since this method calls `handle.sync()` it can cause a deadlock
    pub fn remove(&self, elem: &T, handle: &mut R::Handle<'_>) -> T {
        let popped_elem = unsafe { self.remove_unsynced(elem) };
        handle.quiescent_sync();

        unsafe { Box::from_raw(popped_elem) }.elem
    }

    /// # Safety
    ///
    /// Need to ensure no other threads are referencing the given Tentry before it is
    /// dropped, this can be done by syncing, plus waiting for all other threads already syncing
    /// to finish.
    pub unsafe fn remove_unsynced(&self, elem: &T) -> *mut RcuListElem<T> {
        let guard = self.lock();
        let mut e = self.head.load(Ordering::Relaxed);
        while !e.is_null() && unsafe { (*e).elem != *elem } {
            //TODO FIXME how are we hitting the expect condition
            e = unsafe { (*e).next.load(Ordering::Relaxed) };
        }
        assert!(!e.is_null());
        //need to update container struct if head changes
        //TODO FIXME find &self in list
        //remove e from list, e.g. make e.prev <---> e.next
        let next = unsafe { (*e).next.load(Ordering::Relaxed) };
        let prev = unsafe { (*e).prev.load(Ordering::Relaxed) };
        if !next.is_null() {
            unsafe { (*next).prev.store(prev, Ordering::Relaxed) };
        }
        if !prev.is_null() {
            unsafe { (*prev).next.store(next, Ordering::Relaxed) };
        } else {
            // if elem is self.head update the ptr
            self.head.store(next, Ordering::Relaxed);
        }

        drop(guard);
        e
    }
}
