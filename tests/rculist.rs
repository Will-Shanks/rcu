use rcu::utils::{Futex, Lock, SpinLock};
use rcu::{cds::rculist::RcuList, cds::rculist::RcuListIterator, qsbr::Qsbr};
use std::thread;

fn modify_rcu<L>(id: u64, rcu_handle: &Qsbr<L>, list: &RcuList<u32, L>)
where
    L: for<'a> Lock<'a>,
{
    let mut t_handle = rcu_handle.register(id);
    let id = id.try_into().unwrap();
    list.insert(id);
    let guard = t_handle.read();
    let list_iter = RcuListIterator::new(&guard, list);
    let elems: Vec<_> = list_iter.collect();
    assert!(elems.contains(&&id));
    // test rcu_list drop
    if id % 2 == 0 {
        let my_elem = list.remove(&id, &t_handle);
        assert!(my_elem == id);
    }
    drop(guard);
    t_handle.quiescent_state();
}

#[test]
fn single_threaded_list_futex() {
    let my_rcu = Qsbr::new();
    let my_list = RcuList::<u32, Futex>::new();
    thread::scope(|s| {
        let handle = &my_rcu;
        thread::Builder::new()
            .name("child-1".to_string())
            .spawn_scoped(s, move || {
                modify_rcu(1, handle, &my_list);
            })
            .unwrap();
    });
}

#[test]
fn single_threaded_list_spin() {
    let my_rcu = Qsbr::new();
    let my_list = RcuList::<u32, SpinLock>::new();
    thread::scope(|s| {
        let handle = &my_rcu;
        thread::Builder::new()
            .name("child-1".to_string())
            .spawn_scoped(s, move || {
                modify_rcu(1, handle, &my_list);
            })
            .unwrap();
    });
}

#[test]
fn multi_threaded_list_futex() {
    let my_rcu = Qsbr::new();
    let my_list = RcuList::<u32, Futex>::new();
    thread::scope(|s| {
        for i in 0..20 {
            let handle = &my_rcu;
            let list = &my_list;
            thread::Builder::new()
                .name(format!("child-{}", i))
                .spawn_scoped(s, move || {
                    modify_rcu(i, handle, list);
                })
                .unwrap();
        }
    });
}

#[test]
fn multi_threaded_list_spin() {
    let my_rcu = Qsbr::new();
    let my_list = RcuList::<u32, SpinLock>::new();
    thread::scope(|s| {
        for i in 0..20 {
            let handle = &my_rcu;
            let list = &my_list;
            thread::Builder::new()
                .name(format!("child-{}", i))
                .spawn_scoped(s, move || {
                    modify_rcu(i, handle, list);
                })
                .unwrap();
        }
    });
}
