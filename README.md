# RCU
## What is RCU
RCU (Read Copy Update) is a QSBR (Quiescent state based reclamation) memory reclamation algorithm useful for implementing lockless algorithms in languages without a garbage collector, similar to EBR (Epoch Based Reclamation) (see [crossbeam-epoch](https://github.com/crossbeam-rs/crossbeam/tree/master/crossbeam-epoch)) reclaims memory after a grace period has completed. A Key difference between EBR and QSBR is the caller must signal when a threads passes through a Quiescent state, in EBR this is hidden behind the API, since RCU can amortize this cost it can be more performant than EBR in many cases. see [Performance of memory reclamation for lockless synchronization](https://doi.org/10.1016/j.jpdc.2007.04.010) for an in depth performance comparrison.

RCU is heavily used in operating systems like Linux, but due to some design constraints (naimly preemption), is less popular in user space. However, there is an existing rcu library [librcu](http://liburcu.org/) for user space which contains a few differenet implementations.

## Goal of this library
Currently, this repo is just me messing around with RCU in rust, and should not be considered production ready in any capacity.
However, I hope to eventually implement a "safe, fast, concurrent" rcu api in rust, with comparibly performance to librcu. To this end, I hope to implement a few variations of rcu (subject to change):
- [ ] qsbr - caller must periodically call `rcu_quiescent_state()`
- [ ] async - for use in rust async runtimes, possibly use yield points (awaits) as quiescent points, similar to how the Linux Kernel impl uses context switches?
- [ ] mb - use memory barriers in readers and writers
- [ ] memb - like mb, but uses OS provided [membarrier](https://crates.io/crates/membarrier) if available

