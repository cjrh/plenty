//! Managed cleanup inside provider callbacks must not escape their queue/scope.
use super::*;
use crate::memory::{Header, continue_after, plenty_release};
use std::sync::Arc;

#[derive(Default)]
struct Counts {
    finished: AtomicUsize,
    wrong_scope: AtomicUsize,
    order: AtomicUsize,
    at_callback_return: AtomicUsize,
    errors: AtomicUsize,
}
#[repr(C)]
struct Node {
    header: Header,
    child: Option<Box<Node>>,
    counts: Arc<Counts>,
    id: usize,
    protected: bool,
}
unsafe extern "C" fn destroy_node(header: *mut Header) {
    // SAFETY: every node is a Box transferred once to plenty_release. A resumed
    // parent stays allocated until its child has finished.
    unsafe {
        let node = &mut *header.cast::<Node>();
        if callback_active() != node.protected {
            node.counts.wrong_scope.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(child) = node.child.take() {
            continue_after(header, Box::into_raw(child).cast());
            return;
        }
        node.counts.finished.fetch_add(1, Ordering::Relaxed);
        let order = node.counts.order.load(Ordering::Relaxed);
        node.counts.order.store(
            order.wrapping_mul(10).wrapping_add(node.id),
            Ordering::Relaxed,
        );
        drop(Box::from_raw(header.cast::<Node>()));
    }
}
fn chain(count: usize, counts: &Arc<Counts>, protected: bool) -> Box<Node> {
    let mut child = None;
    for id in (1..=count).rev() {
        child = Some(Box::new(Node {
            header: Header::new(destroy_node),
            child,
            counts: counts.clone(),
            id,
            protected,
        }));
    }
    child.unwrap()
}
#[derive(Clone, Copy)]
enum Mode {
    Allocate,
    Release,
    Destroy,
    Unwind,
}
struct Policy {
    mode: Mode,
    node: Option<Box<Node>>,
    counts: Arc<Counts>,
}
impl Policy {
    fn cleanup(&mut self) {
        if let Some(node) = self.node.take() {
            unsafe {
                plenty_release(Box::into_raw(node).cast());
            }
            self.counts.at_callback_return.store(
                self.counts.finished.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
        }
    }
}
impl Provider for Policy {
    fn allocate(&mut self, layout: Layout) -> Result<Block, AllocError> {
        if matches!(self.mode, Mode::Allocate) {
            self.cleanup();
        }
        Allocator::system().allocate(layout)
    }
    fn release(&mut self, block: Block) {
        if matches!(self.mode, Mode::Release) {
            self.cleanup();
        }
        drop(block);
    }
}
impl Drop for Policy {
    fn drop(&mut self) {
        self.cleanup();
    }
}
#[repr(C)]
struct Driver {
    header: Header,
    allocator: ManuallyDrop<Allocator>,
    block: Option<Block>,
    tail: Option<Box<Node>>,
    counts: Arc<Counts>,
    mode: Mode,
}
unsafe extern "C" fn destroy_driver(header: *mut Header) {
    unsafe {
        let mut driver = Box::from_raw(header.cast::<Driver>());
        // This unrelated pending drop must survive isolation and must NOT
        // inherit the provider's process-fatal callback scope.
        plenty_release(Box::into_raw(driver.tail.take().unwrap()).cast());
        match driver.mode {
            Mode::Allocate => {
                let result = driver.allocator.allocate(Layout::new::<u64>());
                if result.is_err() {
                    driver.counts.errors.fetch_add(1, Ordering::Relaxed);
                }
                drop(result);
            }
            Mode::Release => drop(driver.block.take()),
            Mode::Destroy => {}
            Mode::Unwind => {
                // Catch before crossing this extern C boundary.
                let result =
                    std::panic::catch_unwind(|| callback(|| panic!("native provider unwind")));
                assert!(result.is_err());
                assert!(crate::memory::dropping());
            }
        }
        drop(ManuallyDrop::take(&mut driver.allocator));
        if callback_active() {
            driver.counts.wrong_scope.fetch_add(1, Ordering::Relaxed);
        }
    }
}
fn exercise(mode: Mode, depth: usize, fail: bool) {
    let counts = Arc::new(Counts::default());
    let tail = Arc::new(Counts::default());
    let policy = Policy {
        mode,
        node: Some(chain(depth, &counts, true)),
        counts: counts.clone(),
    };
    let allocator =
        Allocator::new(&Allocator::system(), policy).unwrap_or_else(|_| panic!("bootstrap failed"));
    let block =
        matches!(mode, Mode::Release).then(|| allocator.allocate(Layout::new::<u64>()).unwrap());
    let driver = Box::new(Driver {
        header: Header::new(destroy_driver),
        allocator: ManuallyDrop::new(allocator),
        block,
        tail: Some(chain(1, &tail, false)),
        counts: counts.clone(),
        mode,
    });
    #[cfg(feature = "allocation-checks")]
    crate::accounting::fail_after(fail.then_some(0));
    unsafe {
        plenty_release(Box::into_raw(driver).cast());
    }
    #[cfg(feature = "allocation-checks")]
    crate::accounting::fail_after(None);
    assert_eq!(
        counts.at_callback_return.load(Ordering::Relaxed),
        depth,
        "cleanup outlived callback"
    );
    assert_eq!(counts.finished.load(Ordering::Relaxed), depth);
    assert_eq!(counts.wrong_scope.load(Ordering::Relaxed), 0);
    assert_eq!(counts.errors.load(Ordering::Relaxed), usize::from(fail));
    if depth == 2 {
        assert_eq!(counts.order.load(Ordering::Relaxed), 21);
    }
    assert_eq!(tail.finished.load(Ordering::Relaxed), 1);
    assert_eq!(tail.wrong_scope.load(Ordering::Relaxed), 0);
    assert!(!callback_active());
    assert!(!crate::memory::dropping());
}
#[test]
fn managed_cleanup_finishes_inside_each_callback_and_restores_outer_queue() {
    for mode in [Mode::Allocate, Mode::Release, Mode::Destroy, Mode::Unwind] {
        exercise(mode, 2, false);
    }
}
#[test]
#[cfg(feature = "allocation-checks")]
fn managed_cleanup_finishes_before_recoverable_allocation_error_returns() {
    exercise(Mode::Allocate, 2, true);
}
#[test]
fn managed_cleanup_stays_iterative_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| exercise(Mode::Destroy, 10_000, false))
        .unwrap()
        .join()
        .unwrap();
}
