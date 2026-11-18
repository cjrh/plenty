//! Single-owner executable adapter. No knowledge of the application's widgets,
//! update rules, or state layout is needed on this side of the ABI.
use crate::{
    Error,
    driver::{Draw, run_admitted},
    with_ui,
};
use std::{
    ffi::c_void,
    io::Write,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
};

unsafe extern "C" {
    fn pleg_app_create(error: *mut u32) -> *mut c_void;
    fn pleg_app_draw(state: *mut c_void, token: u64, error: *mut u32) -> u32;
    fn pleg_app_destroy(state: *mut c_void);
}

struct Owner(NonNull<c_void>); // Neither Send nor Sync.
impl Drop for Owner {
    fn drop(&mut self) {
        unsafe { pleg_app_destroy(self.0.as_ptr()) }
    }
}
impl Owner {
    fn create() -> Result<Self, Error> {
        let mut tag = u32::MAX;
        NonNull::new(unsafe { pleg_app_create(&mut tag) })
            .map(Self)
            .ok_or(match tag {
                0 => Error::OutOfMemory,
                1 => Error::CapacityOverflow,
                _ => Error::NativeFailure,
            })
    }
}

impl Draw for Owner {
    fn draw(&mut self, root: &mut egui::Ui) -> Result<(), Error> {
        egui::CentralPanel::default()
            .show(root, |ui| {
                with_ui(ui, |token| {
                    let mut tag = u32::MAX;
                    let status = unsafe { pleg_app_draw(self.0.as_ptr(), token, &mut tag) };
                    crate::draw_result(status, tag)
                })
            })
            .inner
    }
}

fn run() -> Result<(), Error> {
    crate::begin_application()?;
    run_admitted(std::env::args_os().skip(1), Owner::create)
}

#[unsafe(no_mangle)]
extern "C" fn pleg_v1_run() -> u32 {
    catch_unwind(AssertUnwindSafe(|| match run() {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(std::io::stderr().lock(), "egui run: {e:?}");
            e as u32
        }
    }))
    .unwrap_or(Error::NativeFailure as u32)
}
