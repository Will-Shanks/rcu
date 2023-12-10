//use rcu::rcu_elem::RcuElem;
use rcu::qsbr::Qsbr;

fn worker(id: u64, rcu_handle: &Qsbr) {
    let mut t_handle = rcu_handle.register(id);

    t_handle.quiescent_state();
}

#[test]
fn store_things() {
    let my_rcu = Qsbr::new();
    worker(0, &my_rcu);
    /* thread::scope(|s| {
        for i in 0..1 {
            let handle = &my_rcu;
            s.spawn(move || {worker(0, handle)});
        }
    });*/
}
