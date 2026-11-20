//! A trusted, application-supplied color expression in a fixed, resource-free pipeline.
use crate::{Error, Kind, Node, command};
use iced::{Rectangle, mouse, wgpu, widget::shader};
use std::{collections::HashMap, sync::Arc};

const HEADER: &str = r#"
struct Out { @builtin(position) pos:vec4f, @location(0) uv:vec2f }
@vertex fn vs(@builtin(vertex_index) i:u32)->Out {
 let xy=vec2f(f32((i<<1u)&2u),f32(i&2u));
 return Out(vec4f(xy*vec2f(2,-2)+vec2f(-1,1),0,1),xy);
}
@fragment fn fs(@location(0) uv:vec2f)->@location(0) vec4f { return
"#;
fn source(expr: &str) -> Result<String, Error> {
    if expr.contains([';', '{', '}', '@']) || expr.contains("//") || expr.contains("/*") {
        return Err(Error::InvalidArgument);
    }
    let mut src = String::new();
    src.try_reserve(HEADER.len() + expr.len() + 3)
        .map_err(|_| Error::OutOfMemory)?;
    src.push_str(HEADER);
    src.push_str(expr);
    src.push_str("\n;}");
    let module = naga::front::wgsl::parse_str(&src).map_err(|_| Error::InvalidArgument)?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|_| Error::InvalidArgument)?;
    Ok(src)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn plic_v1_fragment(p: *const u8, len: usize, w: u16, h: u16) -> u32 {
    // The facade just validated its Frame; no user callback or suspension intervenes.
    let Some(token) =
        crate::ACTIVE.with(|active| active.try_borrow().ok()?.as_ref().map(|b| b.token))
    else {
        return Error::InvalidContext as u32;
    };
    command(token, |b| {
        if len > crate::MAX_TEXT || b.text_bytes + len > crate::MAX_FRAME_TEXT {
            return Err(Error::LimitExceeded);
        }
        if p.is_null() || !(1..=2048).contains(&w) || !(1..=2048).contains(&h) {
            return Err(Error::InvalidArgument);
        }
        let expr = std::str::from_utf8(unsafe { std::slice::from_raw_parts(p, len) })
            .map_err(|_| Error::InvalidArgument)?;
        b.add(Node {
            kind: Kind::Shader(source(expr)?.into(), w, h),
            children: Vec::new(),
        })?;
        b.text_bytes += len;
        Ok(())
    })
}
#[derive(Debug, Clone)]
pub(super) struct Fragment(pub Arc<str>);
impl<M> shader::Program<M> for Fragment {
    type State = ();
    type Primitive = Self;
    fn draw(&self, _: &(), _: mouse::Cursor, _: Rectangle) -> Self {
        self.clone()
    }
}
pub(super) struct Pipelines {
    format: wgpu::TextureFormat,
    entries: HashMap<Arc<str>, (bool, wgpu::RenderPipeline)>,
}
impl shader::Pipeline for Pipelines {
    fn new(_: &wgpu::Device, _: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            format,
            entries: HashMap::new(),
        }
    }
    fn trim(&mut self) {
        self.entries.retain(|_, (used, _)| std::mem::take(used));
    }
}
impl shader::Primitive for Fragment {
    type Pipeline = Pipelines;
    fn prepare(
        &self,
        store: &mut Pipelines,
        device: &wgpu::Device,
        _: &wgpu::Queue,
        _: &Rectangle,
        _: &shader::Viewport,
    ) {
        store
            .entries
            .entry(self.0.clone())
            .or_insert_with(|| {
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Plenty fragment"),
                    source: wgpu::ShaderSource::Wgsl(self.0.as_ref().into()),
                });
                (
                    true,
                    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("Plenty fragment"),
                        layout: None,
                        vertex: wgpu::VertexState {
                            module: &module,
                            entry_point: Some("vs"),
                            buffers: &[],
                            compilation_options: Default::default(),
                        },
                        primitive: Default::default(),
                        depth_stencil: None,
                        multisample: Default::default(),
                        fragment: Some(wgpu::FragmentState {
                            module: &module,
                            entry_point: Some("fs"),
                            compilation_options: Default::default(),
                            targets: &[Some(store.format.into())],
                        }),
                        multiview: None,
                        cache: None,
                    }),
                )
            })
            .0 = true;
    }
    fn draw(&self, store: &Pipelines, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pass.set_pipeline(&store.entries[&self.0].1);
        pass.draw(0..3, 0..1);
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expressions_have_fixed_interface() {
        let fragment = Fragment(source("vec4f(uv,0,1)").unwrap().into());
        let _widget = iced::widget::shader::<(), _>(fragment);
        for bad in ["uv", "not_a_name", "vec4f(1); }", "vec4f(1) //"] {
            assert_eq!(source(bad), Err(Error::InvalidArgument));
        }
    }
}
