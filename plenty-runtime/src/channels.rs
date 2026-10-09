//! Fallibly allocated bounded MPMC queues. Endpoint headers share one pinned
//! allocation; cloning a handle and waiting on the Linux futex-backed std
//! synchronization primitives need no additional allocator calls.
use crate::aggregates::{self, Type};
use crate::deadline::Deadline;
use crate::memory::{self, AllocError, Header};
use crate::ranges;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Condvar, Mutex, MutexGuard,
};

mod select;
#[cfg(test)]
mod tests;

#[repr(C)]
struct Endpoint {
    header: Header,
    core: *mut Core,
}

struct State {
    head: usize,
    len: usize,
    send_closed: bool,
    recv_closed: bool,
    selectors: *const select::Node,
}

#[repr(C, align(16))]
struct Core {
    sender: Endpoint,
    receiver: Endpoint,
    live_ends: AtomicUsize,
    message: &'static Type,
    capacity: usize,
    words: usize,
    buffer: *mut u128,
    state: Mutex<State>,
    readable: Condvar,
    writable: Condvar,
}

impl Core {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|_| crate::fail("poisoned channel mutex"))
    }
    unsafe fn slot(&self, index: usize) -> *mut u128 {
        // SAFETY: the pinned flexible allocation reserves capacity typed slots.
        // Queue access is protected by state, except final receiver cleanup,
        // after all producers have been excluded from touching the ring.
        unsafe { self.buffer.add(index * self.message.slot_words()) }
    }
}

unsafe fn core(endpoint: u128) -> &'static Core {
    // SAFETY: the caller owns or borrows a live endpoint for the entire operation.
    unsafe { &*(*(endpoint as *const Endpoint)).core }
}

pub(crate) fn create(message: &'static Type, capacity: usize) -> Result<(u128, u128), AllocError> {
    let words = capacity
        .checked_mul(message.slot_words())
        .ok_or(AllocError::CapacityOverflow)?;
    let pointer = memory::try_allocate::<Core, u128>(words)?;
    // SAFETY: this is the sole unpublished allocation. Embedded endpoints and
    // inline message storage never move before the final endpoint is destroyed.
    unsafe {
        pointer.write(Core {
            sender: Endpoint {
                header: Header::new(drop_sender),
                core: pointer,
            },
            receiver: Endpoint {
                header: Header::new(drop_receiver),
                core: pointer,
            },
            live_ends: AtomicUsize::new(2),
            message,
            capacity,
            words,
            buffer: pointer.add(1).cast(),
            state: Mutex::new(State {
                head: 0,
                len: 0,
                send_closed: false,
                recv_closed: false,
                selectors: std::ptr::null(),
            }),
            readable: Condvar::new(),
            writable: Condvar::new(),
        });
        Ok((
            std::ptr::addr_of_mut!((*pointer).sender) as u128,
            std::ptr::addr_of_mut!((*pointer).receiver) as u128,
        ))
    }
}

unsafe fn release_core(pointer: *mut Core) {
    // Each endpoint family holds one core lifetime count until its entire last-
    // release callback finishes, including any reentrant user destructors.
    unsafe {
        if (*pointer).live_ends.fetch_sub(1, Ordering::AcqRel) == 1 {
            let words = (*pointer).words;
            std::ptr::drop_in_place(pointer);
            memory::free::<Core, u128>(pointer, words);
        }
    }
}

unsafe extern "C" fn drop_sender(endpoint: *mut Header) {
    unsafe {
        let pointer = (*endpoint.cast::<Endpoint>()).core;
        let channel = &*pointer;
        let mut state = channel.lock();
        state.send_closed = true;
        select::notify(&state);
        drop(state);
        channel.readable.notify_all();
        release_core(pointer);
    }
}

unsafe extern "C" fn drop_receiver(endpoint: *mut Header) {
    unsafe {
        let pointer = (*endpoint.cast::<Endpoint>()).core;
        let channel = &*pointer;
        let (head, len) = {
            let mut state = channel.lock();
            state.recv_closed = true;
            let saved = (state.head, state.len);
            state.len = 0;
            saved
        };
        channel.writable.notify_all();
        // Never run Plenty destructors while holding a synchronization lock.
        // Closed receivers exclude every producer from writing another slot.
        for i in 0..len {
            aggregates::release(
                channel.slot((head + i) % channel.capacity).read(),
                channel.message,
            );
        }
        release_core(pointer);
    }
}

/// Transfer a message to the queue, or return it intact in an inline error.
/// nowait reports Full; a disconnected receiver always takes precedence.
pub(crate) unsafe fn send(endpoint: u128, value: u128, nowait: bool) -> u128 {
    unsafe { send_wait(endpoint, value, nowait, None) }
}

unsafe fn send_wait(
    endpoint: u128,
    value: u128,
    nowait: bool,
    deadline: Option<&Deadline>,
) -> u128 {
    unsafe {
        let channel = core(endpoint);
        let mut state = channel.lock();
        while state.len == channel.capacity && !state.recv_closed {
            if nowait {
                return aggregates::wrap(aggregates::wrap(value, 0), 1);
            }
            if let Some(deadline) = deadline {
                if deadline.expired() {
                    return aggregates::wrap(aggregates::wrap(value, 1), 1);
                }
                state = deadline.wait(&channel.writable, state);
            } else {
                state = channel
                    .writable
                    .wait(state)
                    .unwrap_or_else(|_| crate::fail("poisoned channel mutex"));
            }
        }
        if state.recv_closed {
            let variant = u64::from(deadline.is_none());
            return aggregates::wrap(aggregates::wrap(value, variant), 1);
        }
        let index = (state.head + state.len) % channel.capacity;
        ranges::store(channel.slot(index), value, channel.message);
        state.len += 1;
        select::notify(&state);
        drop(state);
        channel.readable.notify_one();
        0 // Ok(())
    }
}

/// Move the message into caller-owned output storage while still holding the
/// lock, before another producer can reuse its old ring slot.
pub(crate) unsafe fn recv(endpoint: u128, nowait: bool, out: *mut u128) {
    unsafe { recv_wait(endpoint, nowait, None, out) }
}

unsafe fn recv_wait(endpoint: u128, nowait: bool, deadline: Option<&Deadline>, out: *mut u128) {
    unsafe {
        let channel = core(endpoint);
        let mut state = channel.lock();
        while state.len == 0 && !state.send_closed {
            if nowait {
                out.write(aggregates::wrap(aggregates::wrap(0, 0), 1));
                return;
            }
            if let Some(deadline) = deadline {
                if deadline.expired() {
                    out.write(aggregates::wrap(aggregates::wrap(0, 1), 1));
                    return;
                }
                state = deadline.wait(&channel.readable, state);
            } else {
                state = channel
                    .readable
                    .wait(state)
                    .unwrap_or_else(|_| crate::fail("poisoned channel mutex"));
            }
        }
        if state.len == 0 {
            let variant = u64::from(deadline.is_none());
            out.write(aggregates::wrap(aggregates::wrap(0, variant), 1));
            return;
        }
        let value = channel.slot(state.head).read();
        let value = ranges::copy_payload(value, channel.message, out.add(1).cast());
        out.write(aggregates::wrap(value, 0));
        state.head = (state.head + 1) % channel.capacity;
        state.len -= 1;
        drop(state);
        channel.writable.notify_one();
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_channel(
    op: i64,
    args: *const u128,
    descriptor: *const Type,
    out: *mut u128,
) {
    // SAFETY: the compiler independently checks each opcode's arity/types,
    // allocates full typed output storage, and retains the endpoint until return.
    unsafe {
        match op {
            0 => out.write(aggregates::channel_new(&*descriptor, *args as usize)),
            1 | 2 => {
                let result = send(*args, *args.add(1), op == 2);
                let result = ranges::copy_payload(result, &*descriptor, out.add(1).cast());
                out.write(result);
            }
            3 | 4 => recv(*args, op == 4, out),
            5 => {
                let deadline = Deadline::from_millis(*args.add(2) as u64);
                let result = send_wait(*args, *args.add(1), false, Some(&deadline));
                ranges::store(out, result, &*descriptor);
            }
            6 => {
                let deadline = Deadline::from_millis(*args.add(1) as u64);
                recv_wait(*args, false, Some(&deadline), out);
            }
            7..=9 => {
                let deadline = (op == 9).then(|| Deadline::from_millis(*args.add(2) as u64));
                select::recv(
                    *args,
                    *args.add(1),
                    op == 8,
                    deadline.as_ref(),
                    &*descriptor,
                    out,
                );
            }
            _ => crate::fail("invalid channel operation"),
        }
    }
}
