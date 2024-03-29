#![deny(unsafe_op_in_unsafe_fn)]
pub mod cds;
pub mod qsbr;
pub mod utils;

pub trait RCU {
    type Handle<'a>: RcuHandle<'a>
    where
        Self: 'a;
    fn new() -> Self;
    fn register(&self, id: u64) -> Self::Handle<'_>;
}

pub trait RcuHandle<'a> {
    type Guard<'g>: RcuGuard<'g>
    where
        Self: 'a,
        'a: 'g;
    type Sleeper<'s>: SleepingRcu<'s>
    where
        Self: 'a,
        'a: 's;
    fn read(&self) -> Self::Guard<'a>;
    fn quiescent_state(&mut self);
    fn sleep(self) -> Self::Sleeper<'a>;
    fn sync(&self);
    fn quiescent_sync(&mut self);
}

pub trait RcuGuard<'a> {}

pub trait SleepingRcu<'a> {
    type Handle: RcuHandle<'a>;
    fn wake(self) -> Self::Handle;
}
