use rcu::qsbr::Qsbr;
use rcu::utils::{Futex, Lock, SpinLock};
use rcu::RCU;
use std::thread;

fn register_worker<L>(id: u64, rcu_handle: &Qsbr<L>)
where
    L: for<'a> Lock<'a>,
{
    let mut _t_handle = rcu_handle.register(id);
}

#[test]
fn can_make_qsbr_futex() {
    let _my_rcu = Qsbr::<Futex>::new();
}

#[test]
fn can_make_qsbr_spin() {
    let _my_rcu = Qsbr::<SpinLock>::new();
}

#[test]
fn single_threaded_register_futex() {
    let my_rcu = Qsbr::<Futex>::new();
    println!("{:?}", my_rcu);
    let _t_handle = my_rcu.register(1);
    println!("{:?}", my_rcu);
}

#[test]
fn single_threaded_register_spin() {
    let my_rcu = Qsbr::<SpinLock>::new();
    println!("{:?}", my_rcu);
    let _t_handle = my_rcu.register(1);
    println!("{:?}", my_rcu);
}

#[test]
fn single_other_thread_register_futex() {
    let my_rcu = Qsbr::<Futex>::new();
    thread::scope(|s| {
        s.spawn(move || register_worker(0, &my_rcu));
    });
}

#[test]
fn single_other_thread_register_spin() {
    let my_rcu = Qsbr::<SpinLock>::new();
    thread::scope(|s| {
        s.spawn(move || register_worker(0, &my_rcu));
    });
}

#[test]
fn multi_threaded_register_futex() {
    let my_rcu = Qsbr::<Futex>::new();
    thread::scope(|s| {
        for i in 0..20 {
            let handle = &my_rcu;
            thread::Builder::new()
                .name(format!("child-{}", i.clone()))
                .spawn_scoped(s, move || {
                    register_worker(i, handle);
                    register_worker(i, handle);
                    register_worker(i, handle);
                })
                .unwrap();
        }
    });
}

#[test]
fn multi_threaded_register_spin() {
    let my_rcu = Qsbr::<SpinLock>::new();
    thread::scope(|s| {
        for i in 0..20 {
            let handle = &my_rcu;
            thread::Builder::new()
                .name(format!("child-{}", i.clone()))
                .spawn_scoped(s, move || {
                    register_worker(i, handle);
                    register_worker(i, handle);
                    register_worker(i, handle);
                })
                .unwrap();
        }
    });
}
