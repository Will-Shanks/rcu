use core::sync::atomic::{self, AtomicPtr, AtomicUsize, Ordering};
// TODO needing Mutex dependent on feature `std`, otherwise use a spinlock
use std::sync::Mutex;

struct TentryListElem {
    next: AtomicPtr<Option<TentryListElem>>,
    prev: AtomicPtr<Option<TentryListElem>>,
    elem: Tentry,
}

struct TentryList {
    head: AtomicPtr<Option<TentryListElem>>,
    mutex: Mutex<usize>,
}

struct TentryListIterator<'a> {
    #[allow(dead_code)]
    guard: QsbrGuard<'a>,
    next: Option<&'a TentryListElem>,
}

impl<'a> TentryListIterator<'a> {
    fn new(guard: QsbrGuard<'a>, list: &'a TentryList) -> Self {
        let tmp = list.head.load(Ordering::Relaxed);
        Self {
            next: unsafe { (*tmp).as_ref() },
            guard,
        }
    }
}

impl<'a> Iterator for TentryListIterator<'a> {
    type Item = &'a Tentry;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(e) = self.next {
            self.next = unsafe { (*e.next.load(Ordering::Relaxed)).as_ref() };
            return Some(&e.elem);
        }
        None
    }
}

impl TentryList {
    fn new() -> Self {
        let mut n: Option<TentryListElem> = None;
        Self {
            head: AtomicPtr::new(&mut n),
            mutex: Mutex::new(0),
        }
    }
    fn insert(&self, elem: Tentry) -> &Tentry {
        let guard = self.mutex.lock();
        let mut n: Option<TentryListElem> = None;
        let mut new_elem = TentryListElem {
            next: AtomicPtr::new(&mut n),
            prev: AtomicPtr::new(&mut n),
            elem,
        };
        let mut prev = self.head.load(Ordering::Relaxed);
        if prev.is_null() {
            self.head.store(&mut Some(new_elem), Ordering::Relaxed);
            let ret = unsafe { &(*self.head.load(Ordering::Relaxed)).as_ref().unwrap().elem };
            drop(guard);
            return ret;
        }
        while unsafe { (*prev).as_ref().unwrap().elem.id < new_elem.elem.id } {
            let next = unsafe { (*prev).as_ref().unwrap().next.load(Ordering::Relaxed) };
            if unsafe { (*next).is_none() } {
                break;
            } else {
                prev = next;
            }
        }
        let next = unsafe { (*prev).as_ref().unwrap().next.load(Ordering::Relaxed) };
        new_elem.next = AtomicPtr::new(next);
        new_elem.prev = AtomicPtr::new(prev);
        //TODO figure out how to remove need for Box
        let e: *mut Option<TentryListElem> = Box::leak(Box::new(Some(new_elem)));
        unsafe { (*next).as_ref().unwrap().prev.store(e, Ordering::Relaxed) };
        unsafe { (*prev).as_ref().unwrap().next.store(e, Ordering::Relaxed) };
        let ret = unsafe { &(*e).as_ref().unwrap().elem };

        drop(guard);
        ret
    }
    /// Saftey: Need to ensure no other threads are referencing the given Tentry before it is
    /// dropped, this can be done by syncing, plus waiting for all other threads already syncing
    /// to finish
    unsafe fn remove(&self, elem: &Tentry) -> Tentry {
        let guard = self.mutex.lock();
        let id = elem.id;
        //TODO make unsafe section smaller
        let mut e = self.head.load(Ordering::Relaxed);
        if e.is_null() {
            panic!("no elem in TentryList");
        }
        while unsafe { (*e).as_ref().unwrap().elem.id } != id {
            let tmp = unsafe { (*e).as_ref().unwrap().next.load(Ordering::Relaxed) };
            if unsafe { (*tmp).is_none() } {
                panic!("elem not found in TentryList");
            } else {
                e = tmp;
            }
        }
        let next = unsafe { (*e).as_ref().unwrap().next.load(Ordering::Relaxed) };
        let prev = unsafe { (*e).as_ref().unwrap().prev.load(Ordering::Relaxed) };
        unsafe {
            (*next)
                .as_ref()
                .unwrap()
                .prev
                .store(prev, Ordering::Relaxed)
        };
        unsafe {
            (*prev)
                .as_ref()
                .unwrap()
                .next
                .store(next, Ordering::Relaxed)
        };
        //TODO figure out how to remove need for Box
        let ret = unsafe { Box::from_raw(e).unwrap().elem };
        drop(guard);
        ret
    }
}

/// QSBR quiescent state based reclamation
/// This is the main entry point to everything
pub struct Qsbr {
    //threads will leave as long as self does
    threads: TentryList,
}

impl Default for Qsbr {
    fn default() -> Self {
        Self::new()
    }
}

impl Qsbr {
    /// create a new Qsbr
    pub fn new() -> Self {
        Self {
            threads: TentryList::new(),
        }
    }
    /// register a new thread with Qsbr
    /// takes an unique id for this handle
    /// thread::current().id().as_u64().get() could be a good choice if std is available
    pub fn register(&self, id: u64) -> QsbrThreadHandle {
        let elem: &Tentry = self.threads.insert(Tentry::new(id));
        QsbrThreadHandle {
            info: elem,
            qsbr: self,
        }
    }
    /// Saftey: Need to ensure no other threads are referencing the given Tentry before it is
    /// dropped, this can be done by syncing, plus waiting for all other threads already syncing
    /// to finish
    unsafe fn remove(&self, thread: &Tentry) -> Tentry {
        unsafe { self.threads.remove(thread) }
    }
}

///created via Qsbr::register(), used to register a thread with the Qsbr,
pub struct QsbrThreadHandle<'a> {
    qsbr: &'a Qsbr,
    info: &'a Tentry,
}

impl QsbrThreadHandle<'_> {
    fn threads_iter(&self) -> TentryListIterator {
        TentryListIterator::new(self.lock(), &self.qsbr.threads)
    }
    /// lock starts an rcu critical section, which lasts until the returned
    /// QsbrGuard is dropped
    pub fn lock(&self) -> QsbrGuard {
        QsbrGuard {
            thread_handle: self,
        }
    }
    /// quiescent_state is use to signal to the Qsbr that this thread has passed
    /// through a quiescent state. If this method is not called frequent enough
    /// other QsbrThreadHandle calling sync will block, reducing performance and
    /// in pathilogical cases, causing the program to crash due to OOMing
    pub fn quiescent_state(&mut self) {
        //Ordering: no other thread should be updating qstate, so relaxed is safe
        //make sure we don't accidentally wrap
        if self.info.qstate.load(Ordering::Relaxed) > usize::MAX / 2 {
            //Ordering: needs to happen before the sync that sees it
            self.info.qstate.store(10, Ordering::Release);
        } else {
            self.info.qstate.fetch_add(1, Ordering::Release);
        }
    }
    /// Used to synchronize all QsbrThreadHandles, blocks until all handles have
    /// called quiescent_state, signalling that a grace period has passed
    pub fn sync(&mut self) {
        //TODO
        // split code into 3 parts:
        // 1: create `local_copy` - a list of all Tentrys and their status
        // 2: `try_sync` - tries to sync, returns Ok if syned, Err is not
        // 3: loop over try_sync
        // This will be useful for future api expansion, ex: if an item came be removed from a
        // shared struct, but is needed for a while before dropping the sync list can be created
        // after the removal, but before the private computation, making the final drop likely
        // to go faster. Or for batching elements to drop together e.g. an async drop impl
        // also, a different try_sync_internal method can be used for the special sync needed before
        // dropping a Tentry

        // Ordering: set long term quescent state
        let prev_state = self.info.qstate.swap(1, Ordering::Release);
        // copy all Tentry's with Relaxed ordering
        let local_copy: Vec<(u64, usize)> = self
            .threads_iter()
            .map(|e: &Tentry| (e.id, e.qstate.load(Ordering::Relaxed)))
            .filter(|x| x.1 != 0)
            .collect();
        // Ordering: make qstate in `local_copy` happen before the comming for loop
        atomic::fence(Ordering::Acquire);
        let mut before = local_copy.into_iter();
        let mut b = if let Some(v) = before.next() {
            v
        } else {
            return;
        };
        for after in self.threads_iter() {
            // b is being dropped, so move on to next elem
            // could be removed, sync b _should_ have qstate set to 0
            while b.0 < after.id {
                b = if let Some(v) = before.next() {
                    v
                } else {
                    return;
                };
            }
            // after didn't exist when sync started, so move on to next elem
            if b.0 > after.id {
                continue;
            }
            assert!(b.0 == after.id);
            loop {
                //Ordering: Acq fence after for loop ensures qstate seen has already happened, so
                //can be relaxed here
                let qstate = after.qstate.load(Ordering::Relaxed);
                // if incr then passed through a qstate, if < 10 currently in a long quescent state
                if qstate != b.1 || qstate < 10 {
                    break;
                }
                std::hint::spin_loop();
            }
        }
        //Ordering: make sure all the Tentry's passed through a quescent state before returning
        atomic::fence(Ordering::Acquire);

        //Ordering: passed through a quescent state while syncing
        if prev_state > usize::MAX / 2 {
            self.info.qstate.store(10, Ordering::Release);
        } else {
            self.info.qstate.store(prev_state + 1, Ordering::Release);
        }
    }
}

//unregistering a thread
impl Drop for QsbrThreadHandle<'_> {
    /// unregisters the given handle with Qsbr
    fn drop(&mut self) {
        let tentry = unsafe { self.qsbr.remove(self.info) };
        // will never look at an RCU value again, perminent quiescent_state, set qstate
        self.info.qstate.store(0, Ordering::Release);
        //TODO FIXME wait for running syncs to end to avoid use after free
        //currently a double sync should be enough, but after impl a try_sync it won't be
        //anymore
        self.sync();
        self.sync();
        #[allow(clippy::drop_non_drop)]
        drop(tentry);
    }
}

/// QsbrGuard, used to track critical sections much like a MutexGuard
/// with a key difference being is doesn't block other reader/writers
/// i.e. the data being guarded can be concurrently read and modified
/// essentially it just guaranties existence of the protected struct for the
/// duration of the Guard's lifetime
pub struct QsbrGuard<'a> {
    #[allow(dead_code)]
    thread_handle: &'a QsbrThreadHandle<'a>,
}

// end the rcu critical section
impl Drop for QsbrGuard<'_> {
    /// ends the critical section
    fn drop(&mut self) {
        //QsbrThreadHandle unlock(), which is currently a noop
    }
}

struct Tentry {
    //incremented everytime this thread calls quiescent_state()
    //starts at 10
    //  0 means thread is in a quiescent_state, useful for removing
    //  1 means doing a sync which is a quescent_state, but means ThreadHandles shouldn't be
    //    dropped from the qsbr
    //
    //Tentrys, or for signalling extended quiescent states
    qstate: AtomicUsize,
    //this threads threadId
    id: u64,
}

impl Tentry {
    /// takes an unique id for this Tentry
    /// thread::current().id().as_u64().get() could be a good choice if std is available
    fn new(id: u64) -> Self {
        Tentry {
            qstate: AtomicUsize::new(1),
            id,
        }
    }
}
