//! Single-threaded GPUI host. All Plenty entry/destruction stays on this thread.
use gpui::{
    App, Application, Bounds, Context, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};
use plenty_gpui::{Error, Kind, Scene, build_scene};
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

// A unique originating owner, intentionally !Send and !Sync. GPUI listeners hold
// only weak entity references, never this pointer or an outstanding Plenty loan.
struct StateOwner {
    pointer: NonNull<std::ffi::c_void>,
    _thread_bound: PhantomData<Rc<()>>,
}
impl StateOwner {
    fn create() -> Result<Self, String> {
        let mut error = 0;
        let pointer = unsafe { plgp_app_create(&mut error) };
        Ok(Self {
            pointer: NonNull::new(pointer)
                .ok_or_else(|| format!("Plenty create failed: AllocError code {error}"))?,
            _thread_bound: PhantomData,
        })
    }
    fn render(&self) -> Result<Scene, String> {
        build_scene(|token| {
            let mut error = 0;
            let status = unsafe {
                plgp_app_render(
                    self.pointer.as_ptr(),
                    token,
                    plenty_gpui::plgp_v1_begin as *const () as *mut std::ffi::c_void,
                    plenty_gpui::plgp_v1_end as *const () as *mut std::ffi::c_void,
                    plenty_gpui::plgp_v1_text as *const () as *mut std::ffi::c_void,
                    &mut error,
                )
            };
            callback_status(status, error)
        })
        .map_err(|error| format!("Plenty render failed: {error:?}"))
    }
    fn event(&mut self, action: u64) -> Result<(), String> {
        let mut error = 0;
        let status = unsafe { plgp_app_event(self.pointer.as_ptr(), action, &mut error) };
        callback_status(status, error).map_err(|error| format!("Plenty event failed: {error:?}"))
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

struct View {
    state: StateOwner,
    scene: Scene,
    failure: Rc<Cell<bool>>,
    stopped: bool,
}
impl View {
    fn stop(&mut self, error: String, cx: &mut Context<Self>) {
        self.stopped = true;
        self.failure.set(true);
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
                    .id(SharedString::from(format!("action-{action}")))
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

fn headless(mut state: StateOwner, actions: &[String]) -> Result<(), String> {
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
        let action = action
            .parse::<u64>()
            .map_err(|_| "expected numeric action ID")?;
        if !scene.has_action(action) {
            return Err(format!("unknown action ID: {action}"));
        }
        state.event(action)?;
        scene = state.render()?;
        show(&scene);
    }
    Ok(())
}

fn run() -> Result<(), String> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err("qualified only for Linux x86-64".into());
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--headless") {
        return headless(StateOwner::create()?, &args[1..]);
    }
    if !args.is_empty() {
        return Err("usage: plenty-gpui [--headless [ACTION...]]".into());
    }
    let state = StateOwner::create()?;
    // Fail before opening a native window on malformed initial render.
    let scene = state.render()?;
    let failure = Rc::new(Cell::new(false));
    let result = failure.clone();
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
                    failure: result.clone(),
                    stopped: false,
                })
            },
        ) {
            result.set(true);
            eprintln!("plenty-gpui: open window failed: {error}");
            request_quit(cx);
        }
        cx.activate(true);
    });
    if failure.get() {
        Err("application failed".into())
    } else {
        Ok(())
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("plenty-gpui: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
