// membarrier/mb rcu
// link with -lurcu
// for mb only (generally slower) link with -lurcu-mb
// for signal link with -lurcu-signal
#include <urcu.h> 

//quescent state based rcu
// link with -lurcu-qsbr
#include <urcu-qsbr.h>

//"bulletproof" rcu
//link with -lurcu-bp
#include <urcu-bp.h>


