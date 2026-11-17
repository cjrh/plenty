//! Bounded C-ABI command builder for GPUI 0.2.2 (Zed Industries, Apache-2.0).
//! Numeric frame tokens authorize one synchronous render on its calling thread.
//! No application/GPUI pointers or Rust layouts are exposed. Text is copied
//! before entry returns. Upstream GPUI allocation failures can still abort.
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
    Text {
        text: String,
        color: u32,
        size: f32,
        action: u64,
    },
}

#[derive(Debug, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub gap: f32,
    pub padding: f32,
    pub background: u32,
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
                .any(|node| matches!(node.kind, Kind::Text { action: a, .. } if a == action))
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
pub extern "C" fn plgp_v1_begin(
    token: u64,
    axis: u32,
    gap: f32,
    padding: f32,
    background: u32,
) -> u32 {
    command(token, |builder| {
        if axis > 1 || !dimension(gap) || !dimension(padding) || background > 0xffffff {
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
            kind: if axis == 0 { Kind::Row } else { Kind::Column },
            gap,
            padding,
            background,
            children: Vec::new(),
        })?;
        builder.stack.push(index);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn plgp_v1_end(token: u64) -> u32 {
    command(token, |builder| {
        builder.stack.pop().map(|_| ()).ok_or(Error::InvalidLayout)
    })
}

fn dimension(value: f32) -> bool {
    value.is_finite() && (0.0..=1024.0).contains(&value)
}

/// # Safety
/// `text` supplies `len` readable bytes for this call, or may be null for zero
/// length. No pointer is retained. The Plenty UTF-8 adapter establishes this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plgp_v1_text(
    token: u64,
    text: *const u8,
    len: usize,
    color: u32,
    size: f32,
    action: u64,
) -> u32 {
    command(token, |builder| {
        if len > MAX_TEXT || builder.text_bytes + len > MAX_FRAME_TEXT {
            return Err(Error::LimitExceeded);
        }
        if color > 0xffffff
            || !size.is_finite()
            || !(1.0..=128.0).contains(&size)
            || (len != 0 && text.is_null())
        {
            return Err(Error::InvalidArgument);
        }
        if action != 0 && builder.scene.has_action(action) {
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
            kind: Kind::Text {
                text,
                color,
                size,
                action,
            },
            gap: 0.0,
            padding: 0.0,
            background: 0,
            children: Vec::new(),
        })?;
        builder.text_bytes += len;
        Ok(())
    })
}

#[cfg(test)]
mod tests;
