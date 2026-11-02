//! Motore di disegno su GPU (wgpu), alternativo a quello software della
//! finestra (`--gpu`).
//!
//! Riceve gli stessi dati del percorso software: l'immagine del gioco
//! (320 pixel di larghezza, un `u32` 0x00RRGGBB per pixel) e l'elenco di forme
//! dello strato di sovrapposizione (`overlay::Shape`). Li disegna in due
//! passate dentro lo stesso render pass:
//!
//! 1. **gioco**: un triangolo che copre tutta la finestra; il fragment shader
//!    calcola quale pixel del gioco cade sotto ogni pixel della finestra
//!    (stessa scala intera e stesso margine del percorso software) e lo legge
//!    con `textureLoad`, cioè senza filtri: ingrandimento "a blocchi" identico;
//! 2. **forme**: rettangoli in pixel della finestra (istanze), con trasparenza
//!    e fino a due maschere per istanza (`Shape::Mask`), applicate scartando i
//!    frammenti che vi cadono dentro.
//!
//! Il risultato coincide con quello del percorso software a meno di 1 livello
//! di colore negli arrotondamenti della trasparenza (vedi il test
//! `stesso_risultato_del_disegno_software`). È la base per il lavoro futuro che
//! al software costerebbe troppo: grafica sostitutiva in alta risoluzione,
//! filtri (CRT, ingrandimenti morbidi), testo nitido.
//!
//! Funziona sia su una finestra (superficie di `minifb`, che espone gli handle
//! `raw-window-handle`) sia fuori schermo, su una texture che si può rileggere:
//! così i test e gli esempi girano anche senza GPU (Vulkan software, lavapipe).

use crate::overlay::{Argb, Shape};
use crate::render::{HEIGHT, WIDTH};

/// Disposizione dell'immagine nella finestra: come nel percorso software.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub scale: usize,
    pub margin: usize,
    /// Righe del display mostrate (200 + margine inferiore, o 256).
    pub rows: usize,
}

impl Layout {
    /// Dimensioni della finestra in pixel.
    pub fn size(&self) -> (u32, u32) {
        (
            ((WIDTH + 2 * self.margin) * self.scale) as u32,
            ((self.margin + self.rows) * self.scale) as u32,
        )
    }
}

/// Parametri comuni agli shader.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    /// Dimensioni della finestra in pixel.
    viewport: [f32; 2],
    /// Posizione in pixel della finestra del pixel (0,0) del gioco.
    origin: [f32; 2],
    scale: f32,
    /// Righe dell'immagine del gioco disponibili nella texture.
    src_rows: f32,
    /// Righe del display da mostrare.
    rows: f32,
    _pad: f32,
}

/// Un rettangolo dello strato di sovrapposizione, in pixel della finestra.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Quad {
    /// x0, y0, x1, y1 (estremi esclusi a destra e in basso).
    pub rect: [f32; 4],
    /// Colore RGBA 0..1, alfa non premoltiplicato.
    pub color: [f32; 4],
    /// Maschere: rettangoli in cui non disegnare (x1 <= x0 = nessuna).
    pub mask0: [f32; 4],
    pub mask1: [f32; 4],
}

const NO_MASK: [f32; 4] = [0.0, 0.0, -1.0, -1.0];

const SHADER: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
    origin: vec2<f32>,
    scale: f32,
    src_rows: f32,
    rows: f32,
    pad: f32,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var game: texture_2d<f32>;

// --- Passata 1: l'immagine del gioco ingrandita a blocchi ---

@vertex
fn vs_game(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // Triangolo che copre tutta la finestra.
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_game(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    // p.xy è il centro del pixel della finestra (x + 0.5, y + 0.5).
    let g = floor((p.xy - u.origin) / u.scale);
    if (g.x < 0.0 || g.y < 0.0 || g.x >= 320.0 || g.y >= u.rows || g.y >= u.src_rows) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let c = textureLoad(game, vec2<i32>(i32(g.x), i32(g.y)), 0);
    return vec4<f32>(c.rgb, 1.0);
}

// --- Passata 2: le forme dello strato di sovrapposizione ---

struct QuadIn {
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) mask0: vec4<f32>,
    @location(3) mask1: vec4<f32>,
};
struct QuadOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) mask0: vec4<f32>,
    @location(2) mask1: vec4<f32>,
};

@vertex
fn vs_quad(@builtin(vertex_index) i: u32, q: QuadIn) -> QuadOut {
    // Due triangoli: angoli 0,1,2 e 2,1,3 del rettangolo.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0));
    let c = corners[i];
    let px = mix(q.rect.xy, q.rect.zw, c);
    var o: QuadOut;
    o.pos = vec4<f32>(px.x / u.viewport.x * 2.0 - 1.0, 1.0 - px.y / u.viewport.y * 2.0, 0.0, 1.0);
    o.color = q.color;
    o.mask0 = q.mask0;
    o.mask1 = q.mask1;
    return o;
}

fn inside(p: vec2<f32>, m: vec4<f32>) -> bool {
    return p.x >= m.x && p.x < m.z && p.y >= m.y && p.y < m.w;
}

@fragment
fn fs_quad(q: QuadOut) -> @location(0) vec4<f32> {
    if (inside(q.pos.xy, q.mask0) || inside(q.pos.xy, q.mask1)) {
        discard;
    }
    return q.color;
}
"#;

/// Dove finisce il disegno.
enum Target {
    /// Finestra: la superficie va riconfigurata se la finestra cambia.
    Surface {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    },
    /// Fuori schermo: texture + buffer per rileggerla.
    Offscreen {
        texture: wgpu::Texture,
        readback: wgpu::Buffer,
        padded_row: u32,
    },
}

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: Target,
    format: wgpu::TextureFormat,
    game_texture: wgpu::Texture,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    game_pipeline: wgpu::RenderPipeline,
    quad_pipeline: wgpu::RenderPipeline,
    quads: wgpu::Buffer,
    quad_capacity: usize,
    layout: Layout,
    /// Descrizione della scheda video usata (per il log).
    pub adapter_info: String,
}

/// Converte le forme dello strato in rettangoli in pixel della finestra, con
/// gli stessi arrotondamenti di `overlay::Canvas` (così i due percorsi danno la
/// stessa immagine).
pub fn shapes_to_quads(shapes: &[Shape], layout: &Layout) -> Vec<Quad> {
    let s = layout.scale as f32;
    let o = (layout.margin * layout.scale) as f32;
    let px = |x: f32, y: f32| ((o + x * s).round(), (o + y * s).round());
    let color = |c: Argb| {
        [
            ((c >> 16) & 0xFF) as f32 / 255.0,
            ((c >> 8) & 0xFF) as f32 / 255.0,
            (c & 0xFF) as f32 / 255.0,
            ((c >> 24) & 0xFF) as f32 / 255.0,
        ]
    };
    let mut masks: Vec<[f32; 4]> = Vec::new();
    let mut out = Vec::new();
    let push =
        |out: &mut Vec<Quad>, masks: &[[f32; 4]], x0: f32, y0: f32, x1: f32, y1: f32, c: Argb| {
            if x1 <= x0 || y1 <= y0 {
                return;
            }
            out.push(Quad {
                rect: [x0, y0, x1, y1],
                color: color(c),
                mask0: masks.first().copied().unwrap_or(NO_MASK),
                mask1: masks.get(1).copied().unwrap_or(NO_MASK),
            });
        };
    for sh in shapes {
        match *sh {
            Shape::Mask { x, y, w, h } => {
                let (x0, y0) = px(x, y);
                let (x1, y1) = px(x + w, y + h);
                masks.push([x0, y0, x1, y1]);
            }
            Shape::Unmask => masks.clear(),
            Shape::Fill { x, y, w, h, color } => {
                let (x0, y0) = px(x, y);
                let (x1, y1) = px(x + w, y + h);
                push(&mut out, &masks, x0, y0, x1, y1, color);
            }
            Shape::Rect {
                x,
                y,
                w,
                h,
                color,
                thickness,
            } => {
                let t = thickness.max(1) as f32;
                let (x0, y0) = px(x, y);
                let (x1, y1) = px(x + w, y + h);
                push(&mut out, &masks, x0, y0, x1, y0 + t, color);
                push(&mut out, &masks, x0, y1 - t, x1, y1, color);
                push(&mut out, &masks, x0, y0 + t, x0 + t, y1 - t, color);
                push(&mut out, &masks, x1 - t, y0 + t, x1, y1 - t, color);
            }
            Shape::Line {
                x0,
                y0,
                x1,
                y1,
                color,
                thickness,
            } => {
                let t = thickness.max(1) as f32;
                let half = (t as i64 / 2) as f32;
                let (a0, b0) = px(x0, y0);
                let (a1, b1) = px(x1, y1);
                if b0 == b1 {
                    push(
                        &mut out,
                        &masks,
                        a0.min(a1),
                        b0 - half,
                        a0.max(a1),
                        b0 - half + t,
                        color,
                    );
                } else {
                    push(
                        &mut out,
                        &masks,
                        a0 - half,
                        b0.min(b1),
                        a0 - half + t,
                        b0.max(b1),
                        color,
                    );
                }
            }
        }
    }
    out
}

impl Gpu {
    /// Motore su una finestra di `minifb`.
    ///
    /// # Safety
    /// La finestra deve vivere più a lungo di questo oggetto (in `Frontend` il
    /// campo `gpu` è dichiarato prima di `win`, quindi viene distrutto prima).
    pub unsafe fn for_window(win: &minifb::Window, layout: Layout) -> Result<Gpu, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let target = unsafe { wgpu::SurfaceTargetUnsafe::from_display_and_window(win, win) }
            .map_err(|e| format!("handle della finestra: {e}"))?;
        let surface = unsafe { instance.create_surface_unsafe(target) }
            .map_err(|e| format!("superficie: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("nessuna scheda video adatta: {e}"))?;
        let (device, queue) = Self::device(&adapter)?;
        let caps = surface.get_capabilities(&adapter);
        // Formato non sRGB: i colori vanno scritti così come sono, come fa il
        // percorso software (niente conversioni di gamma).
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or(caps.formats.first().copied())
            .ok_or("la superficie non ha formati")?;
        // Il ritmo lo detta la scheda audio: niente attesa del vsync se si può.
        let present_mode = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
            .into_iter()
            .find(|m| caps.present_modes.contains(m))
            .unwrap_or(wgpu::PresentMode::Fifo);
        let (w, h) = layout.size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w,
            height: h,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: Default::default(),
        };
        surface.configure(&device, &config);
        let info = adapter.get_info();
        let adapter_info = format!(
            "{} ({:?}, {:?}, {:?})",
            info.name, info.backend, format, present_mode
        );
        Ok(Self::build(
            device,
            queue,
            Target::Surface { surface, config },
            format,
            layout,
            adapter_info,
        ))
    }

    /// Motore fuori schermo: il risultato si rilegge con `read_back`.
    pub fn offscreen(layout: Layout) -> Result<Gpu, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| format!("nessuna scheda video adatta: {e}"))?;
        let (device, queue) = Self::device(&adapter)?;
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let (w, h) = layout.size();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("destinazione"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let padded_row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rilettura"),
            size: (padded_row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let info = adapter.get_info();
        let adapter_info = format!("{} ({:?}, fuori schermo)", info.name, info.backend);
        Ok(Self::build(
            device,
            queue,
            Target::Offscreen {
                texture,
                readback,
                padded_row,
            },
            format,
            layout,
            adapter_info,
        ))
    }

    fn device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("madtv"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("dispositivo: {e}"))
    }

    fn build(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target: Target,
        format: wgpu::TextureFormat,
        layout: Layout,
        adapter_info: String,
    ) -> Gpu {
        // Immagine del gioco: memoria 0x00RRGGBB little-endian = byte B,G,R,0,
        // cioè proprio il formato Bgra8Unorm (l'alfa viene ignorato).
        let game_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gioco"),
            size: wgpu::Extent3d {
                width: WIDTH as u32,
                height: HEIGHT as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("parametri"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("risorse"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let view = game_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("risorse"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = |label, vs, fs, buffers: &[Option<wgpu::VertexBufferLayout>], blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let game_pipeline = pipeline("gioco", "vs_game", "fs_game", &[], None);
        let quad_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Quad>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4],
        };
        let quad_pipeline = pipeline(
            "forme",
            "vs_quad",
            "fs_quad",
            &[Some(quad_layout)],
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let quad_capacity = 256;
        let quads = Self::quad_buffer(&device, quad_capacity);
        Gpu {
            device,
            queue,
            target,
            format,
            game_texture,
            uniforms,
            bind_group,
            game_pipeline,
            quad_pipeline,
            quads,
            quad_capacity,
            layout,
            adapter_info,
        }
    }

    fn quad_buffer(device: &wgpu::Device, n: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("forme"),
            size: (n * std::mem::size_of::<Quad>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Disegna un frame: immagine del gioco (larga WIDTH, `src.len() / WIDTH`
    /// righe) e forme dello strato.
    pub fn render(&mut self, src: &[u32], shapes: &[Shape]) -> Result<(), String> {
        let src_rows = (src.len() / WIDTH).min(HEIGHT);
        if src_rows > 0 {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.game_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&src[..src_rows * WIDTH]),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(WIDTH as u32 * 4),
                    rows_per_image: Some(src_rows as u32),
                },
                wgpu::Extent3d {
                    width: WIDTH as u32,
                    height: src_rows as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
        let (w, h) = self.layout.size();
        let mg = (self.layout.margin * self.layout.scale) as f32;
        let u = Uniforms {
            viewport: [w as f32, h as f32],
            origin: [mg, mg],
            scale: self.layout.scale as f32,
            src_rows: src_rows as f32,
            rows: self.layout.rows as f32,
            _pad: 0.0,
        };
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        let quads = shapes_to_quads(shapes, &self.layout);
        if quads.len() > self.quad_capacity {
            self.quad_capacity = quads.len().next_power_of_two();
            self.quads = Self::quad_buffer(&self.device, self.quad_capacity);
        }
        if !quads.is_empty() {
            self.queue
                .write_buffer(&self.quads, 0, bytemuck::cast_slice(&quads));
        }

        let surface_texture = match &self.target {
            Target::Surface { surface, config } => match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t)
                | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    surface.configure(&self.device, config);
                    return Ok(());
                }
                // Finestra coperta o ridotta a icona, attesa scaduta: salta il frame.
                wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                    return Ok(());
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    return Err("superficie non valida".into());
                }
            },
            Target::Offscreen { .. } => None,
        };
        let view = match (&surface_texture, &self.target) {
            (Some(t), _) => t
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
            (None, Target::Offscreen { texture, .. }) => {
                texture.create_view(&wgpu::TextureViewDescriptor::default())
            }
            _ => unreachable!(),
        };
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_pipeline(&self.game_pipeline);
            pass.draw(0..3, 0..1);
            if !quads.is_empty() {
                pass.set_pipeline(&self.quad_pipeline);
                pass.set_vertex_buffer(0, self.quads.slice(..));
                pass.draw(0..6, 0..quads.len() as u32);
            }
        }
        if let Target::Offscreen {
            texture,
            readback,
            padded_row,
        } = &self.target
        {
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(*padded_row),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.queue.submit([enc.finish()]);
        if let Some(t) = surface_texture {
            self.queue.present(t);
        }
        Ok(())
    }

    /// Rilegge l'ultimo frame disegnato fuori schermo (0x00RRGGBB per pixel).
    pub fn read_back(&self) -> Option<Vec<u32>> {
        let Target::Offscreen {
            readback,
            padded_row,
            ..
        } = &self.target
        else {
            return None;
        };
        let (w, h) = self.layout.size();
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let data = slice.get_mapped_range().ok()?;
        let mut out = Vec::with_capacity((w * h) as usize);
        for y in 0..h as usize {
            let row = &data[y * *padded_row as usize..][..w as usize * 4];
            for p in row.chunks_exact(4) {
                // Bgra8Unorm: byte B, G, R, A.
                out.push(u32::from(p[2]) << 16 | u32::from(p[1]) << 8 | u32::from(p[0]));
            }
        }
        drop(data);
        readback.unmap();
        Some(out)
    }

    /// Formato dei pixel della destinazione (diagnostica).
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::Canvas;

    /// Disegno software di riferimento, come `window.rs`.
    fn software(src: &[u32], shapes: &[Shape], l: &Layout) -> Vec<u32> {
        let (w, h) = l.size();
        let (w, h) = (w as usize, h as usize);
        let (s, mg) = (l.scale, l.margin);
        let mut buf = vec![0u32; w * h];
        for y in 0..l.rows {
            for x in 0..WIDTH {
                let p = src.get(y * WIDTH + x).copied().unwrap_or(0);
                for dy in 0..s {
                    for dx in 0..s {
                        buf[((mg + y) * s + dy) * w + (mg + x) * s + dx] = p;
                    }
                }
            }
        }
        Canvas {
            buf: &mut buf,
            width: w,
            height: h,
            scale: s as f32,
            origin: (mg * s, mg * s),
        }
        .draw(shapes);
        buf
    }

    #[test]
    fn stesso_risultato_del_disegno_software() {
        let l = Layout {
            scale: 3,
            margin: 20,
            rows: 220,
        };
        let mut gpu = match Gpu::offscreen(l) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("test saltato, nessuna GPU (nemmeno software): {e}");
                return;
            }
        };
        // Immagine di prova: sfumature e scacchiera.
        let src: Vec<u32> = (0..WIDTH * 200)
            .map(|i| {
                let (x, y) = ((i % WIDTH) as u32, (i / WIDTH) as u32);
                (x * 255 / 320) << 16
                    | (y * 255 / 200) << 8
                    | if (x / 8 + y / 8) % 2 == 0 { 0x40 } else { 0xC0 }
            })
            .collect();
        let shapes = vec![
            Shape::Mask {
                x: 100.0,
                y: 60.0,
                w: 40.0,
                h: 20.0,
            },
            Shape::Fill {
                x: 80.0,
                y: 50.0,
                w: 100.0,
                h: 40.0,
                color: 0x5038D870,
            },
            Shape::Rect {
                x: 80.0,
                y: 50.0,
                w: 100.0,
                h: 40.0,
                color: 0xFF38D870,
                thickness: 2,
            },
            Shape::Unmask,
            Shape::Rect {
                x: 9.5,
                y: 9.5,
                w: 30.0,
                h: 12.0,
                color: 0xE0E84848,
                thickness: 2,
            },
            Shape::Line {
                x0: 200.0,
                y0: 150.5,
                x1: 260.0,
                y1: 150.5,
                color: 0xFFFFFFFF,
                thickness: 1,
            },
            Shape::Line {
                x0: 300.5,
                y0: 10.0,
                x1: 300.5,
                y1: 190.0,
                color: 0x80FFFF00,
                thickness: 3,
            },
        ];
        gpu.render(&src, &shapes).unwrap();
        let g = gpu.read_back().unwrap();
        let c = software(&src, &shapes, &l);
        assert_eq!(g.len(), c.len());
        let channel_diff = |a: u32, b: u32| {
            (0..3)
                .map(|k| (((a >> (k * 8)) & 0xFF) as i32 - ((b >> (k * 8)) & 0xFF) as i32).abs())
                .max()
                .unwrap()
        };
        let worst = g
            .iter()
            .zip(&c)
            .map(|(&a, &b)| channel_diff(a, b))
            .max()
            .unwrap();
        let different = g
            .iter()
            .zip(&c)
            .filter(|(a, b)| channel_diff(**a, **b) > 1)
            .count();
        assert!(
            different == 0,
            "{different} pixel diversi (scarto massimo {worst}) - {}",
            gpu.adapter_info
        );
    }
}
