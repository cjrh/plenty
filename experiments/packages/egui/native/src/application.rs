//! Single-owner executable adapter. No knowledge of the application's widgets,
//! update rules, or state layout is needed on this side of the ABI.
use crate::{Error, with_ui};
use std::{
    cell::Cell,
    ffi::c_void,
    io::Write,
    num::NonZeroUsize,
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

struct App<'a> {
    state: Owner,
    failure: &'a Cell<Option<Error>>,
}
impl eframe::App for App<'_> {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if self.failure.get().is_none()
            && let Err(e) = self.state.draw(ui)
        {
            self.failure.set(Some(e));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    // A second sizing/discard pass must not replay input against mutated state.
    ctx.options_mut(|options| options.max_passes = NonZeroUsize::MIN);
    ctx
}

fn run() -> Result<(), Error> {
    crate::begin_application()?;
    let mut args = std::env::args_os().skip(1);
    if let Some(mode) = args.next() {
        if mode != "--headless" {
            return Err(Error::InvalidArgument);
        }
        return headless::run(Owner::create()?, args);
    }
    let state = Owner::create()?;
    let failure = Cell::new(None);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([480.0, 280.0]),
        ..Default::default()
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        eframe::run_native_ext(
            "Plenty egui",
            options,
            Some(context()),
            Box::new(|_| {
                Ok(Box::new(App {
                    state,
                    failure: &failure,
                }))
            }),
        )
    }));
    if let Some(e) = failure.get() {
        return Err(e);
    }
    result.map_err(|_| Error::NativeFailure)?.map_err(|e| {
        let _ = writeln!(std::io::stderr().lock(), "eframe: {e}");
        Error::NativeFailure
    })
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

mod headless;
