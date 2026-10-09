//! Explicitly shared cooperative cancellation; dropping a handle does not cancel.
use crate::aggregates;
use crate::deadline::Deadline;
use crate::memory::{self, AllocError, Header};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

#[repr(C)]
struct Token {
    header: Header,
    cancelled: AtomicBool,
    gate: Mutex<()>,
    changed: Condvar,
}

fn create() -> Result<*mut Token, AllocError> {
    let token = memory::try_allocate::<Token, u8>(0)?;
    // SAFETY: this is the uniquely owned, correctly aligned allocation.
    unsafe {
        token.write(Token {
            header: Header::new(destroy),
            cancelled: AtomicBool::new(false),
            gate: Mutex::new(()),
            changed: Condvar::new(),
        });
    }
    Ok(token)
}

impl Token {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn cancel(&self) {
        // Synchronize predicate changes with waiter registration. Polling does
        // not take the mutex; the release/acquire pair publishes cancellation.
        let guard = self
            .gate
            .lock()
            .unwrap_or_else(|_| crate::fail("poisoned cancellation mutex"));
        self.cancelled.store(true, Ordering::Release);
        drop(guard);
        self.changed.notify_all();
    }

    fn wait(&self, deadline: Option<Deadline>) -> bool {
        let mut guard = self
            .gate
            .lock()
            .unwrap_or_else(|_| crate::fail("poisoned cancellation mutex"));
        loop {
            // Cancellation wins when it is visible as the deadline expires.
            if self.is_cancelled() {
                return true;
            }
            guard = match &deadline {
                Some(deadline) if deadline.expired() => return false,
                Some(deadline) => deadline.wait(&self.changed, guard),
                None => self
                    .changed
                    .wait(guard)
                    .unwrap_or_else(|_| crate::fail("poisoned cancellation mutex")),
            };
        }
    }
}

unsafe extern "C" fn destroy(header: *mut Header) {
    // SAFETY: each wait borrows a live ownership count. No waiter or caller
    // remains when the final release destroys the synchronization primitives.
    unsafe {
        let token = header.cast::<Token>();
        std::ptr::drop_in_place(token);
        memory::free::<Token, u8>(token, 0);
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_control(
    op: i64,
    args: *const u128,
    _descriptor: *const aggregates::Type,
    out: *mut u128,
) {
    // SAFETY: compiler-checked operations provide live token borrows, the exact
    // arity, and one output slot. The constructor returns its sole owned count.
    unsafe {
        let value = match op {
            0 => match create() {
                Ok(token) => aggregates::wrap(token as u128, 0),
                Err(error) => aggregates::wrap(aggregates::wrap(0, error as u64), 1),
            },
            1 => {
                (&*(*args as *const Token)).cancel();
                0
            }
            2 => (&*(*args as *const Token)).is_cancelled() as u128,
            3 => {
                (&*(*args as *const Token)).wait(None);
                0
            }
            4 => (&*(*args as *const Token)).wait(Some(Deadline::from_millis(*args.add(1) as u64)))
                as u128,
            _ => crate::fail("invalid cancellation operation"),
        };
        out.write(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{plenty_release, plenty_retain};
    use std::sync::Barrier;

    #[test]
    fn sharing_and_dropping_do_not_cancel() {
        unsafe {
            let token = create().unwrap();
            plenty_retain(token.cast());
            plenty_release(token.cast());
            assert!(!(*token).is_cancelled());
            assert!(!(*token).wait(Some(Deadline::from_millis(0))));
            assert!(!(*token).wait(Some(Deadline::from_millis(1))));
            (*token).cancel();
            (*token).cancel();
            assert!((*token).is_cancelled());
            assert!((*token).wait(Some(Deadline::from_millis(0))));
            assert!((*token).wait(Some(Deadline::from_millis(u64::MAX))));
            plenty_release(token.cast());
        }
    }

    #[test]
    fn cancellation_wakes_every_waiter_and_publishes_prior_writes() {
        let published = AtomicBool::new(false);
        let barrier = Barrier::new(5);
        unsafe {
            let token = create().unwrap();
            let address = token.expose_provenance();
            for _ in 0..4 {
                plenty_retain(token.cast());
            }
            std::thread::scope(|scope| {
                for index in 0..4 {
                    let published = &published;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        let token = std::ptr::with_exposed_provenance::<Token>(address);
                        barrier.wait();
                        let deadline = (index % 2 == 0).then(|| Deadline::from_millis(u64::MAX));
                        assert!((*token).wait(deadline));
                        assert!(published.load(Ordering::Relaxed));
                        plenty_release(token.cast_mut().cast());
                    });
                }
                barrier.wait();
                published.store(true, Ordering::Relaxed);
                (*token).cancel();
                plenty_release(token.cast());
            });
        }
    }
}
