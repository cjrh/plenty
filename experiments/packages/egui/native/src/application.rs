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
    fn pleg_app_draw(state: *mut c_void, token: u64, error: *mut u32) -> u32;
}

// Only borrowed for the synchronous loop; Plenty owns the box and its callback.
struct Driver(NonNull<c_void>);
impl Draw for Driver {
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

#[unsafe(no_mangle)]
extern "C" fn pleg_v2_admit() -> u32 {
    status(|| {
        crate::begin_application()?;
        if std::env::args_os()
            .nth(1)
            .is_some_and(|mode| mode != "--headless")
        {
            return Err(Error::InvalidArgument);
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
extern "C" fn pleg_v2_drive(driver: *mut c_void) -> u32 {
    status(|| {
        run_admitted(std::env::args_os().skip(1), || {
            NonNull::new(driver)
                .map(Driver)
                .ok_or(Error::InvalidArgument)
        })
    })
}

fn status(f: impl FnOnce() -> Result<(), Error>) -> u32 {
    catch_unwind(AssertUnwindSafe(|| match f() {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(std::io::stderr().lock(), "egui run: {e:?}");
            e as u32
        }
    }))
    .unwrap_or(Error::NativeFailure as u32)
}
