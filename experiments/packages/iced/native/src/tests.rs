use super::*;

fn text(token: u64, bytes: &[u8], action: u64) -> u32 {
    unsafe {
        if action == 0 {
            plic_v1_text(token, bytes.as_ptr(), bytes.len())
        } else {
            plic_v1_button(token, bytes.as_ptr(), bytes.len(), action)
        }
    }
}
use plic_v1_column as column;

#[test]
fn copied_utf8_including_nul_and_empty() {
    let scene = build_scene(|token| {
        assert_eq!(column(token), 0);
        let mut bytes = "λ\0🙂".as_bytes().to_vec();
        assert_eq!(text(token, &bytes, 1), 0);
        bytes.fill(0);
        assert_eq!(unsafe { plic_v1_text(token, std::ptr::null(), 0) }, 0);
        assert_eq!(plic_v1_end(token), 0);
        Ok(())
    })
    .unwrap();
    assert!(matches!(&scene.nodes[1].kind, Kind::Button(text, 1) if text == "λ\0🙂"));
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
        assert_eq!(plic_v1_end(token), 0);
        Ok(())
    })
    .unwrap();
    assert_eq!(column(old), Error::InvalidContext as u32);
    build_scene(|token| {
        assert_ne!(old, token);
        assert_eq!(column(old), Error::InvalidContext as u32);
        assert_eq!(column(token), 0);
        assert_eq!(plic_v1_end(token), 0);
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
            plic_v1_end(token);
            column(token);
            Ok(())
        }),
        Err(Error::InvalidLayout)
    );
    assert_eq!(
        build_scene(|token| {
            column(token);
            plic_v1_end(token);
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
fn fixed_child_counts_and_grid_bounds() {
    for n in [99, 401] {
        assert_eq!(
            build_scene(|t| {
                plic_v1_float_scaled(t, n);
                Ok(())
            }),
            Err(Error::InvalidArgument)
        );
    }
    for percent in [0, 100] {
        assert_eq!(
            build_scene(|t| {
                plic_v1_pane_grid(t, true, percent);
                Ok(())
            }),
            Err(Error::InvalidArgument)
        );
    }
    for columns in [0, 257] {
        assert_eq!(
            build_scene(|t| {
                plic_v1_grid(t, columns);
                Ok(())
            }),
            Err(Error::InvalidArgument)
        );
    }
    for (expected, open) in [
        (1, (|t| plic_v1_container(t, 4)) as fn(u64) -> u32),
        (1, |t| plic_v1_float_scaled(t, 100)),
        (1, |t| plic_v1_float_scaled(t, 400)),
        (1, |t| plic_v1_mouse_area(t, 2)),
        (1, |t| plic_v1_sensor(t, 3)),
        (1, |t| plic_v1_themer(t, false)),
        (1, |t| plic_v1_pin(t, 2, 4)),
        (1, |t| plic_v1_scrollable(t, 12)),
        (2, |t| plic_v1_tooltip(t)),
        (2, |t| plic_v1_pane_grid(t, true, 35)),
    ] {
        for count in [expected - 1, expected, expected + 1] {
            let result = build_scene(|t| {
                assert_eq!(plic_v1_grid(t, 2), 0);
                assert_eq!(open(t), 0);
                for _ in 0..count {
                    text(t, b"item", 0);
                }
                plic_v1_end(t);
                plic_v1_end(t);
                Ok(())
            });
            if count == expected {
                assert_eq!(result.unwrap().nodes[1].children.len(), count);
            } else {
                assert_eq!(result, Err(Error::InvalidLayout));
            }
        }
    }
}

#[test]
fn wrapper_actions_share_the_button_namespace() {
    for open in [plic_v1_mouse_area, plic_v1_sensor] {
        for first in [true, false] {
            assert_eq!(
                build_scene(|t| {
                    column(t);
                    if first {
                        open(t, 1);
                        text(t, b"duplicate", 1);
                    } else {
                        text(t, b"one", 1);
                        open(t, 1);
                    }
                    Ok(())
                }),
                Err(Error::InvalidArgument)
            );
        }
        assert_eq!(
            build_scene(|t| {
                open(t, 0);
                Ok(())
            }),
            Err(Error::InvalidArgument)
        );
    }
}

#[test]
fn choice_membership_and_selected_action_are_checked() {
    let choice = |t, a| unsafe { plic_v1_choice(t, b"x".as_ptr(), 1, a) };
    for selected in [0, 1, 2] {
        let scene = build_scene(|t| {
            plic_v1_pick_list(t, selected);
            choice(t, 1);
            plic_v1_end(t);
            Ok(())
        });
        if selected == 2 {
            assert_eq!(scene, Err(Error::InvalidArgument))
        } else {
            assert!(scene.unwrap().has_action(1))
        }
    }
    assert_eq!(
        build_scene(|t| {
            choice(t, 1);
            Ok(())
        }),
        Err(Error::InvalidLayout)
    );
    assert_eq!(
        build_scene(|t| {
            plic_v1_pick_list(t, 0);
            text(t, b"x", 1);
            Ok(())
        }),
        Err(Error::InvalidLayout)
    );
    assert_eq!(
        build_scene(|t| {
            plic_v1_pick_list(t, 0);
            plic_v1_end(t);
            Ok(())
        }),
        Err(Error::InvalidLayout)
    );
}
#[test]
fn canvas_checks_commands_and_dimensions() {
    let rect = |t, rgb| plic_v1_fill_rect(t, 0, 0, 1, 1, rgb);
    assert_eq!(
        build_scene(|t| {
            rect(t, 0);
            Ok(())
        }),
        Err(Error::InvalidLayout)
    );
    assert_eq!(
        build_scene(|t| {
            plic_v1_canvas(t, 20, 20);
            text(t, b"oops", 0);
            Ok(())
        }),
        Err(Error::InvalidLayout)
    );
    assert_eq!(
        build_scene(|t| {
            plic_v1_canvas(t, 20, 20);
            rect(t, 0x1000000);
            Ok(())
        }),
        Err(Error::InvalidArgument)
    );
    assert_eq!(
        build_scene(|t| {
            plic_v1_canvas(t, 2049, 1);
            Ok(())
        }),
        Err(Error::LimitExceeded)
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
        plic_v1_end(token);
        Ok(())
    })
    .unwrap();
}

#[test]
fn rejects_invalid_text_and_zero_action() {
    assert_eq!(
        build_scene(|token| {
            text(token, &[0xff], 0);
            Ok(())
        }),
        Err(Error::InvalidArgument)
    );
    assert_eq!(
        build_scene(|token| {
            unsafe { plic_v1_text(token, std::ptr::null(), 1) };
            Ok(())
        }),
        Err(Error::InvalidArgument)
    );
    assert_eq!(
        build_scene(|token| {
            unsafe { plic_v1_button(token, std::ptr::null(), 0, 0) };
            Ok(())
        }),
        Err(Error::InvalidArgument)
    );
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

#[cfg(feature = "application")]
#[test]
fn callback_failure_precedes_a_native_loop_error_during_exit() {
    assert_eq!(
        loop_outcome(Err(Error::NativeFailure), Some(Error::LimitExceeded)),
        Err(Error::LimitExceeded)
    );
    assert_eq!(
        loop_outcome(Err(Error::NativeFailure), None),
        Err(Error::NativeFailure)
    );
}
