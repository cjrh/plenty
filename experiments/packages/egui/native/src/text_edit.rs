use super::{Error, MAX_PASS_BYTES, MAX_TEXT, text_widget, widget};
use egui::{TextBuffer, text::CharIndex};
use std::{any::TypeId, ops::Range};

// A deletion followed by a rejected insertion (paste over a selection) must
// not expose the deletion to Plenty. All editor operations target this draft.
struct Draft {
    text: String,
    limit: usize,
    failure: Option<Error>,
}

impl TextBuffer for Draft {
    fn as_str(&self) -> &str {
        &self.text
    }
    fn is_mutable(&self) -> bool {
        true
    }
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
    fn insert_text(&mut self, text: &str, at: CharIndex) -> usize {
        let error = if text.len() > self.limit - self.text.len() {
            Error::LimitExceeded
        } else if text.contains(['\r', '\n']) {
            Error::InvalidArgument
        } else if self.text.try_reserve_exact(text.len()).is_err() {
            // The bounded extent fits usize and isize.
            Error::OutOfMemory
        } else {
            return self.text.insert_text(text, at);
        };
        self.failure.get_or_insert(error);
        0
    }
    fn delete_char_range(&mut self, range: Range<CharIndex>) {
        self.text.delete_char_range(range);
    }
}

// SAFETY: same input span contract as label. `count` is a disjoint writable
// i64. -1 means unchanged; zero means a replacement with the empty string.
#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_text_edit(
    token: u64,
    id: u64,
    input: *const u8,
    len: usize,
    count: *mut i64,
) -> u32 {
    unsafe {
        text_widget(token, input, len, |pass, original| {
            if count.is_null() {
                return Err(Error::InvalidArgument);
            }
            let limit = MAX_TEXT.min(MAX_PASS_BYTES - pass.bytes + len);
            let mut draft = Draft {
                text: String::new(),
                limit,
                failure: None,
            };
            draft.insert_text(original, CharIndex(0));
            if let Some(e) = draft.failure {
                return Err(e);
            }
            let ui = pass.ui();
            let field = ui.make_persistent_id(("plenty.text_edit", id));
            let focused = ui.memory(|m| m.has_focus(field));
            let response = ui.add(egui::TextEdit::singleline(&mut draft).id(field));
            if let Some(e) = draft.failure {
                return Err(e);
            }
            let submitted =
                focused && response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let changed = response.changed() && draft.text != original;
            let size = if changed { draft.text.len() as i64 } else { -1 };
            pass.bytes += draft.text.len().saturating_sub(len);
            pass.edited = changed.then_some(draft.text);
            count.write(size);
            Ok(submitted)
        })
    }
}

// SAFETY: the strided adapter lends `len` writable u8 elements with this
// spacing. No padding or list metadata is read or written.
#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_copy_edit(token: u64, out: *mut u8, len: usize, stride: usize) -> u32 {
    widget(token, |pass| {
        let text = pass.edited.take().ok_or(Error::InvalidContext)?;
        if text.len() != len || (len != 0 && (out.is_null() || stride == 0)) {
            return Err(Error::InvalidArgument);
        }
        for (i, byte) in text.bytes().enumerate() {
            unsafe { out.add(i * stride).write(byte) };
        }
        Ok(false)
    })
}

#[unsafe(no_mangle)]
extern "C" fn pleg_v1_reject_edit(token: u64, status: u32) -> u32 {
    widget(token, |_| {
        Err(match status {
            2 => Error::OutOfMemory,
            3 => Error::CapacityOverflow,
            _ => Error::NativeFailure,
        })
    })
}

#[cfg(test)]
mod tests {
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
                            model =
                                String::from_utf8(out.into_iter().step_by(2).collect()).unwrap();
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
            frame(vec![Event::Paste("é".repeat(MAX_TEXT))], false),
            ("éA界!".into(), Error::LimitExceeded as u32, -99)
        );
    }
}
