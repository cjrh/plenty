//! Bounded C-ABI command builder for iced 0.14.0 (Héctor Ramón and iced contributors, MIT).
//! Numeric frame tokens authorize one synchronous render on its calling thread.
//! No application/iced pointers or Rust layouts are exposed. Text is copied
//! before entry returns. Upstream iced allocation failures can still abort.
use std::{
    cell::RefCell,
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAX_NODES: usize = 256;
pub const MAX_DEPTH: usize = 16;
pub const MAX_TEXT: usize = 4096;
pub const MAX_FRAME_TEXT: usize = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    OutOfMemory = 1,
    CapacityOverflow = 2,
    InvalidContext = 3,
    LimitExceeded = 4,
    InvalidLayout = 5,
    InvalidArgument = 6,
    NativeFailure = 7,
}

#[derive(Debug, PartialEq)]
pub enum Kind {
    Row,
    Column,
    Stack,
    Grid(u16),
    Container(u16),
    FloatScaled(u16),
    Themer(bool),
    MousePress(u64),
    SensorShow(u64),
    Pin(u16, u16),
    Scrollable(u16),
    Tooltip,
    Rule,
    Space(u16, u16),
    ProgressBar(f32),
    Checkbox(String, bool, u64),
    Toggler(String, bool, u64),
    Radio(String, bool, u64),
    Text(String),
    Button(String, u64),
}

impl Kind {
    fn action(&self) -> Option<u64> {
        match self {
            Self::MousePress(a)
            | Self::SensorShow(a)
            | Self::Button(_, a)
            | Self::Checkbox(_, _, a)
            | Self::Toggler(_, _, a)
            | Self::Radio(_, _, a) => Some(*a),
            _ => None,
        }
    }
    fn exact_children(&self) -> Option<usize> {
        match self {
            Self::Container(_)
            | Self::MousePress(_)
            | Self::SensorShow(_)
            | Self::Themer(_)
            | Self::FloatScaled(_)
            | Self::Pin(_, _)
            | Self::Scrollable(_) => Some(1),
            Self::Tooltip => Some(2),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub children: Vec<usize>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Scene {
    pub nodes: Vec<Node>,
}

impl Scene {
    pub fn has_action(&self, action: u64) -> bool {
        action != 0
            && self
                .nodes
                .iter()
                .any(|node| node.kind.action() == Some(action))
    }
}

struct Builder {
    token: u64,
    scene: Scene,
    stack: Vec<usize>,
    text_bytes: usize,
    failure: Option<Error>,
}

impl Builder {
    fn add(&mut self, node: Node) -> Result<(), Error> {
        if self.scene.nodes.len() == MAX_NODES {
            return Err(Error::LimitExceeded);
        }
        if self.stack.is_empty() && !self.scene.nodes.is_empty() {
            return Err(Error::InvalidLayout);
        }
        self.scene
            .nodes
            .try_reserve(1)
            .map_err(|_| Error::OutOfMemory)?;
        let index = self.scene.nodes.len();
        if let Some(&parent) = self.stack.last() {
            let p = &self.scene.nodes[parent];
            if p.kind.exact_children() == Some(p.children.len()) {
                return Err(Error::InvalidLayout);
            }
            self.scene.nodes[parent]
                .children
                .try_reserve(1)
                .map_err(|_| Error::OutOfMemory)?;
            self.scene.nodes[parent].children.push(index);
        }
        self.scene.nodes.push(node);
        Ok(())
    }
}

thread_local! { static ACTIVE: RefCell<Option<Builder>> = const { RefCell::new(None) }; }
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

struct RenderScope;
impl Drop for RenderScope {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.borrow_mut().take();
        });
    }
}

/// The callback is synchronous. A guard clears the frame even on Rust panic.
/// Returned errors discard the entire scene, including errors ignored by clients.
pub fn build_scene(render: impl FnOnce(u64) -> Result<(), Error>) -> Result<Scene, Error> {
    let token = NEXT_TOKEN
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| Error::CapacityOverflow)?;
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        if active.is_some() {
            return Err(Error::InvalidContext);
        }
        *active = Some(Builder {
            token,
            scene: Scene::default(),
            stack: Vec::new(),
            text_bytes: 0,
            failure: None,
        });
        Ok(())
    })?;
    let _scope = RenderScope;
    render(token)?;
    ACTIVE.with(|active| {
        let builder = active.borrow_mut().take().ok_or(Error::InvalidContext)?;
        if let Some(error) = builder.failure {
            return Err(error);
        }
        if !builder.stack.is_empty() || builder.scene.nodes.is_empty() {
            return Err(Error::InvalidLayout);
        }
        Ok(builder.scene)
    })
}

fn command(token: u64, f: impl FnOnce(&mut Builder) -> Result<(), Error>) -> u32 {
    // Every extern operation is contained: no Rust unwind crosses a Plenty frame.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            let Some(builder) = active.as_mut() else {
                return Error::InvalidContext as u32;
            };
            if builder.token != token {
                return Error::InvalidContext as u32;
            }
            if let Some(error) = builder.failure {
                return error as u32;
            }
            match f(builder) {
                Ok(()) => 0,
                Err(error) => {
                    builder.failure = Some(error);
                    error as u32
                }
            }
        })
    }))
    .unwrap_or_else(|_| {
        let _ = ACTIVE.try_with(|active| {
            if let Ok(mut active) = active.try_borrow_mut()
                && let Some(builder) = active.as_mut()
                && builder.token == token
            {
                builder.failure = Some(Error::NativeFailure);
            }
        });
        Error::NativeFailure as u32
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_row(token: u64) -> u32 {
    group(token, Kind::Row)
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_column(token: u64) -> u32 {
    group(token, Kind::Column)
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_grid(token: u64, columns: u16) -> u32 {
    group(token, Kind::Grid(columns))
}

#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_float_scaled(token: u64, percent: u16) -> u32 {
    group(token, Kind::FloatScaled(percent))
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_mouse_area(token: u64, action: u64) -> u32 {
    group(token, Kind::MousePress(action))
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_sensor(token: u64, action: u64) -> u32 {
    group(token, Kind::SensorShow(action))
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_themer(token: u64, dark: bool) -> u32 {
    group(token, Kind::Themer(dark))
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_container(token: u64, padding: u16) -> u32 {
    group(token, Kind::Container(padding))
}

#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_pin(token: u64, x: u16, y: u16) -> u32 {
    group(token, Kind::Pin(x, y))
}

#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_scrollable(token: u64, height: u16) -> u32 {
    group(token, Kind::Scrollable(height))
}

#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_tooltip(token: u64) -> u32 {
    group(token, Kind::Tooltip)
}

fn group(token: u64, kind: Kind) -> u32 {
    command(token, |builder| {
        if kind
            .action()
            .is_some_and(|a| a == 0 || builder.scene.has_action(a))
        {
            return Err(Error::InvalidArgument);
        }
        if matches!(kind, Kind::FloatScaled(n) if !(100..=400).contains(&n)) {
            return Err(Error::InvalidArgument);
        }
        if matches!(&kind, Kind::Grid(n) if *n == 0 || usize::from(*n) > MAX_NODES) {
            return Err(Error::InvalidArgument);
        }
        if builder.stack.len() == MAX_DEPTH {
            return Err(Error::LimitExceeded);
        }
        builder
            .stack
            .try_reserve(1)
            .map_err(|_| Error::OutOfMemory)?;
        let index = builder.scene.nodes.len();
        builder.add(Node {
            kind,
            children: Vec::new(),
        })?;
        builder.stack.push(index);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_end(token: u64) -> u32 {
    command(token, |builder| {
        let i = builder.stack.pop().ok_or(Error::InvalidLayout)?;
        let node = &builder.scene.nodes[i];
        if node
            .kind
            .exact_children()
            .is_some_and(|n| n != node.children.len())
        {
            return Err(Error::InvalidLayout);
        }
        Ok(())
    })
}

/// # Safety
/// `text` supplies `len` readable bytes for this call, or may be null for zero
/// length. No pointer is retained. The Plenty UTF-8 adapter establishes this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plic_v1_text(token: u64, text: *const u8, len: usize) -> u32 {
    unsafe { leaf(token, text, len, None, Kind::Text) }
}

/// # Safety
/// Same call-scoped UTF-8 buffer contract as `plic_v1_text`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plic_v1_button(
    token: u64,
    text: *const u8,
    len: usize,
    action: u64,
) -> u32 {
    unsafe { leaf(token, text, len, Some(action), |s| Kind::Button(s, action)) }
}

/// # Safety
/// Same call-scoped caption contract as `plic_v1_text`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plic_v1_checkbox(
    token: u64,
    text: *const u8,
    len: usize,
    selected: bool,
    action: u64,
) -> u32 {
    unsafe {
        leaf(token, text, len, Some(action), |s| {
            Kind::Checkbox(s, selected, action)
        })
    }
}

/// # Safety
/// Same call-scoped caption contract as `plic_v1_text`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plic_v1_toggler(
    token: u64,
    text: *const u8,
    len: usize,
    selected: bool,
    action: u64,
) -> u32 {
    unsafe {
        leaf(token, text, len, Some(action), |s| {
            Kind::Toggler(s, selected, action)
        })
    }
}

/// # Safety
/// Same call-scoped caption contract as `plic_v1_text`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plic_v1_radio(
    token: u64,
    text: *const u8,
    len: usize,
    selected: bool,
    action: u64,
) -> u32 {
    unsafe {
        leaf(token, text, len, Some(action), |s| {
            Kind::Radio(s, selected, action)
        })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_progress_bar(token: u64, fraction: f32) -> u32 {
    command(token, |b| {
        if !(0.0..=1.0).contains(&fraction) {
            return Err(Error::InvalidArgument);
        }
        b.add(Node {
            kind: Kind::ProgressBar(fraction),
            children: vec![],
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_rule(token: u64) -> u32 {
    command(token, |b| {
        b.add(Node {
            kind: Kind::Rule,
            children: vec![],
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_space(token: u64, width: u16, height: u16) -> u32 {
    command(token, |b| {
        b.add(Node {
            kind: Kind::Space(width, height),
            children: vec![],
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_stack(token: u64) -> u32 {
    group(token, Kind::Stack)
}

unsafe fn leaf(
    token: u64,
    text: *const u8,
    len: usize,
    action: Option<u64>,
    make: impl FnOnce(String) -> Kind,
) -> u32 {
    command(token, |builder| {
        if len > MAX_TEXT || builder.text_bytes + len > MAX_FRAME_TEXT {
            return Err(Error::LimitExceeded);
        }
        if len != 0 && text.is_null() {
            return Err(Error::InvalidArgument);
        }
        if action.is_some_and(|a| a == 0 || builder.scene.has_action(a)) {
            return Err(Error::InvalidArgument);
        }
        let bytes = if len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(text, len) }
        };
        let source = std::str::from_utf8(bytes).map_err(|_| Error::InvalidArgument)?;
        let mut text = String::new();
        text.try_reserve(len).map_err(|_| Error::OutOfMemory)?;
        text.push_str(source);
        builder.add(Node {
            kind: make(text),
            children: Vec::new(),
        })?;
        builder.text_bytes += len;
        Ok(())
    })
}

#[cfg(test)]
mod tests;

#[cfg(all(feature = "application", not(test)))]
mod application;

#[cfg(feature = "application")]
fn loop_outcome(native: Result<(), Error>, callback: Option<Error>) -> Result<(), Error> {
    callback.map_or(native, Err)
}

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
