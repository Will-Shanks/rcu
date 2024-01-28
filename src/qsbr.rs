use crate::cds::rculist::*;
use crate::utils::Lock;
use crate::{RcuGuard, RcuHandle, RCU};
use std::marker::PhantomData;
use std::sync::atomic::{self, AtomicU32, Ordering};

/// QSBR quiescent state based reclamation
/// This is the main entry point to everything
#[derive(Debug)]
pub struct Qsbr<L>
where
    L: for<'a> Lock<'a>,
{
    //threads will leave as long as self does
    threads: RcuList<Tentry, L>,
    lock: L,
}

impl<L> Default for Qsbr<L>
where
    L: for<'a> Lock<'a>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<L> Qsbr<L>
where
    L: for<'a> Lock<'a>,
{
    /// internal only, used to simplify dropping thread handles
    fn lock(&self) -> <L as Lock<'_>>::Guard {
        self.lock.lock()
    }

    /// Saftey: Need to ensure no other threads are referencing the given Tentry before it is
    /// dropped, this can be done by syncing, plus waiting for all other threads already syncing
    /// to finish
    unsafe fn remove(&self, elem: &Tentry) -> *mut RcuListElem<Tentry> {
        unsafe { self.threads.remove_unsynced(elem) }
    }
}

impl<L> RCU for Qsbr<L>
where
    L: for<'a> Lock<'a>,
{
    type Handle<'a> = QsbrThreadHandle<'a, L> where L: 'a;
    /// create a new Qsbr
    fn new() -> Self {
        Self {
            threads: RcuList::new(),
            lock: L::new(),
        }
    }
    /// register a new thread with Qsbr
    /// takes an unique id for this handle
    /// thread::current().id().as_u64().get() could be a good choice if std is available
    fn register(&self, id: u64) -> Self::Handle<'_> {
        let elem: &Tentry = self.threads.insert(Tentry::new(id));
        QsbrThreadHandle {
            info: elem,
            qsbr: self,
        }
    }
}

///created via Qsbr::register(), used to register a thread with the Qsbr,
pub struct QsbrThreadHandle<'a, L>
where
    L: for<'b> Lock<'b>,
    Self: 'a,
{
    qsbr: &'a Qsbr<L>,
    //needs to be an option so can set to None as part of Self::Drop
    info: &'a Tentry,
}

impl<L> RcuGuard<'_> for QsbrGuard<'_, L> where L: for<'lock> Lock<'lock> {}

impl<'a, L> RcuHandle<'a> for QsbrThreadHandle<'a, L>
where
    L: for<'lock> Lock<'lock>,
{
    type Guard<'b> = QsbrGuard<'b, L> where 'a: 'b;
    /// read starts an rcu critical section, which lasts until the returned
    /// QsbrGuard is dropped, is a no op, but used to ensure liveness of references
    /// by stop quescent_state from being called
    fn read(&self) -> Self::Guard<'a> {
        QsbrGuard {
            _thread_handle: &PhantomData,
        }
    }
    /// quiescent_state is use to signal to the Qsbr that this thread has passed
    /// through a quiescent state. If this method is not called frequent enough
    /// other QsbrThreadHandle calling sync will block, reducing performance and
    /// in pathilogical cases, causing the program to crash due to OOMing
    fn quiescent_state(&mut self) {
        //Ordering: no other thread should be updating qstate, so relaxed is safe
        //make sure we don't accidentally wrap
        if self.info.qstate.fetch_add(1, Ordering::Release) > u32::MAX / 2 {
            //Ordering: needs to happen before the sync that sees it
            self.info.qstate.store(10, Ordering::Release);
        }
        atomic_wait::wake_all(&self.info.qstate);
    }

    fn quiescent_sync(&mut self) {
        let qstate = self.info.qstate.swap(1, Ordering::Release);
        self.sync();
        if qstate > u32::MAX / 2 {
            //Ordering: needs to happen before the sync that sees it
            self.info.qstate.store(10, Ordering::Release);
        } else {
            self.info.qstate.store(qstate + 1, Ordering::Release);
        }
        atomic_wait::wake_all(&self.info.qstate);
    }

    /// Used to synchronize all QsbrThreadHandles, blocks until all handles have
    /// called quiescent_state, signalling that a grace period has passed
    fn sync(&self) {
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
        //let prev_state = self.info.qstate.swap(1, Ordering::Release);

        let local_copy = self.get_state();

        let mut before = local_copy.into_iter();
        let mut b = if let Some(v) = before.next() {
            v
        } else {
            return;
        };
        let guard = self.read();
        for after in RcuListIterator::new(&guard, &self.qsbr.threads) {
            // skip over this thread since we know it is in a quescent state (covered by b.1 == 1)
            // skip over threads in a long quescent state ( < 10 )
            while b.0 < after.id || b.1 < 10 {
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
            //assert!(b.0 == after.id);
            //let mut qstate = after.qstate.load(Ordering::Relaxed);
            // qstate only changes when passing through a quescent state
            // already filtered out threads that started in a long quescent state so don't need
            // to check that again here in the hot loop
            // TODO check if futex (atomic_wait::wait) or busy loop is better
            // might want to have an option to sync either way, or a hybrid approach depending on
            // how hot the call to sync is
            while b.1 == after.qstate.load(Ordering::Relaxed) {
                atomic_wait::wait(&after.qstate, b.1);
            }
        }
        //Ordering: make sure all the Tentry's passed through a quescent state before returning
        atomic::fence(Ordering::Acquire);

        //Ordering: passed through a quescent state while syncing
        //if prev_state > u32::MAX / 2 {
        //    self.info.qstate.store(10, Ordering::Release);
        //} else {
        //    self.info.qstate.store(prev_state + 1, Ordering::Release);
        //}
        atomic_wait::wake_all(&self.info.qstate);
    }
}

impl<L> QsbrThreadHandle<'_, L>
where
    L: for<'lock> Lock<'lock>,
{
    fn get_state(&self) -> Vec<(u64, u32)> {
        let guard = self.read();
        let state_copy: Vec<(u64, u32)> = RcuListIterator::new(&guard, &self.qsbr.threads)
            .map(|e: &Tentry| (e.id, e.qstate.load(Ordering::Relaxed)))
            .collect();
        // make sure state_copy "happened before" fn return
        atomic::fence(Ordering::Acquire);
        state_copy
    }

    // basically the same as regular sync, EXCEPT syncing doesn't count as a quescent state
    // this is needed for dropping a thread handle, since they are used to get thread states when
    // syncing on going syncs need to complete first
    fn drop_sync(&mut self) {
        self.info.qstate.store(2, Ordering::Release);
        atomic_wait::wake_all(&self.info.qstate);
        // TODO UNOPTIMIZED currently only allow one thread to drop sync at a time to stop
        // deadlocks where multiple drop_syncs are blocked on eachother, but it might be
        // possible to let multiple go concurrently, and break deadlocks via self.info.id ordering
        let guard = self.qsbr.lock();
        self.info.qstate.store(1, Ordering::Release);
        let local_copy = self.get_state();

        let mut before = local_copy.into_iter();
        let mut b = if let Some(v) = before.next() {
            v
        } else {
            return;
        };
        let my_guard = self.read();
        for after in RcuListIterator::new(&my_guard, &self.qsbr.threads) {
            // skip over this thread since we know it is in a quescent state
            // skip over threads in a long quescent state, but not syncing ( < 10 && != 1 )
            while b.0 < after.id || b.0 == self.info.id || ((b.1 < 10) && (b.1 != 1)) {
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
            // deadlock condition, if 2 threads are running drop_sync might both sleep waiting on the
            // other to finish so busy loop if b is in a special state, which should only be b.1 ==
            // 1 due to the while loop above. due to the lock around drop_sync any threads in state
            //   1 (syncing) besides self are in a regular sync
            if b.1 != 1 {
                while b.1 == after.qstate.load(Ordering::Relaxed) {
                    atomic_wait::wait(&after.qstate, b.1);
                }
            } else {
                //assert!(b.0 == after.id);
                let mut qstate = after.qstate.load(Ordering::Relaxed);
                // qstate only changes when passing through a quescent state
                // already filtered out threads that started in a long quescent state so don't need
                // to check that again here in the hot loop
                // don't need to check for syncing threads either, since either they started after the
                // thandle was removed from the list, they are no longer syncing, or their qstate
                // hasn't changed
                // TODO UNOPTIMIZED if multiple threads are syncing it would be better to have all but
                // one sleep (on a cond var?) otherwise cache contention will make this slower, plus
                // wasted cpu busy looping
                while qstate == b.1 {
                    std::hint::spin_loop();
                    //Ordering: Acq fence after for loop ensures qstate seen has already happened, so
                    //can be relaxed here
                    qstate = after.qstate.load(Ordering::Relaxed);
                }
            }
        }
        //Ordering: make sure all the Tentry's passed through a quescent state before returning
        atomic::fence(Ordering::Acquire);
        self.info.qstate.store(0, Ordering::Release);
        atomic_wait::wake_all(&self.info.qstate);
        drop(guard);
    }
}

//unregistering a thread
impl<L> Drop for QsbrThreadHandle<'_, L>
where
    L: for<'a> Lock<'a>,
{
    /// unregisters the given handle with Qsbr
    fn drop(&mut self) {
        let tentry_ptr = unsafe { self.qsbr.remove(self.info) };

        self.drop_sync();
        // nothing should have a ptr to this anymore, but just in case
        unsafe { (*tentry_ptr).elem.qstate.store(0, Ordering::Release) };
        // Saftey We are moving and dropping from a share reference
        //which _usually_ is a terrible idea, but since we know no one else has a reference to
        //it anymore since we removed it from the list, and did a drop_sync it is safe
        let _ = unsafe { Box::from_raw(tentry_ptr) };
    }
}

/// QsbrGuard, used to track critical sections much like a MutexGuard
/// with a key difference being is doesn't block other reader/writers
/// i.e. the data being guarded can be concurrently read and modified
/// essentially it just guaranties existence of the protected struct for the
/// duration of the Guard's lifetime
pub struct QsbrGuard<'a, L>
where
    L: for<'lock> Lock<'lock>,
{
    _thread_handle: &'a PhantomData<L>,
    //thread_handle: &'a QsbrThreadHandle<'a, L>,
}

// end the rcu critical section
impl<L> Drop for QsbrGuard<'_, L>
where
    L: for<'a> Lock<'a>,
{
    /// ends the critical section
    fn drop(&mut self) {
        //QsbrThreadHandle unlock(), which is currently a noop
    }
}

impl PartialEq for Tentry {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialOrd for Tentry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.id.cmp(&other.id))
    }
}

#[derive(Debug)]
struct Tentry {
    /// incremented everytime this thread calls quiescent_state()
    /// starts at 10
    ///  0 means thread is in a quiescent_state, useful for removing
    ///  1 means doing a sync which is a quescent_state, but means ThreadHandles shouldn't be
    ///    dropped from the qsbr
    ///  2 long quescent_state, safe to sync and crucially drop_sync as well
    ///
    /// Tentrys, or for signalling extended quiescent states
    qstate: AtomicU32,
    /// this thread's id, Note: nothing stops you from having mulitple tentry's per thread but
    /// you really shouldn't do that, but if you do you can't use the threadid since the id needs
    /// to be unique for each tentry
    id: u64,
}

impl Tentry {
    /// takes an unique id for this Tentry
    /// thread::current().id().as_u64().get() could be a good choice if std is available
    fn new(id: u64) -> Self {
        Tentry {
            qstate: AtomicU32::new(10),
            id,
        }
    }
}
