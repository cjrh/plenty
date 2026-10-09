//! Pairwise receive selection. Pinned stack nodes join each queue's intrusive
//! waiter list; queue locks protect registration, notification and removal.
//! A per-selection flag bridges queue inspection and condvar sleep without a
//! global lock, allocator calls, or a missed notification window.
use super::{core, Core, State};
use crate::{aggregates, aggregates::Type, deadline::Deadline, ranges};
use std::{
    cell::UnsafeCell,
    marker::PhantomPinned,
    pin::{pin, Pin},
    sync::{Condvar, Mutex},
};

struct Waiter {
    notified: Mutex<bool>,
    changed: Condvar,
}

struct Links {
    previous: *const Node,
    next: *const Node,
}

pub(super) struct Node {
    waiter: *const Waiter,
    links: UnsafeCell<Links>,
    _pin: PhantomPinned,
}

impl Node {
    fn new(waiter: &Waiter) -> Self {
        Self {
            waiter,
            links: UnsafeCell::new(Links {
                previous: std::ptr::null(),
                next: std::ptr::null(),
            }),
            _pin: PhantomPinned,
        }
    }
}

/// Must drop before its pinned node and waiter. Only this guard mutates list
/// membership, while all link access is serialized by the channel state lock.
struct Registered<'a> {
    channel: &'a Core,
    node: Pin<&'a Node>,
    _waiter: &'a Waiter,
}

impl<'a> Registered<'a> {
    fn new(channel: &'a Core, node: Pin<&'a Node>, waiter: &'a Waiter) -> Self {
        debug_assert_eq!(node.waiter, waiter as *const Waiter);
        let mut state = channel.lock();
        // SAFETY: node is pinned, unregistered, and outlives this guard. Every
        // linked node has a live guard, and state serializes link mutation.
        unsafe {
            let links = &mut *node.links.get();
            links.next = state.selectors;
            if !links.next.is_null() {
                (*(*links.next).links.get()).previous = node.get_ref();
            }
            state.selectors = node.get_ref();
        }
        Self {
            channel,
            node,
            _waiter: waiter,
        }
    }
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        let mut state = self.channel.lock();
        // SAFETY: this node and its neighbors stay pinned and live until their
        // guards remove them while holding this same lock. No notifier retains
        // a node or waiter pointer after releasing that lock.
        unsafe {
            let links = &*self.node.links.get();
            if links.previous.is_null() {
                state.selectors = links.next;
            } else {
                (*(*links.previous).links.get()).next = links.next;
            }
            if !links.next.is_null() {
                (*(*links.next).links.get()).previous = links.previous;
            }
        }
    }
}

/// Caller holds the channel state lock. Lock order is queue then waiter; the
/// selector always releases its waiter lock before accessing either queue.
pub(super) fn notify(state: &State) {
    // SAFETY: linked registrations pin their nodes and retain their waiter.
    unsafe {
        let mut node = state.selectors;
        while !node.is_null() {
            let waiter = &*(*node).waiter;
            *waiter
                .notified
                .lock()
                .unwrap_or_else(|_| crate::fail("poisoned channel selector mutex")) = true;
            waiter.changed.notify_one();
            node = (*(*node).links.get()).next;
        }
    }
}

enum Poll {
    Received,
    Pending,
    Disconnected,
}

unsafe fn poll(
    channel: &Core,
    state: &mut State,
    variant: u64,
    result_type: &Type,
    out: *mut u128,
) -> Poll {
    if state.len == 0 {
        return if state.send_closed {
            Poll::Disconnected
        } else {
            Poll::Pending
        };
    }
    // SAFETY: the live receiver retains the queue. The lock protects the source
    // slot until the entire selected result has relocated to caller storage.
    unsafe {
        let value = channel.slot(state.head).read();
        ranges::store(
            out,
            aggregates::wrap(aggregates::wrap(value, variant), 0),
            result_type,
        );
    }
    state.head = (state.head + 1) % channel.capacity;
    state.len -= 1;
    Poll::Received
}

unsafe fn poll_pair(first: &Core, second: &Core, result_type: &Type, out: *mut u128) -> Poll {
    unsafe {
        if std::ptr::eq(first, second) {
            let mut state = first.lock();
            let result = poll(first, &mut state, 0, result_type, out);
            drop(state);
            if matches!(result, Poll::Received) {
                first.writable.notify_one();
            }
            return result;
        }
        // Fixed address order prevents cycles between overlapping selections.
        // Both locks provide one readiness snapshot, so an Empty result cannot
        // overlook messages moving between the two independent inspections.
        let (mut first_state, mut second_state) =
            if std::ptr::from_ref(first).addr() < std::ptr::from_ref(second).addr() {
                (first.lock(), second.lock())
            } else {
                let second_state = second.lock();
                (first.lock(), second_state)
            };
        let first_poll = poll(first, &mut first_state, 0, result_type, out);
        if matches!(first_poll, Poll::Received) {
            drop(second_state);
            drop(first_state);
            first.writable.notify_one();
            return first_poll;
        }
        let second_poll = poll(second, &mut second_state, 1, result_type, out);
        drop(second_state);
        drop(first_state);
        if matches!(second_poll, Poll::Received) {
            second.writable.notify_one();
        }
        match (first_poll, second_poll) {
            (_, Poll::Received) => Poll::Received,
            (Poll::Disconnected, Poll::Disconnected) => Poll::Disconnected,
            _ => Poll::Pending,
        }
    }
}

pub(super) unsafe fn recv(
    first: u128,
    second: u128,
    nowait: bool,
    deadline: Option<&Deadline>,
    result_type: &Type,
    out: *mut u128,
) {
    // SAFETY: both receivers remain live, borrowed by the compiler for the full
    // operation, and output reserves the complete inline result layout.
    unsafe {
        let first = core(first);
        let second = core(second);
        match poll_pair(first, second, result_type, out) {
            Poll::Received => return,
            Poll::Disconnected => {
                out.write(aggregates::wrap(aggregates::wrap(0, 1), 1));
                return;
            }
            Poll::Pending => {}
        }
        if nowait || deadline.is_some_and(Deadline::expired) {
            let error = if nowait { 0 } else { 2 };
            out.write(aggregates::wrap(aggregates::wrap(0, error), 1));
            return;
        }

        let waiter = Waiter {
            notified: Mutex::new(false),
            changed: Condvar::new(),
        };
        let first_node = pin!(Node::new(&waiter));
        let second_node = pin!(Node::new(&waiter));
        let _first = Registered::new(first, first_node.as_ref(), &waiter);
        let _second = (!std::ptr::eq(first, second))
            .then(|| Registered::new(second, second_node.as_ref(), &waiter));

        loop {
            // Clear before checking queues. An arrival during/after polling
            // sets the flag, so the selector cannot sleep past that arrival.
            *waiter
                .notified
                .lock()
                .unwrap_or_else(|_| crate::fail("poisoned channel selector mutex")) = false;
            match poll_pair(first, second, result_type, out) {
                Poll::Received => return,
                Poll::Disconnected => {
                    out.write(aggregates::wrap(aggregates::wrap(0, 1), 1));
                    return;
                }
                Poll::Pending => {}
            }
            if deadline.is_some_and(Deadline::expired) {
                out.write(aggregates::wrap(aggregates::wrap(0, 2), 1));
                return;
            }
            let mut notified = waiter
                .notified
                .lock()
                .unwrap_or_else(|_| crate::fail("poisoned channel selector mutex"));
            while !*notified {
                if let Some(deadline) = deadline {
                    if deadline.expired() {
                        break;
                    }
                    notified = deadline.wait(&waiter.changed, notified);
                } else {
                    notified = waiter
                        .changed
                        .wait(notified)
                        .unwrap_or_else(|_| crate::fail("poisoned channel selector mutex"));
                }
            }
        }
    }
}
