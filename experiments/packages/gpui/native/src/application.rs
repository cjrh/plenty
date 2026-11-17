//! Single-threaded GPUI host. All Plenty entry/destruction stays on this thread.
use crate::{Error, Kind, Scene, build_scene};
use gpui::{
    App, Application, Bounds, Context, TitlebarOptions, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};
use std::{cell::Cell, marker::PhantomData, ptr::NonNull, rc::Rc};

unsafe extern "C" {
    fn plgp_app_create(error: *mut u32) -> *mut std::ffi::c_void;
    fn plgp_app_render(
        state: *const std::ffi::c_void,
        token: u64,
        begin: *mut std::ffi::c_void,
        end: *mut std::ffi::c_void,
        text: *mut std::ffi::c_void,
        error: *mut u32,
    ) -> u32;
    fn plgp_app_event(state: *mut std::ffi::c_void, action: u64, error: *mut u32) -> u32;
    fn plgp_app_destroy(state: *mut std::ffi::c_void);
}

#[derive(Debug)]
enum AppError {
    Factory(u32),
    Render(Error),
    Event(Error),
    UnknownAction(u64),
    BadAction,
    BadArguments,
    Unsupported,
    Failed,
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Factory(code) => write!(f, "Plenty create failed: AllocError code {code}"),
            Self::Render(error) => write!(f, "Plenty render failed: {error:?}"),
            Self::Event(error) => write!(f, "Plenty event failed: {error:?}"),
            Self::UnknownAction(action) => write!(f, "unknown action ID: {action}"),
            Self::BadAction => f.write_str("expected numeric action ID"),
            Self::BadArguments => f.write_str("usage: plenty-gpui [--headless [ACTION...]]"),
            Self::Unsupported => f.write_str("qualified only for Linux x86-64"),
            Self::Failed => f.write_str("application failed"),
        }
    }
}

// A unique originating owner, intentionally !Send and !Sync. GPUI listeners hold
// only weak entity references, never this pointer or an outstanding Plenty loan.
struct StateOwner {
    pointer: NonNull<std::ffi::c_void>,
    _thread_bound: PhantomData<Rc<()>>,
}
impl StateOwner {
    fn create() -> Result<Self, AppError> {
        let mut error = 0;
        let pointer = unsafe { plgp_app_create(&mut error) };
        Ok(Self {
            pointer: NonNull::new(pointer).ok_or(AppError::Factory(error))?,
            _thread_bound: PhantomData,
        })
    }
    fn render(&self) -> Result<Scene, AppError> {
        build_scene(|token| {
            let mut error = 0;
            let status = unsafe {
                plgp_app_render(
                    self.pointer.as_ptr(),
                    token,
                    crate::plgp_v1_begin as *const () as *mut std::ffi::c_void,
                    crate::plgp_v1_end as *const () as *mut std::ffi::c_void,
                    crate::plgp_v1_text as *const () as *mut std::ffi::c_void,
                    &mut error,
                )
            };
            callback_status(status, error)
        })
        .map_err(AppError::Render)
    }
    fn event(&mut self, action: u64) -> Result<(), AppError> {
        let mut error = 0;
        let status = unsafe { plgp_app_event(self.pointer.as_ptr(), action, &mut error) };
        callback_status(status, error).map_err(AppError::Event)
    }
}
impl Drop for StateOwner {
    fn drop(&mut self) {
        unsafe { plgp_app_destroy(self.pointer.as_ptr()) };
    }
}

fn callback_status(status: u32, error: u32) -> Result<(), Error> {
    if status == 0 {
        return Ok(());
    }
    Err(match (status, error) {
        (1, 0) => Error::OutOfMemory,
        (1, 1) => Error::CapacityOverflow,
        (1, 2) => Error::InvalidContext,
        (1, 3) => Error::LimitExceeded,
        (1, 4) => Error::InvalidLayout,
        (1, 5) => Error::InvalidArgument,
        _ => Error::NativeFailure,
    })
}

// GPUI 0.2.2's X11 close callback still borrows its platform client. Calling
// quit inline reborrows it and panics; wait until the platform dispatch returns.
fn request_quit(cx: &App) {
    cx.spawn(async |cx| {
        gpui::Timer::after(std::time::Duration::from_millis(1)).await;
        let _ = cx.update(|cx| cx.quit());
    })
    .detach();
}

thread_local! {
    static FAILED: Cell<bool> = const { Cell::new(false) };
}

struct View {
    state: StateOwner,
    scene: Scene,
    stopped: bool,
}
impl View {
    fn stop(&mut self, error: AppError, cx: &mut Context<Self>) {
        self.stopped = true;
        FAILED.set(true);
        eprintln!("plenty-gpui: {error}");
        request_quit(cx);
    }
    fn node(&self, index: usize, cx: &Context<Self>) -> gpui::AnyElement {
        let node = &self.scene.nodes[index];
        match &node.kind {
            Kind::Row | Kind::Column => {
                let mut element = div()
                    .flex()
                    .gap(px(node.gap))
                    .p(px(node.padding))
                    .bg(rgb(node.background));
                if matches!(node.kind, Kind::Column) {
                    element = element.flex_col();
                }
                for &child in &node.children {
                    element = element.child(self.node(child, cx));
                }
                element.into_any_element()
            }
            Kind::Text {
                text,
                color,
                size,
                action,
            } => {
                let element = div()
                    .text_color(rgb(*color))
                    .text_size(px(*size))
                    .child(text.clone());
                if *action == 0 {
                    return element.into_any_element();
                }
                let action = *action;
                element
                    .id(("action", action))
                    .cursor_pointer()
                    .p(px(8.0))
                    .bg(rgb(0x404040))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if view.stopped || !view.scene.has_action(action) {
                            return;
                        }
                        // No shared render borrow remains; mutation ends before
                        // the next render, and GPUI is notified only after return.
                        if let Err(error) = view.state.event(action) {
                            view.stop(error, cx);
                        } else {
                            cx.notify();
                        }
                    }))
                    .into_any_element()
            }
        }
    }
}
impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.stopped {
            match self.state.render() {
                Ok(scene) => self.scene = scene,
                Err(error) => self.stop(error, cx),
            }
        }
        let mut root = div().size_full().bg(rgb(0x202020));
        if !self.scene.nodes.is_empty() {
            root = root.child(self.node(0, cx));
        }
        root
    }
}

fn headless(mut state: StateOwner, actions: impl Iterator<Item = String>) -> Result<(), AppError> {
    fn show(scene: &Scene) {
        for node in &scene.nodes {
            if let Kind::Text { text, .. } = &node.kind {
                println!("{text}");
            }
        }
        println!("---");
    }
    let mut scene = state.render()?;
    show(&scene);
    for action in actions {
        let action = action.parse::<u64>().map_err(|_| AppError::BadAction)?;
        if !scene.has_action(action) {
            return Err(AppError::UnknownAction(action));
        }
        state.event(action)?;
        scene = state.render()?;
        show(&scene);
    }
    Ok(())
}

fn run() -> Result<(), AppError> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err(AppError::Unsupported);
    }
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--headless") => return headless(StateOwner::create()?, args),
        Some(_) => return Err(AppError::BadArguments),
        None => {}
    }
    let state = StateOwner::create()?;
    // Fail before opening a native window on malformed initial render.
    let scene = state.render()?;
    FAILED.set(false);
    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                request_quit(cx);
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(480.0), px(280.0)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Plenty GPUI".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                if let Some(gpu) = window.gpu_specs() {
                    eprintln!("plenty-gpui: renderer {gpu:?}");
                }
                cx.new(|_| View {
                    state,
                    scene,
                    stopped: false,
                })
            },
        ) {
            FAILED.set(true);
            eprintln!("plenty-gpui: open window failed: {error}");
            request_quit(cx);
        }
        cx.activate(true);
    });
    if FAILED.get() {
        Err(AppError::Failed)
    } else {
        Ok(())
    }
}

#[unsafe(no_mangle)]
extern "C" fn plgp_v1_run() -> u32 {
    if let Err(error) = crate::begin_application() {
        return error as u32;
    }
    match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("plenty-gpui: {error}");
            Error::NativeFailure as u32
        }
    }
}
