//! Compiler phase profile, built only with the `profile` feature (issue #150).
//! See book/src/development/profiling.md for how to run and read it.
//!
//! `cargo test --all-features` builds this feature too, so a profile is
//! recorded only when `PLENTY_PROFILE` is set: without a guard the measured
//! functions record nothing and print nothing.

/// Start recording when `PLENTY_PROFILE` is set. The report is written when
/// the returned guard is dropped: to stdout, or to `HOTPATH_OUTPUT_PATH`.
///
/// # Safety
/// Call before starting threads: enabling profiling may set an environment
/// variable, which must not race with other threads reading the environment.
pub unsafe fn start() -> Option<hotpath::HotpathGuard> {
    std::env::var_os("PLENTY_PROFILE")?;
    // hotpath serves live metrics over HTTP on port 6770 by default; a
    // compiler run has no use for a listening socket. Called before any
    // thread starts, so setting the variable races nothing.
    if std::env::var_os("HOTPATH_METRICS_SERVER_OFF").is_none() {
        // SAFETY: the caller guarantees single-threaded startup.
        unsafe { std::env::set_var("HOTPATH_METRICS_SERVER_OFF", "true") };
    }
    Some(
        hotpath::HotpathGuardBuilder::new("main")
            .percentiles(&[50.0, 95.0])
            .functions_limit(0)
            .build(),
    )
}
