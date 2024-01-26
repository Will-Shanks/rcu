use rcu::qsbr::Qsbr;
use std::thread;

fn register_worker(id: u64, rcu_handle: &Qsbr) {
    let mut _t_handle = rcu_handle.register(id);
}

#[test]
fn can_make_qsbr() {
    let _my_rcu = Qsbr::new();
}

#[test]
fn single_threaded_register() {
    let my_rcu = Qsbr::new();
    println!("{:?}", my_rcu);
    let _t_handle = my_rcu.register(1);
    println!("{:?}", my_rcu);
}

#[test]
fn single_other_thread_register() {
    let my_rcu = Qsbr::new();
    thread::scope(|s| {
        s.spawn(move || register_worker(0, &my_rcu));
    });
}

#[test]
fn multi_threaded_register() {
    let my_rcu = Qsbr::new();
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
