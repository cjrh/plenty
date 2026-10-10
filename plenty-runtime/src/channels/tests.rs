use super::*;
use crate::memory::{plenty_release, plenty_retain};

mod selection;
mod timeouts;

static NUMBER: Type = Type {
    kind: b'4',
    affine: false,
    reflexive: true,
    inline_range: false,
    inline_bytes: 0,
    key: None,
    value: None,
    name: "i64",
    variants: &[],
    drop: None,
};

fn release(value: u128) {
    unsafe { plenty_release(value as *mut Header) };
}

#[test]
fn tiny_ring_wraps_under_competing_producers_and_consumers() {
    let (sender, receiver) = create(&NUMBER, 2).unwrap();
    unsafe {
        plenty_retain(sender as *mut Header);
        plenty_retain(receiver as *mut Header);
    }
    let total = std::thread::scope(|scope| {
        let producers: Vec<_> = (0..2)
            .map(|producer| {
                scope.spawn(move || {
                    for i in 0..64 {
                        assert_eq!(unsafe { send(sender, producer * 64 + i, false) }, 0);
                    }
                    release(sender);
                })
            })
            .collect();
        let consumers: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(move || {
                    let mut total = 0;
                    loop {
                        let mut out = 0;
                        unsafe { recv(receiver, false, &mut out) };
                        if out >> 64 != 0 {
                            assert_eq!(out, aggregates::wrap(aggregates::wrap(0, 1), 1));
                            break;
                        }
                        total += out;
                    }
                    release(receiver);
                    total
                })
            })
            .collect();
        for producer in producers {
            producer.join().unwrap();
        }
        consumers
            .into_iter()
            .map(|c| c.join().unwrap())
            .sum::<u128>()
    });
    assert_eq!(total, (0..128).sum::<u128>());
}

#[test]
fn disconnect_wakes_full_senders_and_empty_receivers() {
    for _ in 0..16 {
        let (sender, receiver) = create(&NUMBER, 1).unwrap();
        assert_eq!(unsafe { send(sender, 1, false) }, 0);
        let (ready, entered) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            ready.send(()).unwrap();
            let error = unsafe { send(sender, 2, false) };
            release(sender);
            error
        });
        entered.recv().unwrap();
        release(receiver);
        assert_eq!(
            thread.join().unwrap(),
            aggregates::wrap(aggregates::wrap(2, 1), 1)
        );

        let (sender, receiver) = create(&NUMBER, 1).unwrap();
        let (ready, entered) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            ready.send(()).unwrap();
            let mut out = 0;
            unsafe { recv(receiver, false, &mut out) };
            release(receiver);
            out
        });
        entered.recv().unwrap();
        release(sender);
        assert_eq!(
            thread.join().unwrap(),
            aggregates::wrap(aggregates::wrap(0, 1), 1)
        );
    }
}

#[test]
fn receiving_copies_inline_ranges_before_reusing_the_slot() {
    static RANGE: Type = Type {
        kind: b'R',
        affine: false,
        reflexive: true,
        inline_range: true,
        inline_bytes: 32,
        key: Some(&NUMBER),
        value: None,
        name: "range",
        variants: &[],
        drop: None,
    };
    let (sender, receiver) = create(&RANGE, 1).unwrap();
    let first = ranges::Range::new(5, 9, 1, true);
    let second = ranges::Range::new(20, 24, 1, true);
    let mut out = [0u128; 3];
    unsafe {
        assert_eq!(
            send(sender, &first as *const ranges::Range as u128, false),
            0
        );
        recv(receiver, false, out.as_mut_ptr());
        assert_eq!(
            send(sender, &second as *const ranges::Range as u128, false),
            0
        );
        assert_eq!((*(out[0] as *const ranges::Range)).start, 5);
        assert_eq!(out[0], out.as_ptr().add(1) as u128);
    }
    release(sender);
    release(receiver);
}
