//! Shared workload: regex is the real external crate; the future is a local fixture.
use regex::Regex;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

static MATCHERS: AtomicUsize = AtomicUsize::new(0);
static FUTURES: AtomicUsize = AtomicUsize::new(0);

pub fn scalar(value: i64) -> i64 {
    value.checked_add(1).unwrap()
}

pub fn char_count(text: &str) -> i64 {
    text.chars().count().try_into().unwrap()
}

pub fn identity<T>(value: T) -> T {
    value
}

pub struct Matcher(Regex);

impl Drop for Matcher {
    fn drop(&mut self) {
        MATCHERS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn new_matcher(pattern: &str) -> Matcher {
    let regex = Regex::new(pattern).expect("fixture pattern must be valid");
    MATCHERS.fetch_add(1, Ordering::SeqCst);
    Matcher(regex)
}

pub fn match_count(matcher: &Matcher, text: &str) -> i64 {
    matcher.0.find_iter(text).count().try_into().unwrap()
}

pub fn drop_matcher(matcher: Matcher) -> i64 {
    drop(matcher);
    1
}

struct PendingOnce {
    value: i64,
    pending: bool,
}

impl Future for PendingOnce {
    type Output = i64;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<i64> {
        if self.pending {
            self.pending = false;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(self.value.checked_add(2).unwrap())
        }
    }
}

pub struct Pending {
    future: Pin<Box<PendingOnce>>,
    result: Option<i64>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        FUTURES.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn new_future(value: i64) -> Pending {
    FUTURES.fetch_add(1, Ordering::SeqCst);
    Pending {
        future: Box::pin(PendingOnce {
            value,
            pending: true,
        }),
        result: None,
    }
}

pub fn poll_future(future: &mut Pending) -> i64 {
    assert!(future.result.is_none(), "do not poll after Ready");
    // This fixture is polled explicitly twice, not scheduled by a reactor.
    match future
        .future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Pending => 0,
        Poll::Ready(value) => {
            future.result = Some(value);
            1
        }
    }
}

pub fn finish_future(future: Pending) -> i64 {
    future.result.expect("must poll to completion")
}

pub fn live_owners() -> i64 {
    (MATCHERS.load(Ordering::SeqCst) + FUTURES.load(Ordering::SeqCst)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_and_future_cleanup() {
        assert_eq!(live_owners(), 0);
        let matcher = new_matcher("a+");
        assert_eq!(match_count(&matcher, "aa é a"), 2);
        assert_eq!(drop_matcher(matcher), 1);
        let mut future = new_future(40);
        assert_eq!(poll_future(&mut future), 0);
        assert_eq!(poll_future(&mut future), 1);
        assert_eq!(finish_future(future), 42);
        drop(new_future(1)); // Cancellation before completion still drops.
        assert_eq!(live_owners(), 0);
        assert_eq!(char_count("é\0ab"), 4);
        assert_eq!(identity::<i64>(41), 41);
    }
}
