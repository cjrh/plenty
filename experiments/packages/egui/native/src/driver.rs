//! Shared native and display-free loops. The state is scoped to this invocation.
use crate::Error;
use std::{
    cell::Cell,
    io::Write,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
};

pub(crate) trait Draw {
    fn draw(&mut self, root: &mut egui::Ui) -> Result<(), Error>;
}

struct App<'a, D> {
    state: D,
    failure: &'a Cell<Option<Error>>,
}
impl<D: Draw> eframe::App for App<'_, D> {
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

pub(crate) fn run_admitted<D: Draw>(
    mut args: impl Iterator<Item = std::ffi::OsString>,
    create: impl FnOnce() -> Result<D, Error>,
) -> Result<(), Error> {
    if let Some(mode) = args.next() {
        if mode != "--headless" {
            return Err(Error::InvalidArgument);
        }
        return headless::run(create()?, args);
    }
    let state = create()?;
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

mod headless;

#[cfg(test)]
mod callback_tests {
    use super::*;
    use std::ffi::OsString;

    struct Button<'a> {
        clicks: &'a Cell<usize>,
        drops: &'a Cell<usize>,
    }
    impl Drop for Button<'_> {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    impl Draw for Button<'_> {
        fn draw(&mut self, root: &mut egui::Ui) -> Result<(), Error> {
            egui::CentralPanel::default().show(root, |ui| {
                if ui.button("Approve").clicked() {
                    self.clicks.set(self.clicks.get() + 1);
                }
            });
            Ok(())
        }
    }

    #[test]
    fn invalid_mode_skips_factory() {
        let result = run_admitted(
            [OsString::from("invalid")].into_iter(),
            || -> Result<Button<'static>, Error> { panic!("invalid mode called the factory") },
        );
        assert_eq!(result, Err(Error::InvalidArgument));
    }

    #[test]
    fn scoped_draw_survives_redraw_and_drops_once() {
        let clicks = Cell::new(0);
        let drops = Cell::new(0);
        let create = || {
            Ok(Button {
                clicks: &clicks,
                drops: &drops,
            })
        };
        let args = ["--headless", "20,15"].map(OsString::from);
        run_admitted(args.into_iter(), create).unwrap();
        assert_eq!((clicks.get(), drops.get()), (1, 1));
    }
}
