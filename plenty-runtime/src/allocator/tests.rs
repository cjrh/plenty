use super::*;
use std::sync::Arc;
use std::thread;

#[derive(Default)]
struct Counts {
    allocations: AtomicUsize,
    releases: AtomicUsize,
    drops: AtomicUsize,
    clean: AtomicUsize,
    final_thread: Mutex<Option<thread::ThreadId>>,
}
struct Policy {
    parent: Allocator,
    counts: Arc<Counts>,
    exhausted: bool,
}
impl Provider for Policy {
    fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
        self.counts.allocations.fetch_add(1, Ordering::Relaxed);
        if self.exhausted {
            Err(AllocError::OutOfMemory)
        } else {
            self.parent.allocate(layout)
        }
    }
    fn release(&mut self, block: Block) {
        self.counts.releases.fetch_add(1, Ordering::Relaxed);
        assert!(block.bytes().iter().all(|b| *b == 0));
        self.counts.clean.fetch_add(1, Ordering::Relaxed);
        drop(block);
    }
}
impl Drop for Policy {
    fn drop(&mut self) {
        self.counts.drops.fetch_add(1, Ordering::Relaxed);
        *self.counts.final_thread.lock().unwrap() = Some(thread::current().id());
    }
}
fn policy(parent: Allocator, counts: &Arc<Counts>) -> Policy {
    Policy {
        parent,
        counts: counts.clone(),
        exhausted: false,
    }
}
fn create(bootstrap: &Allocator, provider: impl Provider) -> Allocator {
    Allocator::new(bootstrap, provider).unwrap_or_else(|failure| panic!("{:?}", failure.error))
}
fn bytes(size: usize) -> Layout {
    Layout::from_size_align(size, 8).unwrap()
}

#[test]
fn system_blocks_are_initialized_aligned_and_split_disjointly() {
    for alignment in [1, 2, 8, 16, 64, 4096] {
        for size in [0, 1, 7, 8, 31, 513] {
            let mut block = Allocator::system()
                .allocate(Layout::from_size_align(size, alignment).unwrap())
                .unwrap();
            assert_eq!(block.bytes().len(), size);
            assert!(block.alignment() >= alignment);
            assert!(block.bytes().iter().all(|b| *b == 0));
            let mut suffix = block.split_off(size / 2);
            block.bytes_mut().fill(17);
            suffix.bytes_mut().fill(29);
            assert!(block.bytes().iter().all(|b| *b == 17));
            drop(block);
            assert!(suffix.bytes().iter().all(|b| *b == 29));
            let empty = suffix.split_off(suffix.len);
            drop(suffix);
            assert!(empty.bytes().is_empty());
        }
    }
}

#[test]
fn provider_state_respects_overalignment() {
    #[repr(align(4096))]
    struct Aligned;
    impl Provider for Aligned {
        fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
            Allocator::system().allocate(layout)
        }
        fn release(&mut self, block: Block) {
            drop(block);
        }
    }
    let allocator = create(&Allocator::system(), Aligned);
    assert_eq!(allocator.0.unwrap().as_ptr().addr() % 4096, 0);
    drop(allocator.allocate(bytes(1)).unwrap());
}

#[cfg(all(
    feature = "allocation-checks",
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn explicit_system_root_ignores_ambient_arena() {
    let mut arena = crate::actor_heap::Arena::new(4096).unwrap();
    let block = crate::actor_heap::within(&mut arena, || {
        Allocator::system().allocate(bytes(64)).unwrap()
    })
    .unwrap();
    assert_eq!(arena.high_water(), 0);
    drop(arena);
    assert!(block.bytes().iter().all(|b| *b == 0));
}

#[test]
fn blocks_retain_provider_until_last_range_and_release_before_state_drop() {
    let counts = Arc::new(Counts::default());
    let root = Allocator::system();
    let allocator = create(&root, policy(root.clone(), &counts));
    let mut block = allocator.allocate(bytes(32)).unwrap();
    block.bytes_mut().fill(91);
    let suffix = block.split_off(7);
    drop(allocator);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 0);
    drop(block);
    assert_eq!(counts.releases.load(Ordering::Relaxed), 0);
    drop(suffix);
    assert_eq!(counts.releases.load(Ordering::Relaxed), 1);
    assert_eq!(counts.clean.load(Ordering::Relaxed), 1);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
}

#[test]
fn nested_bootstrap_and_storage_owners_outlive_caller_handles() {
    let first = Arc::new(Counts::default());
    let second = Arc::new(Counts::default());
    let root = Allocator::system();
    let parent = create(&root, policy(root.clone(), &first));
    let child = create(&parent, policy(parent.clone(), &second));
    let block = child.allocate(bytes(13)).unwrap();
    drop(parent);
    drop(child);
    assert_eq!(first.drops.load(Ordering::Relaxed), 0);
    assert_eq!(second.drops.load(Ordering::Relaxed), 0);
    drop(block);
    assert_eq!(first.drops.load(Ordering::Relaxed), 1);
    assert_eq!(second.drops.load(Ordering::Relaxed), 1);
    assert_eq!(first.releases.load(Ordering::Relaxed), 2); // child state + data
    assert_eq!(second.releases.load(Ordering::Relaxed), 1);
}

#[test]
fn failed_bootstrap_returns_provider_and_never_falls_back() {
    let root = Allocator::system();
    let exhausted = Arc::new(Counts::default());
    let bootstrap = create(
        &root,
        Policy {
            parent: root.clone(),
            counts: exhausted.clone(),
            exhausted: true,
        },
    );
    let counts = Arc::new(Counts::default());
    let failure = match Allocator::new(&bootstrap, policy(root, &counts)) {
        Err(failure) => failure,
        Ok(_) => panic!("bootstrap silently fell back"),
    };
    assert_eq!(failure.error, AllocError::OutOfMemory);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 0);
    drop(failure.provider);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
    assert_eq!(exhausted.allocations.load(Ordering::Relaxed), 1);
    assert_eq!(exhausted.releases.load(Ordering::Relaxed), 0);
}

#[test]
fn independent_provider_failures_and_capacity_overflow_do_not_fall_back() {
    let root = Allocator::system();
    let exhausted = Arc::new(Counts::default());
    let healthy = Arc::new(Counts::default());
    let a = create(
        &root,
        Policy {
            parent: root.clone(),
            counts: exhausted.clone(),
            exhausted: true,
        },
    );
    let b = create(&root, policy(root.clone(), &healthy));
    assert!(matches!(a.allocate(bytes(9)), Err(AllocError::OutOfMemory)));
    let block = b.allocate(bytes(9)).unwrap();
    let calls = healthy.allocations.load(Ordering::Relaxed);
    let huge = Layout::from_size_align(isize::MAX as usize, 1).unwrap();
    assert!(matches!(
        b.allocate(huge),
        Err(AllocError::CapacityOverflow)
    ));
    assert_eq!(healthy.allocations.load(Ordering::Relaxed), calls);
    drop(block);
}

#[test]
fn concurrent_callbacks_are_serialized_and_final_state_can_drop_on_worker() {
    struct Counter {
        parent: Allocator,
        count: usize, // deliberately non-atomic; only callback serialization protects it
        result: Arc<AtomicUsize>,
    }
    impl Provider for Counter {
        fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
            let n = self.count;
            thread::yield_now();
            self.count = n + 1;
            self.parent.allocate(layout)
        }
        fn release(&mut self, block: Block) {
            drop(block);
        }
    }
    impl Drop for Counter {
        fn drop(&mut self) {
            self.result.store(self.count, Ordering::Relaxed);
        }
    }
    let root = Allocator::system();
    let result = Arc::new(AtomicUsize::new(0));
    let allocator = create(
        &root,
        Counter {
            parent: root.clone(),
            count: 0,
            result: result.clone(),
        },
    );
    thread::scope(|scope| {
        for _ in 0..8 {
            let handle = allocator.clone();
            scope.spawn(move || {
                for _ in 0..100 {
                    let mut block = handle.allocate(bytes(32)).unwrap();
                    block.bytes_mut().fill(71);
                    drop(block);
                }
            });
        }
        drop(allocator);
    });
    assert_eq!(result.load(Ordering::Relaxed), 800);

    let counts = Arc::new(Counts::default());
    let allocator = create(&root, policy(root.clone(), &counts));
    let block = allocator.allocate(bytes(1)).unwrap();
    drop(allocator);
    let final_thread = thread::spawn(move || {
        drop(block);
        thread::current().id()
    })
    .join()
    .unwrap();
    assert_eq!(*counts.final_thread.lock().unwrap(), Some(final_thread));
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
}

#[test]
fn disjoint_ranges_can_move_to_independent_workers() {
    let mut left = Allocator::system().allocate(bytes(32)).unwrap();
    let mut right = left.split_off(16);
    thread::scope(|scope| {
        scope.spawn(|| left.bytes_mut().fill(11));
        scope.spawn(|| right.bytes_mut().fill(22));
    });
    assert_eq!(left.bytes(), &[11; 16]);
    assert_eq!(right.bytes(), &[22; 16]);
}

#[test]
fn released_storage_can_be_cached_and_reused() {
    struct Pool {
        parent: Allocator,
        cached: Option<Block>,
        fresh: Arc<AtomicUsize>,
    }
    impl Provider for Pool {
        fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
            if let Some(block) = self.cached.take() {
                assert!(block.len >= layout.size() && block.alignment() >= layout.align());
                return Ok(block);
            }
            self.fresh.fetch_add(1, Ordering::Relaxed);
            self.parent.allocate(layout)
        }
        fn release(&mut self, block: Block) {
            assert!(block.bytes().iter().all(|b| *b == 0));
            self.cached = Some(block);
        }
    }
    let root = Allocator::system();
    let fresh = Arc::new(AtomicUsize::new(0));
    let pool = create(
        &root,
        Pool {
            parent: root.clone(),
            cached: None,
            fresh: fresh.clone(),
        },
    );
    for _ in 0..10 {
        let mut block = pool.allocate(bytes(17)).unwrap();
        assert!(block.bytes().iter().all(|b| *b == 0));
        block.bytes_mut().fill(255);
    }
    assert_eq!(fresh.load(Ordering::Relaxed), 1);
}

#[cfg(feature = "allocation-checks")]
#[test]
fn actual_system_failure_and_allocation_free_handle_operations() {
    let counts = Arc::new(Counts::default());
    crate::accounting::begin_no_allocations();
    let system = Allocator::system();
    let clone = system.clone();
    drop(clone);
    crate::accounting::end_no_allocations();
    let provider = policy(system.clone(), &counts);
    crate::accounting::fail_after(Some(0));
    let failure = Allocator::new(&system, provider);
    crate::accounting::fail_after(None);
    let Err(failure) = failure else {
        panic!("system failure not propagated")
    };
    assert_eq!(failure.error, AllocError::OutOfMemory);
    drop(failure.provider);
    assert_eq!(counts.drops.load(Ordering::Relaxed), 1);
    let allocator = create(&system, policy(system.clone(), &counts));
    crate::accounting::begin_no_allocations();
    let clone = allocator.clone();
    drop(clone);
    crate::accounting::end_no_allocations();
    crate::accounting::fail_after(Some(0));
    let block = allocator.allocate(bytes(7));
    crate::accounting::fail_after(None);
    assert!(matches!(block, Err(AllocError::OutOfMemory)));
    assert_eq!(counts.releases.load(Ordering::Relaxed), 0);
}

#[test]
fn invalid_callbacks_trap_instead_of_exposing_storage_or_deadlocking() {
    for (case, message) in [
        ("short", "insufficient extent or alignment"),
        ("misaligned", "insufficient extent or alignment"),
        ("reentrant", "reentrant allocator callback"),
    ] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "allocator::tests::trap_probe", "--nocapture"])
            .env("PLENTY_ALLOCATOR_TRAP_PROBE", case)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("{case}: allocator callback deadlocked");
            }
            thread::sleep(std::time::Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{case}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(message),
            "{output:?}"
        );
    }
}

#[test]
fn trap_probe() {
    let Ok(case) = std::env::var("PLENTY_ALLOCATOR_TRAP_PROBE") else {
        return;
    };
    struct Bad(String, Arc<Mutex<Option<Allocator>>>);
    impl Provider for Bad {
        fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
            match self.0.as_str() {
                "short" => Allocator::system().allocate(bytes(1)),
                "misaligned" => {
                    let mut block = Allocator::system().allocate(
                        Layout::from_size_align(layout.size() + 1, layout.align()).unwrap(),
                    )?;
                    Ok(block.split_off(1))
                }
                "reentrant" => {
                    let allocator = self.1.lock().unwrap().as_ref().unwrap().clone();
                    allocator.allocate(layout)
                }
                _ => panic!("unknown probe"),
            }
        }
        fn release(&mut self, block: Block) {
            drop(block);
        }
    }
    let root = Allocator::system();
    let slot = Arc::new(Mutex::new(None));
    let allocator = create(&root, Bad(case, slot.clone()));
    *slot.lock().unwrap() = Some(allocator.clone());
    drop(allocator.allocate(bytes(1)));
    panic!("invalid callback did not trap");
}
