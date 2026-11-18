use super::*;

fn probe(token: u64) -> u32 {
    unsafe { pleg_v1_label(token, std::ptr::null(), 0) }
}

fn pass(ui: &mut egui::Ui, body: impl FnOnce(u64)) -> Result<(), Error> {
    with_ui(ui, |token| {
        body(token);
        Ok(())
    })
}

#[test]
fn only_the_live_token_authorizes_non_reentrant_calls_on_its_thread() {
    egui::__run_test_ui(|ui| {
        let mut old = 0;
        pass(ui, |token| {
            old = token;
            assert_eq!(probe(0), Error::InvalidContext as u32);
            assert_eq!(
                std::thread::spawn(move || probe(token)).join().unwrap(),
                Error::InvalidContext as u32
            );
            egui::__run_test_ui(|other| {
                assert_eq!(pass(other, |_| unreachable!()), Err(Error::InvalidContext))
            });
            assert_eq!(
                widget(token, |_| {
                    assert_eq!(probe(token), Error::InvalidContext as u32);
                    Ok(false)
                }),
                0
            );
            assert_eq!(probe(token), 0);
        })
        .unwrap();
        assert_eq!(probe(old), Error::InvalidContext as u32);
        pass(ui, |fresh| {
            assert_ne!(old, fresh);
            assert_eq!(probe(old), Error::InvalidContext as u32);
        })
        .unwrap();
    });
}

#[test]
fn widget_validates_lengths_utf8_pointer_and_pass_budgets() {
    egui::__run_test_ui(|ui| {
        for (bytes, len, expected) in [
            (std::ptr::null(), MAX_TEXT + 1, Error::LimitExceeded),
            (std::ptr::null(), 1, Error::InvalidArgument),
            ([0xff].as_ptr(), 1, Error::InvalidArgument),
        ] {
            assert_eq!(
                pass(ui, |t| {
                    assert_eq!(unsafe { pleg_v1_button(t, bytes, len) }, expected as u32);
                    assert_eq!(probe(t), expected as u32); // first error is sticky
                }),
                Err(expected)
            );
        }
        assert_eq!(
            pass(ui, |t| {
                for _ in 0..MAX_WIDGETS {
                    assert_eq!(probe(t), 0);
                }
                assert_eq!(probe(t), Error::LimitExceeded as u32);
            }),
            Err(Error::LimitExceeded)
        );
        let bytes = [b' '; MAX_TEXT];
        assert_eq!(
            pass(ui, |t| {
                for _ in 0..MAX_PASS_BYTES / MAX_TEXT {
                    assert_eq!(unsafe { pleg_v1_label(t, bytes.as_ptr(), MAX_TEXT) }, 0);
                }
                assert_eq!(
                    unsafe { pleg_v1_button(t, bytes.as_ptr(), 1) },
                    Error::LimitExceeded as u32
                );
            }),
            Err(Error::LimitExceeded)
        );
    });
}

#[test]
fn unwind_releases_loan_and_widget_panic_returns_native_failure() {
    egui::__run_test_ui(|ui| {
        let mut old = 0;
        let result = catch_unwind(AssertUnwindSafe(|| {
            pass(ui, |t| {
                old = t;
                panic!("scope");
            })
        }));
        assert!(result.is_err());
        assert_eq!(probe(old), Error::InvalidContext as u32);
        assert_eq!(
            pass(ui, |t| {
                assert_eq!(widget(t, |_| panic!("widget")), Error::NativeFailure as u32);
            }),
            Err(Error::NativeFailure)
        );
        pass(ui, |t| assert_eq!(probe(t), 0)).unwrap();
    });
}

#[test]
fn checked_draw_errors_are_not_native_failures() {
    assert_eq!(draw_result(0, u32::MAX), Ok(()));
    for (tag, expected) in [
        Error::OutOfMemory,
        Error::CapacityOverflow,
        Error::InvalidContext,
        Error::LimitExceeded,
        Error::InvalidArgument,
        Error::NativeFailure,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(draw_result(1, tag as u32), Err(expected));
    }
    assert_eq!(draw_result(2, 0), Err(Error::NativeFailure));
}
