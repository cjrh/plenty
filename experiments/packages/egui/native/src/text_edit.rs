use super::{Error, MAX_PASS_BYTES, MAX_TEXT, text_widget, widget};
use egui::{Event, Key, TextBuffer, text::CharIndex};
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

// Inspect event strings by reference: TextEdit's filtered event list clones
// them, and single-line paste replaces each CR/LF byte with one space.
fn oversized_paste(ui: &egui::Ui, field: egui::Id, limit: usize) -> bool {
    let focused = ui.memory(|m| m.has_focus(field));
    let under_pointer = ui
        .ctx()
        .interaction_snapshot(|s| s.contains_pointer.contains(&field));
    ui.input(|i| {
        // When a pointer transition can redirect keyboard focus, let TextEdit
        // route the paste and let Draft check it. Never reject a sibling's input.
        let keyboard = focused && !i.pointer.any_pressed() && !i.pointer.any_released();
        i.events.iter().any(|event| match event {
            Event::Paste(text) => keyboard && text.len() > limit,
            Event::MiddleClickPaste { text, .. } => under_pointer && text.len() > limit,
            _ => false,
        })
    })
}

fn submit(response: &egui::Response) -> bool {
    if !response.has_focus() {
        return false;
    }
    // consume_key(NONE, Enter) also accepts Shift/Alt. Consume only bare
    // Enter presses and only for the field that owns focus after interaction.
    let submitted = response.ctx.input_mut(|i| {
        let before = i.events.len();
        i.events.retain(|e| {
            !matches!(e, Event::Key {
            key: Key::Enter, pressed: true, modifiers, ..
        } if modifiers.is_none())
        });
        i.events.len() != before
    });
    if submitted {
        response.surrender_focus();
    }
    submitted
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
            if oversized_paste(ui, field, limit) {
                return Err(Error::LimitExceeded);
            }
            let response = ui.add(
                egui::TextEdit::singleline(&mut draft)
                    .id(field)
                    .return_key(None),
            );
            if let Some(e) = draft.failure {
                return Err(e);
            }
            let submitted = submit(&response);
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
mod tests;
