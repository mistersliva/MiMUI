//! The GPU renderer.
//!
//! One pipeline for boxes (a rounded-rectangle SDF shader, which gives radii,
//! borders and soft shadows for free) and one for textured quads (images and
//! glyphs). Both share a vertex layout and a bind group layout.

use crate::ctx::{DrawCmd, Frame, ImageContent, TextureRef, Transform};
use crate::geom::{Corners, Rect, Vec2};
use crate::image::{decode_raster, decode_svg};
// winit 0.30 speaks raw-window-handle 0.6; bring that version's types in.
use winit::raw_window_handle::{self, HasDisplayHandle};

/// Vertex layout shared by both pipelines.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    /// Clip rect as (x, y, w, h) in pixels.
    clip: [f32; 4],
    /// Position in shape space, for the SDF.
    local: [f32; 2],
    /// Half width, half height, corner radius.
    extent: [f32; 3],
    /// Border width in pixels; 0 for plain quads.
    border: f32,
    /// Transform: cos, sin, scale.x, scale.y.
    xform: [f32; 4],
}

const VERTEX_SIZE: usize = std::mem::size_of::<Vertex>();

/// Per-frame uniform block.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    /// Drawing-buffer size in pixels.
    viewport: [f32; 2],
    /// Device pixels per logical pixel.
    scale: f32,
    _pad: f32,
}

/// A decoded image on the GPU.
struct GpuImage {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// An owned display handle.
///
/// wgpu 30 wants a `HasDisplayHandle` that is `Send + Sync + 'static`, but a
/// handle borrowed from the window is neither. Storing the raw handle keeps the
/// window alive alongside the renderer, so re-borrowing it is sound.
#[derive(Debug)]
struct OwnedDisplayHandle(raw_window_handle::DisplayHandle<'static>);

impl OwnedDisplayHandle {
    fn new(handle: raw_window_handle::DisplayHandle<'_>) -> Self {
        // SAFETY: the window is owned by the renderer for as long as this handle
        // is used, so the raw handle cannot be invalidated.
        let handle = unsafe {
            std::mem::transmute::<
                raw_window_handle::DisplayHandle<'_>,
                raw_window_handle::DisplayHandle<'static>,
            >(handle)
        };
        Self(handle)
    }
}

impl HasDisplayHandle for OwnedDisplayHandle {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(self.0)
    }
}

// SAFETY: the handle is only read, and it points at a window the renderer owns.
unsafe impl Send for OwnedDisplayHandle {}
// SAFETY: as above; access is read-only.
unsafe impl Sync for OwnedDisplayHandle {}

/// A contiguous run of triangles sharing a pipeline and texture.
struct Batch {
    /// Pipeline to draw with.
    image: bool,
    /// Texture view to bind, or `None` for boxes.
    texture: Option<TextureRef>,
    first: u32,
    count: u32,
}

/// Owns the device, pipelines, textures and per-frame buffers.
///
/// The window is kept in an `Arc` so the surface can be `'static` and the
/// renderer can live alongside the window without a self-referential struct.
pub struct Renderer {
    /// Kept alive so the surface handle stays valid.
    _window: std::sync::Arc<winit::window::Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Whether `surface.configure` has been called at least once.
    configured: bool,

    box_pipeline: wgpu::RenderPipeline,
    image_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    globals: Globals,
    placeholder: wgpu::TextureView,

    vertex_buffer: wgpu::Buffer,
    vertex_capacity: usize,

    glyph_atlas: Option<wgpu::TextureView>,
    images: HashMapOfImages,
    failed: Vec<u64>,
    /// Bind group used by the box pipeline (which samples nothing).
    box_bind: wgpu::BindGroup,
}

/// A small wrapper so the map type stays readable.
type HashMapOfImages = std::collections::HashMap<u64, GpuImage>;

impl Renderer {
    /// Initialises the device and surface for a window.
    ///
    /// The window is moved into an `Arc` that the renderer keeps alive, so the
    /// surface can borrow the handle for `'static`.
    pub fn new(window: std::sync::Arc<winit::window::Window>) -> Result<Self, String> {
        Self::create(window)
    }

    fn create(window: std::sync::Arc<winit::window::Window>) -> Result<Self, String> {
        // wgpu 30 needs the display handle up front so it can pick a backend.
        let display = OwnedDisplayHandle::new(window.display_handle().map_err(|e| e.to_string())?);
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.display = Some(Box::new(display));
        let instance = wgpu::Instance::new(desc);

        // `Arc<Window>` implements `HasWindowHandle`, so the surface can hold
        // it as a `'static` reference; we keep the `Arc` in `self`.
        let target = wgpu::SurfaceTarget::Window(Box::new(window.clone()));
        let surface = instance
            .create_surface(target)
            .map_err(|e| format!("could not create a surface: {e}"))?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no suitable GPU adapter was found: {e}"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mimui-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|e| format!("could not create a GPU device: {e:?}"))?;

        let size = window.inner_size();
        let w = size.width.max(1);
        let h = size.height.max(1);
        let mut config = surface
            .get_default_config(&adapter, w, h)
            .ok_or("the GPU cannot present to this surface")?;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.width = w;
        config.height = h;
        let format = config.format;

        device.on_uncaptured_error(std::sync::Arc::new(|e| eprintln!("MiMUI/wgpu: {e}")));

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mimui-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mimui-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mimui-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let attrs = wgpu::vertex_attr_array![
            0 => Float32x2, // pos
            1 => Float32x2, // uv
            2 => Float32x4, // color
            3 => Float32x4, // clip
            4 => Float32x2, // local
            5 => Float32x3, // extent
            6 => Float32,   // border
            7 => Float32x4, // xform
        ];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: VERTEX_SIZE as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attrs,
        })];

        let make_pipeline = |label: &str, fs_entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs_entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let box_pipeline = make_pipeline("mimui-box", "fs_box");
        let image_pipeline = make_pipeline("mimui-image", "fs_image");

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mimui-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mimui-globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // A 1x1 white texture so the box pipeline has something bound.
        let placeholder_tex = upload_texture(
            &device,
            &queue,
            1,
            1,
            &[255, 255, 255, 255],
            "mimui-placeholder",
        );
        let placeholder = placeholder_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let box_bind = make_bind(&device, &layout, &uniform, &sampler, &placeholder);

        let vertex_capacity = 16 * 1024;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mimui-vertices"),
            size: (vertex_capacity * VERTEX_SIZE) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            _window: window,
            device,
            queue,
            surface,
            config,
            configured: false,
            box_pipeline,
            image_pipeline,
            layout,
            sampler,
            uniform,
            globals: Globals::default(),
            placeholder,
            vertex_buffer,
            vertex_capacity,
            glyph_atlas: None,
            images: HashMapOfImages::new(),
            failed: Vec::new(),
            box_bind,
        })
    }

    /// Uploads new glyph-atlas pixels (an 8-bit alpha mask).
    ///
    /// The coverage mask is widened to RGBA8 so a single shader can sample both
    /// glyphs and real images from the same pipeline.
    pub fn upload_glyph_atlas(&mut self, size: u32, pixels: &[u8]) {
        let rgba: Vec<u8> = pixels.iter().flat_map(|&a| [a, a, a, a]).collect();
        let tex = upload_texture(&self.device, &self.queue, size, size, &rgba, "mimui-glyphs");
        self.glyph_atlas = Some(tex.create_view(&wgpu::TextureViewDescriptor::default()));
    }

    /// Decodes and uploads an image, if it is not already resident.
    pub fn ensure_image(&mut self, img: &ImageContent) {
        let key = crate::ctx::image_key(img);
        if self.images.contains_key(&key) || self.failed.contains(&key) {
            return;
        }

        let decoded = match img.kind {
            crate::image::ImageKind::Svg => {
                let size = if img.intrinsic.x > 0.0 { img.intrinsic.x } else { 64.0 };
                decode_svg(&img.source, size).ok()
            }
            _ => decode_raster(&img.source).ok(),
        };

        let Some(decoded) = decoded else {
            // Remember the failure so we do not retry every frame.
            self.failed.push(key);
            return;
        };

        let tex = upload_texture(
            &self.device,
            &self.queue,
            decoded.width,
            decoded.height,
            &decoded.pixels,
            "mimui-image",
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.images.insert(key, GpuImage { _texture: tex, view });
    }

    /// Draws one frame.
    pub fn render(&mut self, frame: &Frame, viewport: Vec2, scale: f32) {
        if viewport.x < 1.0 || viewport.y < 1.0 {
            return;
        }

        let px_w = (viewport.x * scale).round().max(1.0) as u32;
        let px_h = (viewport.y * scale).round().max(1.0) as u32;
        if !self.configured || self.config.width != px_w || self.config.height != px_h {
            self.config.width = px_w;
            self.config.height = px_h;
            self.surface.configure(&self.device, &self.config);
            self.configured = true;
        }

        let batches = self.build_vertices(frame, scale);
        // An empty draw list is not a reason to skip the frame: the window
        // still has to be cleared, or it keeps whatever was there before.

        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            // A suboptimal frame still draws, but reconfiguring keeps it fast.
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                self.surface.configure(&self.device, &self.config);
                t
            }
            // Occluded, outdated, lost or timed out: skip until the next frame.
            _ => return,
        };
        let view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.globals = Globals {
            viewport: [px_w as f32, px_h as f32],
            scale,
            _pad: 0.0,
        };
        self.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&self.globals));

        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mimui-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // The clear bypasses the shader, so it needs the same
                        // sRGB-to-linear conversion the fragment stage does.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: srgb_to_linear(frame.clear.r) as f64,
                            g: srgb_to_linear(frame.clear.g) as f64,
                            b: srgb_to_linear(frame.clear.b) as f64,
                            a: frame.clear.a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            for batch in &batches {
                if batch.count == 0 {
                    continue;
                }
                if batch.image {
                    let Some(bind) = self.bind_group(batch.texture) else {
                        // Texture not resident yet; skip until next frame.
                        continue;
                    };
                    pass.set_pipeline(&self.image_pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                } else {
                    pass.set_pipeline(&self.box_pipeline);
                    pass.set_bind_group(0, &self.box_bind, &[]);
                }
                let first = batch.first as u64 * VERTEX_SIZE as u64;
                let end = (batch.first + batch.count) as u64 * VERTEX_SIZE as u64;
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(first..end));
                pass.draw(0..batch.count, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        // In wgpu 30 presentation is a queue operation, not a texture method.
        self.queue.present(surface_texture);
    }

    /// Builds a bind group for a texture, or `None` if it is not resident yet.
    fn bind_group(&self, tex: Option<TextureRef>) -> Option<wgpu::BindGroup> {
        let view = match tex {
            Some(TextureRef::GlyphAtlas) => self.glyph_atlas.as_ref().unwrap_or(&self.placeholder),
            Some(TextureRef::Image(k)) => {
                // Not decoded yet: skip this batch rather than sampling garbage.
                self.images.get(&k).map(|i| &i.view)?
            }
            None => &self.placeholder,
        };
        Some(self.make_bind_group(view))
    }

    fn make_bind_group(&self, view: &wgpu::TextureView) -> wgpu::BindGroup {
        make_bind(&self.device, &self.layout, &self.uniform, &self.sampler, view)
    }
}

/// Builds the three-entry bind group every pipeline shares.
fn make_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mimui-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(view),
            },
        ],
    })
}

/// Expands a frame's draw commands into triangles, batched by pipeline and texture.
impl Renderer {
    fn build_vertices(&mut self, frame: &Frame, scale: f32) -> Vec<Batch> {
        let mut verts: Vec<Vertex> = Vec::new();
        let mut batches: Vec<Batch> = Vec::new();
        // Draw order matters: boxes and images are interleaved in the frame, so
        // we cannot batch across pipeline switches without reordering. Instead
        // we emit one batch per contiguous run, which keeps the frame faithful.
        let mut current: Option<(bool, Option<TextureRef>, u32)> = None;

        for cmd in &frame.cmds {
            match cmd {
                DrawCmd::Box { rect, radius, fill, border, shadow, clip, opacity, filter, transform } => {
                    let _ = filter;
                    let start = verts.len() as u32;

                    // The shadow goes down first so the box paints over it.
                    if let Some(sh) = shadow
                        && !sh.color.is_transparent()
                    {
                        let grow = sh.blur * 0.5;
                        let sr = Rect {
                            x: rect.x + sh.offset.x - grow,
                            y: rect.y + sh.offset.y - grow,
                            w: rect.w + grow * 2.0,
                            h: rect.h + grow * 2.0,
                        };
                        let r = radius.map(|v| v + grow).clamp(sr.size());
                        let _ = &r;
                        let soft = sh.blur.max(1.0);
                        self.push_box(
                            &mut verts,
                            sr,
                            r,
                            Some(sh.color.with_alpha(sh.color.a * opacity)),
                            None,
                            0.0,
                            *clip,
                            *transform,
                            soft,
                        );
                    }

                    let f = fill.filter(|c| !c.is_transparent());
                    let b = border.filter(|(_, c)| !c.is_transparent());
                    if f.is_some() || b.is_some() {
                        let border_w = b.map(|(w, _)| w).unwrap_or(0.0);
                        let color = f.or(b.map(|(_, c)| c)).expect("checked");
                        self.push_box(
                            &mut verts,
                            *rect,
                            radius.clamp(rect.size()),
                            Some(color.with_alpha(color.a * opacity)),
                            None,
                            border_w,
                            *clip,
                            *transform,
                            0.0,
                        );
                    }

                    Self::close_batch(&mut batches, &mut current, start, verts.len() as u32, false, None);
                }
                DrawCmd::Image { rect, uv, tint, texture, clip, opacity, radius, transform, .. } => {
                    if tint.is_transparent() || rect.is_empty() {
                        continue;
                    }
                    let start = verts.len() as u32;
                    self.push_quad(
                        &mut verts,
                        *rect,
                        *uv,
                        [tint.r, tint.g, tint.b, tint.a * opacity],
                        *clip,
                        radius.tl,
                        0.0,
                        *transform,
                        scale,
                    );
                    Self::close_batch(
                        &mut batches,
                        &mut current,
                        start,
                        verts.len() as u32,
                        true,
                        Some(*texture),
                    );
                }
            }
        }

        if verts.is_empty() {
            return Vec::new();
        }

        // Grow the vertex buffer if needed.
        if verts.len() > self.vertex_capacity {
            self.vertex_capacity = verts.len().next_power_of_two();
            self.vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mimui-vertices"),
                size: (self.vertex_capacity * VERTEX_SIZE) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let data = bytemuck::cast_slice(&verts);
        self.queue.write_buffer(&self.vertex_buffer, 0, data);

        batches
    }

    /// Appends a batch, merging with the previous one when they match.
    fn close_batch(
        batches: &mut Vec<Batch>,
        current: &mut Option<(bool, Option<TextureRef>, u32)>,
        start: u32,
        end: u32,
        image: bool,
        texture: Option<TextureRef>,
    ) {
        if start == end {
            return;
        }
        match current {
            Some((c_image, c_tex, c_start)) if *c_image == image && *c_tex == texture => {
                // Extend the run in place.
                if let Some(b) = batches.last_mut() {
                    b.count = end - *c_start;
                }
            }
            _ => {
                batches.push(Batch { image, texture, first: start, count: end - start });
                *current = Some((image, texture, start));
            }
        }
    }

    /// Appends a rounded box as two triangles.
    #[allow(clippy::too_many_arguments)]
    fn push_box(
        &mut self,
        verts: &mut Vec<Vertex>,
        rect: Rect,
        radius: Corners<f32>,
        fill: Option<crate::color::Color>,
        _border_color: Option<crate::color::Color>,
        border_width: f32,
        clip: Option<Rect>,
        transform: Transform,
        _softness: f32,
    ) {
        let Some(color) = fill else { return };
        self.push_quad(
            verts,
            rect,
            Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            [color.r, color.g, color.b, color.a],
            clip,
            radius.tl,
            border_width,
            transform,
            1.0,
        );
    }

    /// Appends a quad as two triangles, applying the command's transform.
    #[allow(clippy::too_many_arguments)]
    fn push_quad(
        &mut self,
        verts: &mut Vec<Vertex>,
        rect: Rect,
        uv: Rect,
        color: [f32; 4],
        clip: Option<Rect>,
        radius: f32,
        border: f32,
        transform: Transform,
        scale: f32,
    ) {
        let corners = transform.corners(rect);
        let aabb = transform.aabb(rect);
        let clip_v = clip.map(|c| [c.x, c.y, c.w, c.h]).unwrap_or([aabb.x, aabb.y, aabb.w, aabb.h]);
        let (sin, cos) = transform.rotate.sin_cos();
        let xform = [cos, sin, transform.scale.x, transform.scale.y];

        let uvs = [
            Vec2::new(uv.x, uv.y),
            Vec2::new(uv.x + uv.w, uv.y),
            Vec2::new(uv.x + uv.w, uv.y + uv.h),
            Vec2::new(uv.x, uv.y + uv.h),
        ];
        let hw = rect.w * 0.5;
        let hh = rect.h * 0.5;
        let locals = [
            Vec2::new(-hw, -hh),
            Vec2::new(hw, -hh),
            Vec2::new(hw, hh),
            Vec2::new(-hw, hh),
        ];

        let mk = |p: Vec2, local: Vec2, uv: Vec2| Vertex {
            pos: [p.x * scale, p.y * scale],
            uv: [uv.x, uv.y],
            color,
            clip: clip_v,
            local: [local.x, local.y],
            extent: [hw, hh, radius],
            border,
            xform,
        };

        let v: [Vertex; 6] = [
            mk(corners[0], locals[0], uvs[0]),
            mk(corners[1], locals[1], uvs[1]),
            mk(corners[2], locals[2], uvs[2]),
            mk(corners[0], locals[0], uvs[0]),
            mk(corners[2], locals[2], uvs[2]),
            mk(corners[3], locals[3], uvs[3]),
        ];
        verts.extend_from_slice(&v);
    }
}

/// The vertex stage, the rounded-box fragment stage and the textured one.
const SHADER: &str = r#"
struct Globals {
    viewport: vec2<f32>,
    scale: f32,
    pad: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var tex_sampler: sampler;
@group(0) @binding(2) var tex: texture_2d<f32>;

struct VertexIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) clip: vec4<f32>,
    @location(4) local: vec2<f32>,
    @location(5) extent: vec3<f32>,
    @location(6) border: f32,
    @location(7) xform: vec4<f32>,
};

struct VertexOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) local: vec2<f32>,
    @location(3) extent: vec3<f32>,
    @location(4) border: f32,
    @location(5) xform: vec4<f32>,
    @location(6) clip: vec4<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    // Logical pixels to clip space, y down.
    let ndc = vec2<f32>(
        (in.pos.x * globals.scale) / globals.viewport.x * 2.0 - 1.0,
        1.0 - (in.pos.y * globals.scale) / globals.viewport.y * 2.0,
    );
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    out.local = in.local;
    out.extent = in.extent;
    out.border = in.border;
    out.xform = in.xform;
    out.clip = vec4<f32>(in.clip.xy, in.clip.zw);
    return out;
}

/// Is the fragment (in logical pixels) inside the clip rectangle?
fn inside_clip(pos: vec2<f32>, clip: vec4<f32>) -> bool {
    return pos.x >= clip.x
        && pos.y >= clip.y
        && pos.x <= clip.x + clip.z
        && pos.y <= clip.y + clip.w;
}

/// Signed distance to a rounded box centred at the origin.
fn sd_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

/// Colours are authored in sRGB, but the surface is an `*-srgb` format, so the
/// hardware re-encodes whatever it is given. Handing it sRGB values would
/// lighten every one of them; convert first so blending also happens in linear.
/// The clear path in `render` uses the scalar [`srgb_to_linear`] for the same
/// reason.
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let cut = vec3<f32>(0.04045);
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= cut);
}

/// Maps a fragment back into shape space, undoing scale and rotation.
fn to_shape_space(local: vec2<f32>, xform: vec4<f32>) -> vec2<f32> {
    var p = local;
    let s = vec2<f32>(xform.z, xform.w);
    if s.x != 0.0 && s.y != 0.0 {
        p = p / s;
    }
    let c = xform.x;
    let sn = xform.y;
    return vec2<f32>(p.x * c + p.y * sn, -p.x * sn + p.y * c);
}

@fragment
fn fs_box(in: VertexOut) -> @location(0) vec4<f32> {
    let p = to_shape_space(in.local, in.xform);
    let b = max(in.extent.xy, vec2<f32>(0.0, 0.0));
    let r = clamp(in.extent.z, 0.0, min(b.x, b.y));

    let d = sd_rounded_box(p, b, r);
    // Antialias across roughly one pixel.
    var coverage = clamp(0.5 - d, 0.0, 1.0);

    if in.border > 0.0 {
        // Punch the interior out, leaving the ring.
        let inner_b = max(b - vec2<f32>(in.border, in.border), vec2<f32>(0.0, 0.0));
        let inner_r = clamp(r - in.border, 0.0, min(inner_b.x, inner_b.y));
        let inner_d = sd_rounded_box(p, inner_b, inner_r);
        let inner_coverage = clamp(0.5 - inner_d, 0.0, 1.0);
        coverage = coverage * (1.0 - inner_coverage);
    }

    if coverage <= 0.001 {
        discard;
    }

    // Clip is in logical pixels; `in.pos` is in physical pixels.
    let px = in.pos.xy / globals.scale;
    if !inside_clip(px, in.clip) {
        discard;
    }
    return vec4<f32>(to_linear(in.color.rgb), in.color.a * coverage);
}

@fragment
fn fs_image(in: VertexOut) -> @location(0) vec4<f32> {
    let px = in.pos.xy / globals.scale;
    if !inside_clip(px, in.clip) {
        discard;
    }
    let mask = textureSample(tex, tex_sampler, in.uv).a;
    let alpha = in.color.a * mask;
    if alpha <= 0.001 {
        discard;
    }
    return vec4<f32>(to_linear(in.color.rgb), alpha);
}
"#;

/// Colours are authored in sRGB, but the surface is an `*-srgb` format, so the
/// hardware re-encodes whatever it is given. Handing it sRGB values would
/// lighten every one of them; convert first so blending also happens in linear.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    pixels: &[u8],
    label: &str,
) -> wgpu::Texture {
    let width = width.max(1);
    let height = height.max(1);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // `write_texture` needs the row pitch padded to 256 bytes.
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
    let unpadded = width as usize * 4;
    let padded = unpadded.div_ceil(align) * align;
    let mut staging = vec![0u8; padded * height as usize];
    for row in 0..height as usize {
        let src = row * unpadded;
        let dst = row * padded;
        let end = (src + unpadded).min(pixels.len());
        if src >= pixels.len() {
            break;
        }
        staging[dst..dst + (end - src)].copy_from_slice(&pixels[src..end]);
    }

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &staging,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(padded as u32),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );

    texture
}
