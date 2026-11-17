use super::*;

fn text(token: u64, bytes: &[u8], action: u64) -> u32 {
    unsafe { plgp_v1_text(token, bytes.as_ptr(), bytes.len(), 0xffffff, 20.0, action) }
}
fn column(token: u64) -> u32 {
    plgp_v1_begin(token, 1, 8.0, 8.0, 0x202020)
}

#[test]
fn copied_utf8_including_nul_and_empty() {
    let scene = build_scene(|token| {
        assert_eq!(column(token), 0);
        let mut bytes = "λ\0🙂".as_bytes().to_vec();
        assert_eq!(text(token, &bytes, 1), 0);
        bytes.fill(0);
        assert_eq!(
            unsafe { plgp_v1_text(token, std::ptr::null(), 0, 0, 20.0, 0) },
            0
        );
        assert_eq!(plgp_v1_end(token), 0);
        Ok(())
    })
    .unwrap();
    assert!(matches!(&scene.nodes[1].kind, Kind::Text { text, .. } if text == "λ\0🙂"));
    assert!(scene.has_action(1));
    assert!(!scene.has_action(0));
    assert!(!scene.has_action(2));
}

#[test]
fn phase_thread_and_non_reused_tokens() {
    assert_eq!(column(1), Error::InvalidContext as u32);
    let mut old = 0;
    build_scene(|token| {
        old = token;
        assert_eq!(build_scene(|_| Ok(())), Err(Error::InvalidContext));
        assert_eq!(
            std::thread::spawn(move || column(token)).join().unwrap(),
            Error::InvalidContext as u32
        );
        assert_eq!(column(token), 0);
        assert_eq!(plgp_v1_end(token), 0);
        Ok(())
    })
    .unwrap();
    assert_eq!(column(old), Error::InvalidContext as u32);
    build_scene(|token| {
        assert_ne!(old, token);
        assert_eq!(column(old), Error::InvalidContext as u32);
        assert_eq!(column(token), 0);
        assert_eq!(plgp_v1_end(token), 0);
        Ok(())
    })
    .unwrap();
}

#[test]
fn malformed_layout_and_ignored_errors_discard_frame() {
    assert_eq!(build_scene(|_| Ok(())), Err(Error::InvalidLayout));
    assert_eq!(
        build_scene(|token| {
            column(token);
            Ok(())
        }),
        Err(Error::InvalidLayout)
    );
    assert_eq!(
        build_scene(|token| {
            plgp_v1_end(token);
            column(token);
            Ok(())
        }),
        Err(Error::InvalidLayout)
    );
    assert_eq!(
        build_scene(|token| {
            column(token);
            plgp_v1_end(token);
            column(token);
            Ok(())
        }),
        Err(Error::InvalidLayout)
    );
    assert_eq!(
        build_scene(|token| {
            column(token);
            text(token, b"one", 1);
            text(token, b"duplicate", 1);
            Ok(())
        }),
        Err(Error::InvalidArgument)
    );
}

#[test]
fn node_depth_and_text_limits() {
    assert_eq!(
        build_scene(|token| {
            for _ in 0..MAX_DEPTH {
                assert_eq!(column(token), 0);
            }
            assert_eq!(column(token), Error::LimitExceeded as u32);
            Ok(())
        }),
        Err(Error::LimitExceeded)
    );
    assert_eq!(
        build_scene(|token| {
            assert_eq!(column(token), 0);
            for _ in 1..MAX_NODES {
                assert_eq!(text(token, b"", 0), 0);
            }
            assert_eq!(text(token, b"", 0), Error::LimitExceeded as u32);
            Ok(())
        }),
        Err(Error::LimitExceeded)
    );
    assert_eq!(
        build_scene(|token| {
            text(token, &vec![b'x'; MAX_TEXT + 1], 0);
            Ok(())
        }),
        Err(Error::LimitExceeded)
    );
    assert_eq!(
        build_scene(|token| {
            column(token);
            for _ in 0..MAX_FRAME_TEXT / MAX_TEXT {
                assert_eq!(text(token, &vec![b'x'; MAX_TEXT], 0), 0);
            }
            assert_eq!(text(token, b"x", 0), Error::LimitExceeded as u32);
            Ok(())
        }),
        Err(Error::LimitExceeded)
    );
    // Exact limits are admitted, not merely rejected beyond their boundary.
    build_scene(|token| {
        column(token);
        for _ in 1..MAX_NODES {
            assert_eq!(text(token, b"", 0), 0);
        }
        plgp_v1_end(token);
        Ok(())
    })
    .unwrap();
}

#[test]
fn rejects_bad_numeric_values_and_utf8() {
    for (axis, gap, padding, color) in [
        (2, 0.0, 0.0, 0),
        (0, f32::NAN, 0.0, 0),
        (0, -1.0, 0.0, 0),
        (0, 0.0, f32::INFINITY, 0),
        (0, 0.0, 1025.0, 0),
        (0, 0.0, 0.0, 0x1000000),
    ] {
        assert_eq!(
            build_scene(|token| {
                plgp_v1_begin(token, axis, gap, padding, color);
                Ok(())
            }),
            Err(Error::InvalidArgument)
        );
    }
    assert_eq!(
        build_scene(|token| {
            text(token, &[0xff], 0);
            Ok(())
        }),
        Err(Error::InvalidArgument)
    );
    assert_eq!(
        build_scene(|token| {
            unsafe { plgp_v1_text(token, std::ptr::null(), 1, 0, 20.0, 0) };
            Ok(())
        }),
        Err(Error::InvalidArgument)
    );
    for size in [0.0, 129.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            build_scene(|token| {
                unsafe { plgp_v1_text(token, std::ptr::null(), 0, 0, size, 0) };
                Ok(())
            }),
            Err(Error::InvalidArgument)
        );
    }
}

#[test]
fn scope_clears_on_callback_error_or_panic() {
    assert_eq!(
        build_scene(|token| {
            column(token);
            assert_eq!(
                command(token, |_| panic!("native command")),
                Error::NativeFailure as u32
            );
            Ok(())
        }),
        Err(Error::NativeFailure)
    );
    assert_eq!(
        build_scene(|token| {
            column(token);
            Err(Error::NativeFailure)
        }),
        Err(Error::NativeFailure)
    );
    assert!(std::panic::catch_unwind(|| build_scene(|_| panic!("test callback"))).is_err());
    build_scene(|token| {
        text(token, b"recovered", 0);
        Ok(())
    })
    .unwrap();
}
