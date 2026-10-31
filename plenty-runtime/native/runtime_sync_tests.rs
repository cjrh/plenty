//! Standalone tests for the real synchronization backends while unrelated
//! runtime modules are still being ported. Compile as a Cargo test target with
//! libc = "=0.2.186"; this imports the production mutex and thread source.
#[path = "../src/mutex.rs"]
mod mutex;
#[path = "../src/threads.rs"]
mod threads;

fn fail(message: &str) -> ! {
    panic!("{message}")
}

// These are unrelated runtime policy hooks, not synchronization substitutes.
mod actor_heap {
    pub(crate) fn running() -> bool {
        false
    }
}

mod stack_overflow {
    pub(crate) fn guarded<T>(entry: impl FnOnce() -> T) -> T {
        entry()
    }
}
