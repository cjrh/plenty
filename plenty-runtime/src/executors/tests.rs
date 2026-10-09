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
