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

#[cfg(feature = "driver")]
#[derive(Debug)]
pub struct Qr {
    payload: String,
    data: iced::widget::qr_code::Data,
}
#[cfg(feature = "driver")]
impl PartialEq for Qr {
    fn eq(&self, other: &Self) -> bool {
        self.payload == other.payload
    }
}

#[derive(Debug, Default)]
pub struct ComboCache {
    #[cfg(all(feature = "driver", not(test)))]
    state: std::cell::OnceCell<iced::widget::combo_box::State<application::ComboChoice>>,
}
// Cached toolkit state does not participate in snapshot equality.
impl PartialEq for ComboCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[derive(Debug)]
pub struct PaneSplit {
    side_by_side: bool,
    first_percent: u16,
    #[cfg(all(feature = "driver", not(test)))]
    layout: std::cell::OnceCell<iced::widget::pane_grid::State<usize>>,
}

impl PartialEq for PaneSplit {
    fn eq(&self, other: &Self) -> bool {
        (self.side_by_side, self.first_percent) == (other.side_by_side, other.first_percent)
    }
}

#[derive(Debug, PartialEq)]
pub enum Kind {
    Row,
    Column,
    Stack,
    Grid(u16),
    PaneGrid(PaneSplit),
    Slider {
        vertical: bool,
        value: u16,
        min: u16,
        max: u16,
        id: u64,
    },
    Canvas(u16, u16),
    FillRect {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        rgb: u32,
    },
    PickList(u64),
    ComboBox(u64, ComboCache),
    Choice(String, u64),
    Responsive(u16),
    #[cfg(feature = "driver")]
    Image(iced::widget::image::Handle, u32, u32),
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
    TextInput(String, u64),
    TextEditor(String, u64),
    #[cfg(feature = "shader")]
    Shader(std::sync::Arc<str>, u16, u16),
    Svg(String),
    #[cfg(feature = "driver")]
    QrCode(Qr),
    Button(String, u64),
}

impl Kind {
    fn action(&self) -> Option<u64> {
        match self {
            Self::Slider { id: a, .. }
            | Self::TextEditor(_, a)
            | Self::TextInput(_, a)
            | Self::Choice(_, a)
            | Self::MousePress(a)
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
            Self::Tooltip | Self::PaneGrid(_) => Some(2),
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
        let in_canvas = self
            .stack
            .last()
            .is_some_and(|i| matches!(self.scene.nodes[*i].kind, Kind::Canvas(..)));
        if in_canvas != matches!(node.kind, Kind::FillRect { .. }) {
            return Err(Error::InvalidLayout);
        }
        let in_list = self.stack.last().is_some_and(|i| {
            matches!(
                self.scene.nodes[*i].kind,
                Kind::PickList(_) | Kind::ComboBox(..)
            )
        });
        if in_list != matches!(node.kind, Kind::Choice(..)) {
            return Err(Error::InvalidLayout);
        }
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
        if matches!(&node.kind,Kind::TextInput(s,_) if s.contains(['\r','\n'])) {
            return Err(Error::InvalidArgument);
        }
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
pub extern "C" fn plic_v1_check_frame(token: u64) -> u32 {
    command(token, |_| Ok(()))
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
pub extern "C" fn plic_v1_responsive(token: u64, row_min_width: u16) -> u32 {
    group(token, Kind::Responsive(row_min_width))
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_combo_box(token: u64, selected: u64) -> u32 {
    group(token, Kind::ComboBox(selected, Default::default()))
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_pick_list(token: u64, selected: u64) -> u32 {
    group(token, Kind::PickList(selected))
}
/// # Safety
/// Same call-scoped UTF-8 caption contract as `plic_v1_text`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plic_v1_choice(token: u64, p: *const u8, len: usize, action: u64) -> u32 {
    unsafe { leaf(token, p, len, Some(action), |s| Kind::Choice(s, action)) }
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_canvas(token: u64, w: u16, h: u16) -> u32 {
    group(token, Kind::Canvas(w, h))
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_fill_rect(token: u64, x: u16, y: u16, w: u16, h: u16, rgb: u32) -> u32 {
    command(token, |b| {
        if w == 0 || h == 0 || rgb > 0xffffff {
            return Err(Error::InvalidArgument);
        }
        b.add(Node {
            kind: Kind::FillRect { x, y, w, h, rgb },
            children: vec![],
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_slider(
    token: u64,
    vertical: bool,
    value: u16,
    min: u16,
    max: u16,
    id: u64,
) -> u32 {
    command(token, |b| {
        if min >= max || !(min..=max).contains(&value) || id == 0 || b.scene.has_action(id) {
            return Err(Error::InvalidArgument);
        }
        b.add(Node {
            kind: Kind::Slider {
                vertical,
                value,
                min,
                max,
                id,
            },
            children: vec![],
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn plic_v1_pane_grid(token: u64, side_by_side: bool, first_percent: u16) -> u32 {
    group(
        token,
        Kind::PaneGrid(PaneSplit {
            side_by_side,
            first_percent,
            #[cfg(all(feature = "driver", not(test)))]
            layout: Default::default(),
        }),
    )
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
        if let Kind::Canvas(w, h) = kind {
            if w == 0 || h == 0 {
                return Err(Error::InvalidArgument);
            }
            if w > 2048 || h > 2048 {
                return Err(Error::LimitExceeded);
            }
        }
        if matches!(&kind, Kind::PaneGrid(s) if !(1..100).contains(&s.first_percent)) {
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
        if let Kind::PickList(selected) | Kind::ComboBox(selected, _) = node.kind {
            if node.children.is_empty() {
                return Err(Error::InvalidLayout);
            }
            if selected != 0
                && !node
                    .children
                    .iter()
                    .any(|i| builder.scene.nodes[*i].kind.action() == Some(selected))
            {
                return Err(Error::InvalidArgument);
            }
        }
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
pub unsafe extern "C" fn plic_v1_svg(token: u64, source: *const u8, len: usize) -> u32 {
    unsafe { leaf(token, source, len, None, Kind::Svg) }
}

/// # Safety
/// Same call-scoped UTF-8 buffer contract as `plic_v1_text`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plic_v1_text_editor(
    token: u64,
    text: *const u8,
    len: usize,
    id: u64,
) -> u32 {
    unsafe { leaf(token, text, len, Some(id), |s| Kind::TextEditor(s, id)) }
}
/// # Safety
/// Same call-scoped UTF-8 buffer contract as `plic_v1_text`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plic_v1_text_input(
    token: u64,
    text: *const u8,
    len: usize,
    id: u64,
) -> u32 {
    unsafe { leaf(token, text, len, Some(id), |s| Kind::TextInput(s, id)) }
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

/// # Safety
/// The caller lends `len` readable UTF-8 bytes for this call.
#[cfg(feature = "driver")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plic_v1_qr_code(token: u64, text: *const u8, len: usize) -> u32 {
    command(token, |b| {
        if len > MAX_TEXT || b.text_bytes + len > MAX_FRAME_TEXT {
            return Err(Error::LimitExceeded);
        }
        if len == 0 || text.is_null() {
            return Err(Error::InvalidArgument);
        }
        let s = std::str::from_utf8(unsafe { std::slice::from_raw_parts(text, len) })
            .map_err(|_| Error::InvalidArgument)?;
        let data = iced::widget::qr_code::Data::new(s).map_err(|e| match e {
            iced::widget::qr_code::Error::DataTooLong => Error::LimitExceeded,
            _ => Error::NativeFailure,
        })?;
        let mut payload = String::new();
        payload
            .try_reserve_exact(len)
            .map_err(|_| Error::OutOfMemory)?;
        payload.push_str(s);
        b.add(Node {
            kind: Kind::QrCode(Qr { payload, data }),
            children: Vec::new(),
        })?;
        b.text_bytes += len;
        Ok(())
    })
}

#[cfg(feature = "driver")]
mod image;
#[cfg(feature = "shader")]
mod shader;

#[cfg(test)]
mod tests;

#[cfg(all(feature = "driver", not(test)))]
mod application;

#[cfg(feature = "driver")]
fn loop_outcome(native: Result<(), Error>, callback: Option<Error>) -> Result<(), Error> {
    callback.map_or(native, Err)
}

#[cfg(feature = "driver")]
static MAIN_THREAD: std::sync::OnceLock<std::thread::ThreadId> = std::sync::OnceLock::new();

#[cfg(feature = "driver")]
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

#[cfg(all(test, feature = "driver"))]
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

#[cfg(all(feature = "driver", not(test)))]
#[unsafe(no_mangle)]
unsafe extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    MAIN_THREAD
        .set(std::thread::current().id())
        .expect("one process entry");
    // SAFETY: the native process entry supplies valid process-lifetime argv.
    unsafe { plenty_runtime::main(argc, argv) }
}
