use super::*;
use crate::aggregates::Variant;

static RANGE: Type = Type {
    kind: b'R',
    inline_range: true,
    inline_bytes: 32,
    key: Some(&NUMBER),
    name: "range",
    ..NUMBER
};
static ERROR: Type = Type {
    kind: b'B',
    name: "SelectError",
    variants: &[
        Variant {
            name: "Empty",
            fields: &[],
        },
        Variant {
            name: "Disconnected",
            fields: &[],
        },
        Variant {
            name: "TimedOut",
            fields: &[],
        },
    ],
    ..NUMBER
};
static SELECTED: Type = Type {
    kind: b'B',
    name: "Selected[i64,i64]",
    variants: &[
        Variant {
            name: "First",
            fields: &[&NUMBER],
        },
        Variant {
            name: "Second",
            fields: &[&NUMBER],
        },
    ],
    ..NUMBER
};
static RESULT: Type = Type {
    kind: b'B',
    name: "Result[Selected[i64,i64],SelectError]",
    variants: &[
        Variant {
            name: "Ok",
            fields: &[&SELECTED],
        },
        Variant {
            name: "Err",
            fields: &[&ERROR],
        },
    ],
    ..NUMBER
};

fn error(variant: u64) -> u128 {
    aggregates::wrap(aggregates::wrap(0, variant), 1)
}

fn selected(value: u128, variant: u64) -> u128 {
    aggregates::wrap(aggregates::wrap(value, variant), 0)
}

#[test]
fn selection_prioritizes_first_and_disconnects_only_when_both_are_drained() {
    let (a, first) = create(&NUMBER, 2).unwrap();
    let (b, second) = create(&NUMBER, 2).unwrap();
    let mut out = 0;
    unsafe {
        select::recv(first, second, true, None, &RESULT, &mut out);
        assert_eq!(out, error(0));
        assert_eq!(send(a, 1, false), 0);
        assert_eq!(send(b, 2, false), 0);
        release(a);
        select::recv(first, second, true, None, &RESULT, &mut out);
        assert_eq!(out, selected(1, 0));
        select::recv(first, second, true, None, &RESULT, &mut out);
        assert_eq!(out, selected(2, 1));
        select::recv(first, second, true, None, &RESULT, &mut out);
        assert_eq!(out, error(0));
        release(b);
        select::recv(
            first,
            second,
            false,
            Some(&Deadline::from_millis(0)),
            &RESULT,
            &mut out,
        );
        assert_eq!(out, error(1));
    }
    release(first);
    release(second);
}

#[test]
fn duplicate_receiver_selects_once_and_unregisters_after_expiration() {
    let (sender, receiver) = create(&NUMBER, 1).unwrap();
    let mut out = 0;
    unsafe {
        select::recv(
            receiver,
            receiver,
            false,
            Some(&Deadline::from_millis(1)),
            &RESULT,
            &mut out,
        );
        assert_eq!(out, error(2));
        assert!(core(receiver).lock().selectors.is_null());
        assert_eq!(send(sender, 5, false), 0);
        select::recv(
            receiver,
            receiver,
            false,
            Some(&Deadline::from_millis(0)),
            &RESULT,
            &mut out,
        );
        assert_eq!(out, selected(5, 0));
        select::recv(receiver, receiver, true, None, &RESULT, &mut out);
        assert_eq!(out, error(0));
        release(sender);
        select::recv(receiver, receiver, false, None, &RESULT, &mut out);
        assert_eq!(out, error(1));
    }
    release(receiver);
}

#[test]
fn selecting_inline_payload_relocates_before_slot_reuse() {
    static SELECTED_RANGE: Type = Type {
        kind: b'B',
        inline_range: true,
        inline_bytes: 32,
        variants: &[
            Variant {
                name: "First",
                fields: &[&NUMBER],
            },
            Variant {
                name: "Second",
                fields: &[&RANGE],
            },
        ],
        ..SELECTED
    };
    static RANGE_RESULT: Type = Type {
        inline_range: true,
        inline_bytes: 32,
        variants: &[
            Variant {
                name: "Ok",
                fields: &[&SELECTED_RANGE],
            },
            Variant {
                name: "Err",
                fields: &[&ERROR],
            },
        ],
        ..RESULT
    };
    let (a, first) = create(&NUMBER, 1).unwrap();
    let (b, second) = create(&RANGE, 1).unwrap();
    let value = ranges::Range::new(8, 12, 1, true);
    let replacement = ranges::Range::new(20, 24, 1, true);
    let mut out = [0u128; 3];
    unsafe {
        assert_eq!(send(b, &value as *const _ as u128, false), 0);
        select::recv(first, second, false, None, &RANGE_RESULT, out.as_mut_ptr());
        assert_eq!(send(b, &replacement as *const _ as u128, false), 0);
        let pointer = out[0] as u64 as *const ranges::Range;
        assert_eq!(pointer, out.as_ptr().add(1).cast());
        assert_eq!((*pointer).start, 8);
        assert_eq!(out[0] >> 64, 2); // Ok(Second(range))
    }
    release(a);
    release(b);
    release(first);
    release(second);
}

#[test]
fn registered_selector_wakes_for_second_and_removes_both_nodes() {
    let (a, first) = create(&NUMBER, 1).unwrap();
    let (b, second) = create(&NUMBER, 1).unwrap();
    std::thread::scope(|scope| {
        let thread = scope.spawn(|| {
            let mut out = 0;
            unsafe { select::recv(first, second, false, None, &RESULT, &mut out) };
            assert_eq!(out, selected(21, 1));
        });
        unsafe {
            while core(second).lock().selectors.is_null() {
                std::thread::yield_now();
            }
            assert!(!core(first).lock().selectors.is_null());
            assert_eq!(send(b, 21, false), 0);
        }
        thread.join().unwrap();
    });
    unsafe {
        assert!(core(first).lock().selectors.is_null());
        assert!(core(second).lock().selectors.is_null());
        // A notification after stack storage has gone away must see no node.
        assert_eq!(send(a, 1, false), 0);
        assert_eq!(send(b, 2, false), 0);
    }
    release(a);
    release(b);
    release(first);
    release(second);
}

#[test]
fn competing_selectors_deliver_every_message_once_and_wake_on_final_sender() {
    let (a, first) = create(&NUMBER, 1).unwrap();
    let (b, second) = create(&NUMBER, 1).unwrap();
    unsafe {
        plenty_retain(first as *mut Header);
        plenty_retain(second as *mut Header);
    }
    let total = std::thread::scope(|scope| {
        let consumers: Vec<_> = (0..2)
            .map(|index| {
                scope.spawn(move || unsafe {
                    let (first, second) = if index == 0 {
                        (first, second)
                    } else {
                        (second, first)
                    };
                    let mut sum = 0;
                    loop {
                        let mut out = 0;
                        select::recv(first, second, false, None, &RESULT, &mut out);
                        if out & (1u128 << 64) != 0 {
                            assert_eq!(out, error(1));
                            break;
                        }
                        sum += out as u64;
                    }
                    release(first);
                    release(second);
                    sum
                })
            })
            .collect();
        let producer_a = scope.spawn(move || {
            for n in 1..=16 {
                assert_eq!(unsafe { send(a, n, false) }, 0);
            }
            release(a);
        });
        let producer_b = scope.spawn(move || {
            for n in 17..=32 {
                assert_eq!(unsafe { send(b, n, false) }, 0);
            }
            release(b);
        });
        producer_a.join().unwrap();
        producer_b.join().unwrap();
        consumers
            .into_iter()
            .map(|c| c.join().unwrap())
            .sum::<u64>()
    });
    assert_eq!(total, (1..=32).sum());
}
