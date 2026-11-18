//! Immediate-mode egui calls. A token is not an address: it authorizes a
//! synchronous reborrow of this thread's current pass, never a later one.
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_WIDGETS: usize = 256;
const MAX_TEXT: usize = 4096;
const MAX_PASS_BYTES: usize = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum Error {
    OutOfMemory = 2,
    CapacityOverflow,
    InvalidContext,
    LimitExceeded,
    InvalidArgument,
    NativeFailure,
}

#[cfg(any(feature = "application", test))]
fn draw_result(status: u32, tag: u32) -> Result<(), Error> {
    match (status, tag) {
        (0, _) => Ok(()),
        (1, 0) => Err(Error::OutOfMemory),
        (1, 1) => Err(Error::CapacityOverflow),
        (1, 2) => Err(Error::InvalidContext),
        (1, 3) => Err(Error::LimitExceeded),
        (1, 4) => Err(Error::InvalidArgument),
        _ => Err(Error::NativeFailure),
    }
}

struct Pass {
    token: u64,
    ui: NonNull<egui::Ui>,
    widgets: usize,
    bytes: usize,
    failure: Option<Error>,
    edited: Option<String>,
}
impl Pass {
    fn ui(&mut self) -> &mut egui::Ui {
        // with_ui's scope keeps the original Ui alive. The TLS exclusive
        // borrow covers the entire widget operation and rejects reentry.
        unsafe { self.ui.as_mut() }
    }
}
thread_local! { static ACTIVE: RefCell<Option<Pass>> = const { RefCell::new(None) }; }
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Scope;
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with_borrow_mut(|slot| *slot = None);
    }
}

fn with_ui(ui: &mut egui::Ui, draw: impl FnOnce(u64) -> Result<(), Error>) -> Result<(), Error> {
    let token = ACTIVE.with(|slot| {
        let mut slot = slot.try_borrow_mut().map_err(|_| Error::InvalidContext)?;
        if slot.is_some() {
            return Err(Error::InvalidContext);
        }
        let token = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::CapacityOverflow)?;
        *slot = Some(Pass {
            token,
            ui: NonNull::from(ui),
            widgets: 0,
            bytes: 0,
            failure: None,
            edited: None,
        });
        Ok(token)
    })?;
    let _clear = Scope;
    draw(token)?;
    ACTIVE.with_borrow(|slot| slot.as_ref().unwrap().failure.map_or(Ok(()), Err))
}

fn widget(token: u64, add: impl FnOnce(&mut Pass) -> Result<bool, Error>) -> u32 {
    let result = ACTIVE.with(|slot| {
        let mut slot = slot.try_borrow_mut().map_err(|_| Error::InvalidContext)?;
        let pass = slot
            .as_mut()
            .filter(|p| p.token == token)
            .ok_or(Error::InvalidContext)?;
        if let Some(e) = pass.failure {
            return Err(e);
        }
        catch_unwind(AssertUnwindSafe(|| add(pass)))
            .unwrap_or(Err(Error::NativeFailure))
            .inspect_err(|&e| pass.failure = Some(e))
    });
    result.map_or_else(|e| e as u32, u32::from)
}

mod text_edit;

// SAFETY: non-null input points to `len` readable bytes for this call.
unsafe fn text_widget(
    token: u64,
    text: *const u8,
    len: usize,
    add: impl FnOnce(&mut Pass, &str) -> Result<bool, Error>,
) -> u32 {
    widget(token, |pass| {
        if pass.widgets == MAX_WIDGETS || len > MAX_TEXT || pass.bytes + len > MAX_PASS_BYTES {
            return Err(Error::LimitExceeded);
        }
        let bytes = if len == 0 {
            &[]
        } else if text.is_null() {
            return Err(Error::InvalidArgument);
        } else {
            unsafe { std::slice::from_raw_parts(text, len) }
        };
        let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidArgument)?;
        pass.widgets += 1;
        pass.bytes += len;
        add(pass, text)
    })
}
#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_label(token: u64, text: *const u8, len: usize) -> u32 {
    unsafe {
        text_widget(token, text, len, |pass, text| {
            pass.ui().label(text);
            Ok(false)
        })
    }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn pleg_v1_button(token: u64, text: *const u8, len: usize) -> u32 {
    unsafe {
        text_widget(token, text, len, |pass, text| {
            Ok(pass.ui().button(text).clicked())
        })
    }
}

#[cfg(all(
    feature = "application",
    not(feature = "legacy-application"),
    not(test)
))]
mod application;
#[cfg(feature = "application")]
mod driver;
#[cfg(all(feature = "legacy-application", not(test)))]
mod legacy_application;
#[cfg(feature = "application")]
static MAIN_THREAD: std::sync::OnceLock<std::thread::ThreadId> = std::sync::OnceLock::new();

#[cfg(feature = "application")]
fn begin_application() -> Result<(), Error> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if MAIN_THREAD.get() != Some(&std::thread::current().id())
        || STARTED.swap(true, Ordering::AcqRel)
    {
        return Err(Error::InvalidContext);
    }
    Ok(())
}

#[cfg(all(test, feature = "application"))]
#[test]
fn application_only_enters_on_registered_thread_once() {
    MAIN_THREAD.set(std::thread::current().id()).unwrap();
    assert_eq!(
        std::thread::spawn(begin_application).join().unwrap(),
        Err(Error::InvalidContext)
    );
    assert_eq!(begin_application(), Ok(()));
    assert_eq!(begin_application(), Err(Error::InvalidContext));
}

#[cfg(all(feature = "application", not(test)))]
#[unsafe(no_mangle)]
unsafe extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    MAIN_THREAD
        .set(std::thread::current().id())
        .expect("one process entry");
    // SAFETY: the native process entry supplies valid process-lifetime argv.
    unsafe { plenty_runtime::main(argc, argv) }
}

#[cfg(test)]
mod tests;
