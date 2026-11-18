use super::*;
use crate::with_ui;
use egui::{Event, Key, Modifiers, RawInput};

fn press(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        modifiers,
        pressed: true,
        physical_key: None,
        repeat: false,
    }
}

#[test]
fn caret_selection_and_submission_follow_the_field_not_the_widget_ordinal() {
    let ctx = egui::Context::default();
    ctx.options_mut(|o| o.max_passes = std::num::NonZeroUsize::MIN);
    let mut model = String::from("éA猫!");
    let mut redraw = 0;
    let mut frame = |events, focus: bool| {
        let mut status = 99;
        let mut count = -99;
        let mut output = ctx.run_ui(
            RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                if redraw % 2 == 0 {
                    ui.label("other widget");
                }
                if focus {
                    let id = ui.make_persistent_id(("plenty.text_edit", 7u64));
                    ui.memory_mut(|m| m.request_focus(id));
                }
                let result = with_ui(ui, |token| {
                    status = unsafe {
                        pleg_v1_text_edit(token, 7, model.as_ptr(), model.len(), &mut count)
                    };
                    if count >= 0 {
                        // The byte adapter is strided, not a Rust Vec or C string.
                        let mut out = vec![0xFF; 2 * count as usize];
                        assert_eq!(
                            unsafe {
                                pleg_v1_copy_edit(token, out.as_mut_ptr(), count as usize, 2)
                            },
                            0
                        );
                        assert!(out.iter().skip(1).step_by(2).all(|b| *b == 0xFF));
                        model = String::from_utf8(out.into_iter().step_by(2).collect()).unwrap();
                    }
                    Ok(())
                });
                assert_eq!(result.is_err(), status > 1);
            },
        );
        output.textures_delta.clear();
        redraw += 1;
        (model.clone(), status, count)
    };
    frame(vec![], true);
    frame(
        vec![
            press(Key::End, Modifiers::NONE),
            press(Key::ArrowLeft, Modifiers::NONE),
        ],
        false,
    );
    frame(vec![], false);
    frame(vec![press(Key::ArrowLeft, Modifiers::SHIFT)], false);
    frame(vec![], false);
    // TextEdit replaces the selected 猫, rather than appending or splitting UTF-8.
    assert_eq!(
        frame(vec![Event::Text("界".into())], false),
        ("éA界!".into(), 0, 7)
    );
    assert_eq!(frame(vec![press(Key::Enter, Modifiers::NONE)], false).1, 1);
    for _ in 0..4 {
        assert_eq!(frame(vec![], false), ("éA界!".into(), 0, -1));
    }
    frame(vec![], true);
    frame(
        vec![
            press(Key::End, Modifiers::NONE),
            press(Key::ArrowLeft, Modifiers::SHIFT),
        ],
        false,
    );
    // egui deletes the selection before the insertion fails. No changed
    // extent (not even zero) may escape to trigger a partial writeback.
    assert_eq!(
        frame(vec![Event::Paste("é".repeat(MAX_TEXT / 2))], false),
        ("éA界!".into(), Error::LimitExceeded as u32, -99)
    );
}

fn frame(ctx: &egui::Context, events: Vec<Event>, draw: impl FnMut(&mut egui::Ui)) {
    ctx.run_ui(
        RawInput {
            events,
            ..Default::default()
        },
        draw,
    )
    .textures_delta
    .clear();
}

fn field(ui: &egui::Ui, id: u64) -> egui::Id {
    ui.make_persistent_id(("plenty.text_edit", id))
}

fn edit(token: u64, id: u64, input: &str, count: &mut i64) -> u32 {
    unsafe { pleg_v1_text_edit(token, id, input.as_ptr(), input.len(), count) }
}

#[test]
fn enter_on_new_focus_submits_and_surrenders_once() {
    let ctx = egui::Context::default();
    frame(&ctx, vec![press(Key::Enter, Modifiers::NONE)], |ui| {
        let id = field(ui, 1);
        ui.memory_mut(|m| m.request_focus(id));
        with_ui(ui, |t| {
            assert_eq!(edit(t, 2, "", &mut -9), 0); // don't steal Enter
            assert_eq!(edit(t, 1, "", &mut -9), 1);
            Ok(())
        })
        .unwrap();
        assert!(!ui.memory(|m| m.has_focus(id)));
        assert!(!ui.input(|i| i.key_pressed(Key::Enter)));
    });
}

#[test]
fn modified_enter_and_click_elsewhere_is_not_submission() {
    let ctx = egui::Context::default();
    frame(&ctx, vec![], |ui| {
        let id = field(ui, 1);
        ui.memory_mut(|m| m.request_focus(id));
        with_ui(ui, |t| {
            assert_eq!(edit(t, 1, "", &mut -9), 0);
            Ok(())
        })
        .unwrap();
    });
    let pos = egui::pos2(450., 250.);
    let mut events = vec![press(Key::Enter, Modifiers::CTRL), Event::PointerMoved(pos)];
    for pressed in [true, false] {
        events.push(Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: Modifiers::CTRL,
        });
    }
    frame(&ctx, events, |ui| {
        let id = field(ui, 1);
        with_ui(ui, |t| {
            assert_eq!(edit(t, 1, "", &mut -9), 0);
            Ok(())
        })
        .unwrap();
        assert!(!ui.memory(|m| m.has_focus(id)));
        assert!(ui.input(|i| i.key_pressed(Key::Enter)));
    });
}

#[test]
fn modified_enter_does_not_dismiss_the_focused_field() {
    let ctx = egui::Context::default();
    for modifiers in [Modifiers::SHIFT, Modifiers::ALT, Modifiers::CTRL] {
        frame(&ctx, vec![press(Key::Enter, modifiers)], |ui| {
            let id = field(ui, 1);
            ui.memory_mut(|m| m.request_focus(id));
            with_ui(ui, |t| {
                assert_eq!(edit(t, 1, "a", &mut -9), 0);
                Ok(())
            })
            .unwrap();
            assert!(ui.memory(|m| m.has_focus(id)));
            assert!(ui.input(|i| i.key_pressed(Key::Enter)));
        });
    }
}

#[test]
fn paste_only_preflights_the_recipient() {
    for middle in [false, true] {
        let ctx = egui::Context::default();
        let mut second = egui::Pos2::ZERO;
        frame(&ctx, vec![], |ui| {
            let id = field(ui, 2);
            with_ui(ui, |t| {
                assert_eq!(edit(t, 1, "a", &mut -9), 0);
                assert_eq!(edit(t, 2, "b", &mut -9), 0);
                Ok(())
            })
            .unwrap();
            second = ui.ctx().read_response(id).unwrap().rect.center();
        });
        let text = "x".repeat(MAX_TEXT + 1);
        let events = if middle {
            vec![
                Event::PointerMoved(second),
                Event::MiddleClickPaste { pos: second, text },
            ]
        } else {
            vec![Event::Paste(text)]
        };
        frame(&ctx, events, |ui| {
            let target = field(ui, if middle { 1 } else { 2 });
            ui.memory_mut(|m| m.request_focus(target));
            assert!(!oversized_paste(ui, field(ui, 1), MAX_TEXT));
            assert!(oversized_paste(ui, field(ui, 2), MAX_TEXT));
            let result = with_ui(ui, |t| {
                let mut count = -9;
                assert_eq!(edit(t, 1, "a", &mut count), 0);
                assert_eq!(count, -1);
                assert_eq!(edit(t, 2, "b", &mut count), Error::LimitExceeded as u32);
                assert_eq!(count, -1);
                Ok(())
            });
            assert_eq!(result, Err(Error::LimitExceeded));
        });
    }
}

#[test]
fn deletion_to_empty_is_a_replacement_not_an_idle_frame() {
    frame(
        &egui::Context::default(),
        vec![press(Key::Backspace, Modifiers::NONE)],
        |ui| {
            let id = field(ui, 1);
            ui.memory_mut(|m| m.request_focus(id));
            with_ui(ui, |t| {
                let mut count = -9;
                assert_eq!(edit(t, 1, "猫", &mut count), 0);
                assert_eq!(count, 0);
                assert_eq!(
                    unsafe { pleg_v1_copy_edit(t, std::ptr::null_mut(), 0, 1) },
                    0
                );
                Ok(())
            })
            .unwrap();
        },
    );
}

#[test]
fn paste_normalizes_newlines_and_respects_remaining_pass_bytes() {
    for (paste, allowed) in [("\r\n", true), ("猫", false)] {
        frame(
            &egui::Context::default(),
            vec![Event::Paste(paste.into())],
            |ui| {
                let id = field(ui, 1);
                ui.memory_mut(|m| m.request_focus(id));
                let result = with_ui(ui, |t| {
                    let filler = [b' '; MAX_TEXT];
                    for chunk in (0..MAX_PASS_BYTES - 3).step_by(MAX_TEXT) {
                        let len = MAX_TEXT.min(MAX_PASS_BYTES - 3 - chunk);
                        assert_eq!(unsafe { crate::pleg_v1_label(t, filler.as_ptr(), len) }, 0);
                    }
                    let mut count = -9;
                    let status = edit(t, 1, "a", &mut count);
                    if allowed {
                        assert_eq!((status, count), (0, 3));
                        let mut out = [0; 3];
                        assert_eq!(
                            unsafe { pleg_v1_copy_edit(t, out.as_mut_ptr(), out.len(), 1) },
                            0
                        );
                        assert_eq!(&out, b"a  ");
                    } else {
                        assert_eq!((status, count), (Error::LimitExceeded as u32, -9));
                    }
                    Ok(())
                });
                assert_eq!(
                    result,
                    if allowed {
                        Ok(())
                    } else {
                        Err(Error::LimitExceeded)
                    }
                );
            },
        );
    }
}

#[test]
fn ignored_copyback_rejection_is_sticky_in_the_real_pass() {
    frame(
        &egui::Context::default(),
        vec![Event::Text("b".into())],
        |ui| {
            let id = field(ui, 1);
            ui.memory_mut(|m| m.request_focus(id));
            let result = with_ui(ui, |t| {
                let mut count = -9;
                assert_eq!(edit(t, 1, "a", &mut count), 0);
                assert_eq!(count, 2);
                // The std wrapper reports failure to allocate the destination. Even
                // if the application ignores Err, no later widget or copy succeeds.
                pleg_v1_reject_edit(t, Error::OutOfMemory as u32);
                let mut out = [0; 2];
                assert_eq!(
                    unsafe { pleg_v1_copy_edit(t, out.as_mut_ptr(), 2, 1) },
                    Error::OutOfMemory as u32
                );
                assert_eq!(out, [0; 2]);
                Ok(())
            });
            assert_eq!(result, Err(Error::OutOfMemory));
        },
    );
}
