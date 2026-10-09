use super::*;

fn timeout_error(value: u128, variant: u64) -> u128 {
    aggregates::wrap(aggregates::wrap(value, variant), 1)
}

#[test]
fn zero_timeout_observes_ready_then_disconnected_then_expiration() {
    let deadline = Deadline::from_millis(0);
    let (sender, receiver) = create(&NUMBER, 1).unwrap();
    let mut out = 0;
    unsafe {
        recv_wait(receiver, false, Some(&deadline), &mut out);
        assert_eq!(out, timeout_error(0, 1));
        assert_eq!(send_wait(sender, 3, false, Some(&deadline)), 0);
        assert_eq!(
            send_wait(sender, 9, false, Some(&deadline)),
            timeout_error(9, 1)
        );
        recv_wait(receiver, false, Some(&deadline), &mut out);
        assert_eq!(out, 3);
        release(receiver);
        assert_eq!(
            send_wait(sender, 7, false, Some(&deadline)),
            timeout_error(7, 0)
        );
        release(sender);
    }

    let (sender, receiver) = create(&NUMBER, 1).unwrap();
    unsafe {
        assert_eq!(send_wait(sender, 11, false, Some(&deadline)), 0);
        release(sender);
        recv_wait(receiver, false, Some(&deadline), &mut out);
        assert_eq!(out, 11);
        recv_wait(receiver, false, Some(&deadline), &mut out);
        assert_eq!(out, timeout_error(0, 0));
        release(receiver);
    }
}

#[test]
fn timed_waits_wake_for_progress_and_disconnect() {
    let (sender, receiver) = create(&NUMBER, 1).unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let receiver_thread = scope.spawn(|| unsafe {
            let deadline = Deadline::from_millis(u64::MAX);
            let mut out = 0;
            recv_wait(receiver, false, Some(&deadline), &mut out);
            assert_eq!(out, 13);
            barrier.wait();
            release(receiver);
        });
        unsafe {
            let deadline = Deadline::from_millis(u64::MAX);
            assert_eq!(send_wait(sender, 13, false, Some(&deadline)), 0);
            assert_eq!(send_wait(sender, 14, false, Some(&deadline)), 0);
            barrier.wait();
            // This send either sees the already disconnected receiver, or
            // blocks behind 14 until its final drop wakes the writer.
            assert_eq!(
                send_wait(sender, 15, false, Some(&deadline)),
                timeout_error(15, 0)
            );
            release(sender);
        }
        receiver_thread.join().unwrap();
    });
}

#[test]
fn finite_waits_expire_without_queue_changes() {
    let (sender, receiver) = create(&NUMBER, 1).unwrap();
    unsafe {
        let mut out = 0;
        recv_wait(receiver, false, Some(&Deadline::from_millis(1)), &mut out);
        assert_eq!(out, timeout_error(0, 1));
        assert_eq!(send(sender, 1, false), 0);
        assert_eq!(
            send_wait(sender, 2, false, Some(&Deadline::from_millis(1))),
            timeout_error(2, 1)
        );
    }
    release(sender);
    release(receiver);
}
