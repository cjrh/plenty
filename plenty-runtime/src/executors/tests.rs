use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

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
};

unsafe extern "C" fn square(input: *const u128, out: *mut u128) {
    unsafe {
        out.write(*input * *input);
    }
}

// Miri cannot call pthread_create. Source integration tests also exercise these
// native lifecycle paths; the cell/ring tests below can run under Miri.
#[test]
#[cfg(not(miri))]
fn reuses_workers_and_preserves_results_after_shutdown() {
    let pool = create(3, 2).unwrap();
    let mut jobs = Vec::new();
    unsafe {
        for value in 0..100 {
            jobs.push(submit(pool, value, &NUMBER, &NUMBER, square, false).unwrap());
        }
        shutdown(pool, false);
        plenty_release(pool.cast());
        for (value, job) in jobs.into_iter().enumerate() {
            let mut output = 0;
            result(job, &mut output);
            assert_eq!(output, (value * value) as u128);
            plenty_release(job.cast());
        }
    }
}

#[test]
#[cfg(not(miri))]
fn partial_native_start_failure_joins_the_started_prefix() {
    for limit in 0..3 {
        threads::fail_after(Some(limit));
        assert_eq!(
            create(3, 2).unwrap_err(),
            error_payload(2, aggregates::wrap(11, 0))
        );
        threads::fail_after(None);
    }
    assert_eq!(create(0, 1).unwrap_err(), 0);
    assert_eq!(create(1, 0).unwrap_err(), 0);
    assert_eq!(
        create(usize::MAX, 1).unwrap_err(),
        error_payload(1, aggregates::wrap(0, 1))
    );
}

struct Gate {
    started: AtomicBool,
    open: AtomicBool,
}
unsafe extern "C" fn gated(input: *const u128, out: *mut u128) {
    unsafe {
        let gate = &*(*input as *const Gate);
        gate.started.store(true, Ordering::Release);
        while !gate.open.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        out.write(42);
    }
}

#[test]
#[cfg(not(miri))]
fn bounded_submission_cancellation_and_closed_recovery() {
    let gate = Gate {
        started: AtomicBool::new(false),
        open: AtomicBool::new(false),
    };
    unsafe {
        let pool = create(1, 1).unwrap();
        let first = submit(
            pool,
            &gate as *const Gate as u128,
            &NUMBER,
            &NUMBER,
            gated,
            false,
        )
        .unwrap();
        while !gate.started.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        assert!(!cancel(first));
        assert!(!done(first));
        let second = submit(pool, 9, &NUMBER, &NUMBER, square, true).unwrap();
        assert_eq!(
            submit(pool, 7, &NUMBER, &NUMBER, square, true).unwrap_err(),
            0
        );
        assert!(cancel(second));
        assert!(cancel(second));
        assert!(done(second));
        let mut output = 0;
        result(second, &mut output);
        assert_eq!(output, aggregates::wrap(0, 1));
        plenty_release(second.cast());
        gate.open.store(true, Ordering::Release);
        shutdown(pool, false);
        assert_eq!(
            submit(pool, 7, &NUMBER, &NUMBER, square, true).unwrap_err(),
            1
        );
        result(first, &mut output);
        assert_eq!(output, 42);
        plenty_release(first.cast());
        plenty_release(pool.cast());
    }
}

#[test]
fn cell_moves_inline_result_before_releasing_storage() {
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
    };
    unsafe {
        let words = 1 + RANGE.slot_words();
        let job = memory::try_allocate::<Job, u128>(words).unwrap();
        job.write(Job {
            header: Header::new(destroy_job),
            input: &NUMBER,
            output: &RANGE,
            entry: square,
            data: job.add(1).cast(),
            words,
            phase: Mutex::new(Phase::Ready),
            completed: Condvar::new(),
        });
        let range = ranges::Range::new(3, 9, 1, true);
        ranges::store(
            (*job).data.add(1),
            &range as *const ranges::Range as u128,
            &RANGE,
        );
        let mut output = [0u128; 3];
        result(job, output.as_mut_ptr());
        plenty_release(job.cast());
        assert_eq!((*(output[0] as *const ranges::Range)).start, 3);
        assert_eq!(output[0], output.as_ptr().add(1) as u128);
    }
}

#[test]
fn timed_wait_preserves_pending_cancelled_and_completed_futures() {
    unsafe {
        let words = 2;
        let job = memory::try_allocate::<Job, u128>(words).unwrap();
        job.write(Job {
            header: Header::new(destroy_job),
            input: &NUMBER,
            output: &NUMBER,
            entry: square,
            data: job.add(1).cast(),
            words,
            phase: Mutex::new(Phase::Pending),
            completed: Condvar::new(),
        });
        (*job).data.write(7);
        assert!(!wait_timeout(job, 0));
        assert!(!wait_timeout(job, 1));
        assert!(!done(job));
        assert!(cancel(job));
        assert!(wait_timeout(job, 0));
        assert!(wait_timeout(job, u64::MAX));
        let mut out = 0;
        result(job, &mut out);
        assert_eq!(out, aggregates::wrap(0, 1));
        plenty_release(job.cast());

        let job = memory::try_allocate::<Job, u128>(words).unwrap();
        job.write(Job {
            header: Header::new(destroy_job),
            input: &NUMBER,
            output: &NUMBER,
            entry: square,
            data: job.add(1).cast(),
            words,
            phase: Mutex::new(Phase::Running),
            completed: Condvar::new(),
        });
        assert!(!wait_timeout(job, 0));
        let address = job.expose_provenance();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let job = std::ptr::with_exposed_provenance_mut::<Job>(address);
                (*job).data.add(1).write(42);
                *lock(&(*job).phase) = Phase::Ready;
                (*job).completed.notify_all();
            });
            assert!(wait_timeout(job, u64::MAX));
            assert!(wait_timeout(job, 0));
            result(job, &mut out);
            assert_eq!(out, 42);
        });
        plenty_release(job.cast());
    }
}

#[test]
fn ring_and_cells_synchronize_competing_workers() {
    // Rust-owned native test threads also work in Miri. They run the very same
    // scheduler entry as pthread workers; only startup/join differ.
    let pool = allocate_pool(2, 1).unwrap();
    unsafe {
        for i in 0..2 {
            (*pool).workers.add(i).write(Worker { thread: 0, pool });
        }
        let address = pool.expose_provenance();
        std::thread::scope(|scope| {
            for i in 0..2 {
                scope.spawn(move || {
                    let pool = std::ptr::with_exposed_provenance_mut::<Pool>(address);
                    run_worker((*pool).workers.add(i).cast());
                });
            }
            let mut jobs = Vec::new();
            for n in 0..24 {
                let job = submit(pool, n, &NUMBER, &NUMBER, square, false).unwrap();
                if n % 3 == 0 {
                    plenty_release(job.cast());
                } else {
                    jobs.push((n, job));
                }
            }
            for (n, job) in jobs {
                let mut out = 0;
                result(job, &mut out);
                assert_eq!(out, n * n);
                plenty_release(job.cast());
            }
            lock(&(*pool).queue).closed = true;
            (*pool).readable.notify_all();
        });
        (*pool).joined = true;
        plenty_release(pool.cast());
    }
}

#[test]
fn pair_jobs_relocate_both_inline_captures_and_result() {
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
    };
    static PAIR: Type = Type {
        kind: b'H',
        affine: true,
        reflexive: false,
        inline_range: false,
        inline_bytes: 112,
        key: None,
        value: None,
        name: "pair",
        variants: &[aggregates::Variant {
            name: "captures",
            fields: &[&RANGE, &RANGE],
        }],
    };
    unsafe extern "C" fn combine(input: *const u128, out: *mut u128) {
        unsafe {
            let environment = *input as *const u128;
            let mut left = *(*environment.add(1) as *const ranges::Range);
            let right = *(*environment.add(4) as *const ranges::Range);
            left.start += right.start;
            ranges::store(out, &left as *const ranges::Range as u128, &RANGE);
        }
    }
    let pool = allocate_pool(1, 1).unwrap();
    unsafe {
        (*pool).workers.write(Worker { thread: 0, pool });
        let address = pool.expose_provenance();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let pool = std::ptr::with_exposed_provenance_mut::<Pool>(address);
                run_worker((*pool).workers.cast());
            });
            let left = ranges::Range::new(3, 9, 1, true);
            let right = ranges::Range::new(4, 10, 1, true);
            let job = submit_pair(
                pool,
                [
                    &left as *const ranges::Range as u128,
                    &right as *const ranges::Range as u128,
                ],
                &PAIR,
                &RANGE,
                combine,
            )
            .unwrap();
            let mut output = [0u128; 3];
            result(job, output.as_mut_ptr());
            plenty_release(job.cast());
            assert_eq!(output[0], output.as_ptr().add(1) as u128);
            assert_eq!((*(output[0] as *const ranges::Range)).start, 7);
            assert_eq!(left.start, 3);
            assert_eq!(right.start, 4);
            lock(&(*pool).queue).closed = true;
            (*pool).readable.notify_all();
        });
        (*pool).joined = true;
        plenty_release(pool.cast());
    }
}

#[test]
#[cfg(not(miri))]
fn cancel_shutdown_skips_queued_jobs_but_waits_for_running_work() {
    let gate = Gate {
        started: AtomicBool::new(false),
        open: AtomicBool::new(false),
    };
    unsafe {
        let pool = create(1, 1).unwrap();
        let first = submit(
            pool,
            &gate as *const Gate as u128,
            &NUMBER,
            &NUMBER,
            gated,
            false,
        )
        .unwrap();
        while !gate.started.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let queued = submit(pool, 7, &NUMBER, &NUMBER, square, false).unwrap();
        let queue_address = std::ptr::addr_of!((*pool).queue).expose_provenance();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let queue = &*std::ptr::with_exposed_provenance::<Mutex<Queue>>(queue_address);
                while !lock(queue).closed {
                    std::thread::yield_now();
                }
                gate.open.store(true, Ordering::Release);
            });
            shutdown(pool, true);
        });
        let mut out = 0;
        result(first, &mut out);
        assert_eq!(out, 42);
        result(queued, &mut out);
        assert_eq!(out, aggregates::wrap(0, 1));
        plenty_release(first.cast());
        plenty_release(queued.cast());
        plenty_release(pool.cast());
    }
}
