//! Persistent Metal resources for the baked-model fixture and live terrain.
//! Terrain lives in packed column-local buffers per column and pass, drawn
//! through one shared quad index list with the column's offset as instance
//! data. Each 16-block section is frustum-culled per frame; solid then cutout
//! columns draw near to far, translucent columns far to near. Depth is
//! reversed-Z with an infinite far plane, as upstream Horizon's terrain.
use crate::{
    network::MeshUpdate,
    terrain::{FACES, Registry, TerrainPass, TerrainVertex, Vertex, quad_indices, sees},
};
use glam::{Mat4, Vec3, Vec4};
use horizon_protocol_47::play::PositionLook;
use horizon_world_189::entities::{Entity, Kind};

/// `RenderManager.shouldRender`: an entity draws (with its name tag) when a
/// generous box around it (1.5 blocks out, from half a block below to 3.5
/// above its interpolated feet) meets the view frustum. Mobs bigger than
/// that box (giants, slimes and magma cubes, ghasts, the dragon and the
/// wither) and fishing bobbers, whose line reaches the angler, always draw.
pub fn entity_in_view(planes: &[Vec4; 5], entity: &Entity, partial: f32, origin: [i64; 3]) -> bool {
    if matches!(
        entity.kind,
        Kind::Mob(53 | 55 | 56 | 62 | 63 | 64) | Kind::Object(90)
    ) {
        return true;
    }
    let at: [f32; 3] = std::array::from_fn(|axis| {
        let previous = entity.previous[axis];
        let feet = previous + (entity.position[axis] - previous) * f64::from(partial);
        (feet - origin[axis] as f64) as f32
    });
    let feet = Vec3::from_array(at);
    Scene::visible(
        planes,
        feet - Vec3::new(1.5, 0.5, 1.5),
        feet + Vec3::new(1.5, 3.5, 1.5),
    )
}
use std::{collections::HashMap, error::Error};

const MIP_LEVELS: u32 = 5;
/// Vanilla-like daytime fog and sky, sRGB (0.66, 0.78, 1.0) in linear light.
pub const SKY: [f64; 3] = [0.392, 0.571, 1.0];

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_projection: [f32; 16],
    /// Eye position relative to the mesh origin, then the fog end distance.
    eye: [f32; 4],
    /// Fog colour, then the fog start distance.
    fog: [f32; 4],
}

struct GpuMesh {
    buffer: wgpu::Buffer,
    count: u32,
}

struct GpuPass {
    buffer: wgpu::Buffer,
    sections: [u32; 17],
}

/// Solid, cutout and translucent terrain, then block entities.
struct GpuColumn {
    passes: [Option<GpuPass>; 3],
    entity: Option<GpuMesh>,
    visibility: [u64; 16],
}

/// `RenderGlobal.setupTerrain`'s occlusion walk: breadth-first from the
/// camera's section, never stepping back toward the camera, and leaving a
/// section only through a face its `VisGraph` joins to the face it was
/// entered by. Returns the reached sections of each loaded column. Frustum
/// culling is applied afterwards, so the walk only reruns when the camera
/// changes section or columns change.
pub fn reachable(
    start: [i32; 3],
    visibility: impl Fn((i32, i32)) -> Option<[u64; 16]>,
) -> HashMap<(i32, i32), u16> {
    let mut reached: HashMap<(i32, i32), u16> = HashMap::new();
    if visibility((start[0], start[2])).is_none() || !(0..16).contains(&start[1]) {
        return reached;
    }
    reached.insert((start[0], start[2]), 1 << start[1]);
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((start, None::<usize>, 0u8));
    while let Some((at, entered, directions)) = queue.pop_front() {
        let Some(sections) = visibility((at[0], at[2])) else {
            continue;
        };
        let here = sections[at[1] as usize];
        for (face, step) in FACES.iter().enumerate() {
            if directions & (1 << (face ^ 1)) != 0 {
                continue;
            }
            if let Some(entered) = entered
                && here & sees(entered ^ 1, face) == 0
            {
                continue;
            }
            let next = [at[0] + step[0], at[1] + step[1], at[2] + step[2]];
            if !(0..16).contains(&next[1]) || visibility((next[0], next[2])).is_none() {
                continue;
            }
            let mask = reached.entry((next[0], next[2])).or_default();
            if *mask & (1 << next[1]) != 0 {
                continue;
            }
            *mask |= 1 << next[1];
            queue.push_back((next, Some(face), directions | 1 << face));
        }
    }
    reached
}

/// Contiguous quad ranges of the sections in `mask`, merged where adjacent.
pub fn section_runs(mask: u16, sections: &[u32; 17]) -> impl Iterator<Item = (u32, u32)> + '_ {
    let mut section = 0;
    std::iter::from_fn(move || {
        while section < 16 {
            let (start, mut end) = (sections[section], sections[section + 1]);
            section += 1;
            if mask & (1 << (section - 1)) == 0 || start == end {
                continue;
            }
            while section < 16 && mask & (1 << section) != 0 {
                end = sections[section + 1];
                section += 1;
            }
            return Some((start, end));
        }
        None
    })
}

pub struct Scene {
    /// Draw calls issued since `take_draws`.
    draws: std::cell::Cell<u32>,
    /// The fixture and entities, in the full vertex format.
    pipeline: wgpu::RenderPipeline,
    /// Packed terrain: solid (no alpha test), cutout, translucent.
    terrain_pipelines: [wgpu::RenderPipeline; 3],
    /// `quad_indices` for `quad_capacity` quads.
    quads: wgpu::Buffer,
    quad_capacity: u32,
    /// Per-frame column offsets, one instance per visible column.
    bases: wgpu::Buffer,
    base_capacity: u64,
    /// Visible columns near to far with their visible-section masks.
    frame: Vec<((i32, i32), u16)>,
    /// Entity and label vertices go up through reused, mapped staging
    /// chunks: `queue.write_buffer` makes a fresh staging buffer per call,
    /// and a crowd's megabytes of fresh pages cost more than the copy.
    belt: wgpu::util::StagingBelt,
    upload: Option<wgpu::CommandEncoder>,
    /// The occlusion walk's result and the camera section it started from;
    /// `None` reruns it.
    reached: Option<([i32; 3], HashMap<(i32, i32), u16>)>,
    /// In gamma space after the world: block cracks, the enchantment glint over entity
    /// geometry it matches in depth, `renderLivingLabel` without and with a
    /// depth test, and `RenderFish` lines.
    label_pipelines: [wgpu::RenderPipeline; 5],
    labels: wgpu::Buffer,
    labels_capacity: u64,
    /// Vertex counts for each of `label_pipelines`, stored in that order.
    label_counts: [u32; 5],
    hand_glint: wgpu::RenderPipeline,
    /// First-person vertices, then their glint.
    hand_glint_count: u32,
    fixture: Option<GpuMesh>,
    columns: HashMap<(i32, i32), GpuColumn>,
    origin: [i64; 3],
    bindings: wgpu::BindGroup,
    entity_bindings: wgpu::BindGroup,
    entity_texture: wgpu::Texture,
    pub skin_base: u32,
    skin_capacity: u32,
    /// Moving entities, rewritten every frame.
    entities: wgpu::Buffer,
    entity_capacity: u64,
    entity_count: u32,
    /// The first-person hand: view-space vertices with their own camera.
    hand: wgpu::Buffer,
    hand_capacity: u64,
    hand_count: u32,
    hand_camera: wgpu::Buffer,
    hand_bindings: wgpu::BindGroup,
    /// The inventory's player model (`drawEntityOnScreen`), with its own
    /// interface-space camera.
    model: wgpu::Buffer,
    model_capacity: u64,
    model_count: u32,
    model_camera: wgpu::Buffer,
    model_bindings: wgpu::BindGroup,
    /// `hurtCameraEffect` and view bobbing, before the camera orientation.
    effects: Mat4,
    camera: wgpu::Buffer,
    view: Mat4,
    eye: Vec3,
    fov: f32,
    fog_end: f32,
    pub depth: wgpu::TextureView,
}

/// sRGB-encoded byte to linear light and back, for mip averaging.
fn to_linear(value: u8) -> f32 {
    let c = f32::from(value) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(value: f32) -> u8 {
    let c = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Halve an RGBA8 square: alpha-weighted colour in linear light, mean alpha.
fn downsample(source: &[u8], size: usize) -> Vec<u8> {
    let half = size / 2;
    let mut out = vec![0; half * half * 4];
    for y in 0..half {
        for x in 0..half {
            let mut colour = [0.0f32; 3];
            let mut alpha = 0.0f32;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let index = ((y * 2 + dy) * size + x * 2 + dx) * 4;
                let a = f32::from(source[index + 3]) / 255.0;
                for channel in 0..3 {
                    colour[channel] += to_linear(source[index + channel]) * a;
                }
                alpha += a;
            }
            let target = (y * half + x) * 4;
            for channel in 0..3 {
                out[target + channel] = if alpha > 0.0 {
                    to_srgb(colour[channel] / alpha)
                } else {
                    0
                };
            }
            out[target + 3] = (alpha / 4.0 * 255.0).round() as u8;
        }
    }
    out
}

impl Scene {
    /// FOV is an optical setting; the centre ray and near plane stay fixed.
    fn world_projection(fov: f32, aspect: f32) -> Mat4 {
        Mat4::perspective_infinite_reverse_rh(fov.to_radians(), aspect, 0.05)
    }

    #[cfg(test)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        jar: &std::path::Path,
    ) -> Result<Self, Box<dyn Error>> {
        Self::from_registry(device, queue, config, &Registry::open(jar)?, true, 8)
    }

    pub fn from_registry(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        registry: &Registry,
        fixture: bool,
        view_distance: u8,
    ) -> Result<Self, Box<dyn Error>> {
        let layers = registry.textures.len() as u32;
        if layers > device.limits().max_texture_array_layers {
            return Err("Texture array exceeds device limits".into());
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Owned legacy texture array"),
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: layers,
            },
            mip_level_count: MIP_LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, image) in registry.textures.iter().enumerate() {
            let mut level_data = image.rgba.clone();
            for level in 0..MIP_LEVELS {
                let size = 16u32 >> level;
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: level,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: layer as u32,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &level_data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size * 4),
                        rows_per_image: Some(size),
                    },
                    wgpu::Extent3d {
                        width: size,
                        height: size,
                        depth_or_array_layers: 1,
                    },
                );
                if size > 1 {
                    level_data = downsample(&level_data, size as usize);
                }
            }
        }
        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Persistent camera uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Native legacy terrain"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Legacy terrain resources"),
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Legacy terrain layout"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let make_pipeline = |translucent: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if translucent {
                    "Native translucent terrain"
                } else {
                    "Native textured terrain"
                }),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0=>Float32x3, 1=>Float32x2, 2=>Uint32, 3=>Float32x3, 4=>Float32],
                    })],
                },
                // Vanilla culls back faces of terrain; baked quads are
                // counter-clockwise from outside. Translucent water is seen
                // from below too, so that pass draws both sides.
                primitive: wgpu::PrimitiveState {
                    cull_mode: (!translucent).then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(!translucent),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: translucent.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make_pipeline(false);
        let make_terrain = |label: &str, entry: &str, translucent: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("terrain_vertex"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<TerrainVertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0=>Uint16x4, 1=>Uint16x2, 2=>Unorm8x4],
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 12,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &wgpu::vertex_attr_array![3=>Float32x3],
                        }),
                    ],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: (!translucent).then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(!translucent),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: translucent.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let terrain_pipelines = [
            make_terrain("Solid terrain", "terrain_solid", false),
            make_terrain("Cutout terrain", "terrain_cutout", false),
            make_terrain("Translucent terrain", "terrain_cutout", true),
        ];
        // Overlays drawn after the world: labels, lines and the glint.
        let additive = wgpu::BlendState {
            // `glBlendFunc(GL_SRC_COLOR, GL_ONE)`.
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Src,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let make_overlay = |label: &str,
                            compare: wgpu::CompareFunction,
                            topology: wgpu::PrimitiveTopology,
                            blend: wgpu::BlendState,
                            entry: &str,
                            format: wgpu::TextureFormat,
                            bias: wgpu::DepthBiasState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0=>Float32x3, 1=>Float32x2, 2=>Uint32, 3=>Float32x3, 4=>Float32],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias,
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let plain = config.format.remove_srgb_suffix();
        let triangles = wgpu::PrimitiveTopology::TriangleList;
        let label_pipelines = [
            // `drawBlockDamageTexture`: polygon offset toward the eye and
            // `glBlendFunc(GL_DST_COLOR, GL_SRC_COLOR)`.
            make_overlay(
                "Block cracks",
                wgpu::CompareFunction::GreaterEqual,
                triangles,
                wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Dst,
                        dst_factor: wgpu::BlendFactor::Src,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Zero,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                },
                "crack_fragment",
                plain,
                wgpu::DepthBiasState {
                    constant: 3,
                    slope_scale: 3.0,
                    clamp: 0.0,
                },
            ),
            make_overlay(
                "Enchantment glint",
                wgpu::CompareFunction::Equal,
                triangles,
                additive,
                "glint_gamma",
                plain,
                Default::default(),
            ),
            make_overlay(
                "See-through labels",
                wgpu::CompareFunction::Always,
                triangles,
                wgpu::BlendState::ALPHA_BLENDING,
                "label_fragment",
                plain,
                Default::default(),
            ),
            make_overlay(
                "Depth-tested labels",
                wgpu::CompareFunction::GreaterEqual,
                triangles,
                wgpu::BlendState::ALPHA_BLENDING,
                "label_fragment",
                plain,
                Default::default(),
            ),
            make_overlay(
                "Fishing lines",
                wgpu::CompareFunction::GreaterEqual,
                wgpu::PrimitiveTopology::LineList,
                wgpu::BlendState::ALPHA_BLENDING,
                "label_fragment",
                plain,
                Default::default(),
            ),
        ];
        // The hand pass draws to the sRGB view, so its glint outputs linear.
        let hand_glint = make_overlay(
            "First-person glint",
            wgpu::CompareFunction::Equal,
            triangles,
            additive,
            "glint_linear",
            config.format,
            Default::default(),
        );
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        // GL_NEAREST_MIPMAP_LINEAR, as vanilla's mipmapped terrain atlas. Each sprite
        // is its own layer, so repeating tiles a greedy-merged quad's sprite once per
        // block; with nearest texels a 0-1 quad samples exactly as with clamping.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Legacy terrain sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Legacy terrain resources"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let entity_size = crate::models::LAYER_SIZE;
        let skin_base = registry.entity_textures.len() as u32;
        let entity_layers = (skin_base + 256).min(device.limits().max_texture_array_layers);
        let entity_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Owned legacy entity textures"),
            size: wgpu::Extent3d {
                width: entity_size,
                height: entity_size,
                depth_or_array_layers: entity_layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, image) in registry.entity_textures.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &entity_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &image.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(entity_size * 4),
                    rows_per_image: Some(entity_size),
                },
                wgpu::Extent3d {
                    width: entity_size,
                    height: entity_size,
                    depth_or_array_layers: 1,
                },
            );
        }
        let entity_view = entity_texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let entity_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Legacy entity sampler"),
            ..Default::default()
        });
        let entity_bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Legacy entity resources"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&entity_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&entity_sampler),
                },
            ],
        });
        let hand_camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("First-person camera uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hand_bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("First-person resources"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: hand_camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&entity_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&entity_sampler),
                },
            ],
        });
        let model_camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Inventory model camera uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let model_bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Inventory model resources"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: model_camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&entity_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&entity_sampler),
                },
            ],
        });
        let model_capacity = 64 * 1024;
        let model = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Inventory model vertices"),
            size: model_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let hand_capacity = 64 * 1024;
        let hand = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("First-person vertices"),
            size: hand_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let labels_capacity = 64 * 1024;
        let labels = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Label vertices"),
            size: labels_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entity_capacity = 64 * 1024;
        let entities = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Entity vertices"),
            size: entity_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let eye = Vec3::new(2.5, 1.8, 2.5);
        let quad_capacity = 1 << 14;
        let quads = Self::quad_buffer(device, queue, quad_capacity);
        let base_capacity = 1024;
        let bases = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Column offsets"),
            size: base_capacity * 12,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut scene = Self {
            draws: std::cell::Cell::new(0),
            pipeline,
            terrain_pipelines,
            quads,
            quad_capacity,
            bases,
            base_capacity,
            frame: Vec::new(),
            belt: wgpu::util::StagingBelt::new(device.clone(), 8 << 20),
            upload: None,
            reached: None,
            label_pipelines,
            labels,
            labels_capacity,
            label_counts: [0; 5],
            hand_glint,
            hand_glint_count: 0,
            fixture: None,
            columns: HashMap::new(),
            origin: [0; 3],
            bindings,
            entity_bindings,
            entity_texture,
            skin_base,
            skin_capacity: entity_layers,
            entities,
            entity_capacity,
            entity_count: 0,
            hand,
            hand_capacity,
            hand_count: 0,
            hand_camera,
            hand_bindings,
            model,
            model_capacity,
            model_count: 0,
            model_camera,
            model_bindings,
            effects: Mat4::IDENTITY,
            camera,
            view: Mat4::look_at_rh(eye, Vec3::splat(0.5), Vec3::Y),
            eye,
            fov: 60.0,
            fog_end: f32::from(view_distance.clamp(2, 16)) * 16.0,
            depth: Self::depth_view(device, config),
        };
        scene.write_camera(queue, config);
        if fixture {
            let vertices = registry.stone_fixture();
            scene.fixture = Self::mesh_buffer(device, queue, &vertices, "Fixture vertices")?;
        }
        eprintln!(
            "TEXTURE_ARRAY_READY layers={layers} mips={MIP_LEVELS} fixture_vertices={}",
            scene.fixture.as_ref().map_or(0, |mesh| mesh.count)
        );
        Ok(scene)
    }

    fn mesh_buffer(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[Vertex],
        label: &str,
    ) -> Result<Option<GpuMesh>, Box<dyn Error>> {
        if vertices.is_empty() {
            return Ok(None);
        }
        let bytes: &[u8] = bytemuck::cast_slice(vertices);
        if bytes.len() as u64 > device.limits().max_buffer_size {
            return Err("Column mesh exceeds device buffer limit".into());
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytes);
        Ok(Some(GpuMesh {
            buffer,
            count: u32::try_from(vertices.len())?,
        }))
    }

    fn quad_buffer(device: &wgpu::Device, queue: &wgpu::Queue, quads: u32) -> wgpu::Buffer {
        let indices = quad_indices(quads);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shared quad indices"),
            size: (indices.len() * 4) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&indices));
        buffer
    }

    fn pass_buffer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &TerrainPass,
        label: &str,
    ) -> Result<Option<GpuPass>, Box<dyn Error>> {
        if pass.vertices.is_empty() {
            return Ok(None);
        }
        let bytes: &[u8] = bytemuck::cast_slice(&pass.vertices);
        if bytes.len() as u64 > device.limits().max_buffer_size {
            return Err("Column mesh exceeds device buffer limit".into());
        }
        if pass.quads() > self.quad_capacity {
            self.quad_capacity = pass.quads().next_power_of_two();
            self.quads = Self::quad_buffer(device, queue, self.quad_capacity);
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytes);
        Ok(Some(GpuPass {
            buffer,
            sections: pass.sections,
        }))
    }

    /// Replace or remove one column's buffers. Columns meshed against an
    /// older origin are dropped once the origin moves.
    pub fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        update: MeshUpdate,
    ) -> Result<(), Box<dyn Error>> {
        if update.origin != self.origin {
            self.columns.clear();
            self.origin = update.origin;
            self.reached = None;
        }
        match update.mesh {
            None => {
                if self.columns.remove(&update.key).is_some() {
                    self.reached = None;
                }
            }
            Some(mesh) => {
                // Block edits that keep every section's face connectivity
                // leave the occlusion walk as it was.
                if self
                    .columns
                    .get(&update.key)
                    .is_none_or(|old| old.visibility != mesh.visibility)
                {
                    self.reached = None;
                }
                let column = GpuColumn {
                    visibility: mesh.visibility,
                    passes: [
                        self.pass_buffer(device, queue, &mesh.solid, "Column solid")?,
                        self.pass_buffer(device, queue, &mesh.cutout, "Column cutout")?,
                        self.pass_buffer(device, queue, &mesh.translucent, "Column translucent")?,
                    ],
                    entity: Self::mesh_buffer(
                        device,
                        queue,
                        &mesh.entity,
                        "Column block entities",
                    )?,
                };
                self.columns.insert(update.key, column);
            }
        }
        Ok(())
    }

    pub fn upload_skin(
        &self,
        queue: &wgpu::Queue,
        layer: u32,
        loaded: &crate::skins::Loaded,
    ) -> bool {
        if layer < self.skin_base || layer + 1 >= self.skin_capacity {
            return false;
        }
        for (index, image) in [
            (layer, Some(&loaded.skin)),
            (layer + 1, loaded.cape.as_ref()),
        ] {
            let Some(image) = image else { continue };
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.entity_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: index,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &image.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.width * 4),
                    rows_per_image: Some(image.height),
                },
                wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        true
    }

    /// Replace this frame's moving-entity vertices.
    pub fn set_entities(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[Vertex],
    ) -> Result<(), Box<dyn Error>> {
        let bytes: &[u8] = bytemuck::cast_slice(vertices);
        if bytes.len() as u64 > self.entity_capacity {
            self.entity_capacity = (bytes.len() as u64)
                .next_power_of_two()
                .min(device.limits().max_buffer_size);
            if bytes.len() as u64 > self.entity_capacity {
                return Err("Entity mesh exceeds device buffer limit".into());
            }
            self.entities = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Entity vertices"),
                size: self.entity_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        // A frame that never rendered left its upload unsent: send it now.
        if let Some(encoder) = self.upload.take() {
            self.belt.finish();
            queue.submit([encoder.finish()]);
            self.belt.recall();
        }
        if let Some(size) = wgpu::BufferSize::new(bytes.len() as u64) {
            let encoder = self.upload.get_or_insert_with(|| {
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Entity upload"),
                })
            });
            self.belt
                .write_buffer(encoder, &self.entities, 0, size)
                .copy_from_slice(bytes);
        }
        self.entity_count = u32::try_from(vertices.len())?;
        Ok(())
    }

    /// This frame's staged uploads, to submit before its draws.
    pub fn take_upload(&mut self) -> Option<wgpu::CommandBuffer> {
        let encoder = self.upload.take()?;
        self.belt.finish();
        Some(encoder.finish())
    }

    /// After submitting: staging chunks come back once the GPU is done
    /// with them (on a later `device.poll`).
    pub fn recall(&mut self) {
        self.belt.recall();
    }

    /// Replace this frame's overlays, in `label_pipelines` order: glint,
    /// labels drawn over everything, labels only where nothing nearer hides
    /// them, and depth-tested line segments.
    pub fn set_labels(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lists: [&[Vertex]; 5],
    ) -> Result<(), Box<dyn Error>> {
        let size: usize = lists.iter().map(|list| std::mem::size_of_val(*list)).sum();
        if size as u64 > self.labels_capacity {
            self.labels_capacity = (size as u64)
                .next_power_of_two()
                .min(device.limits().max_buffer_size);
            if size as u64 > self.labels_capacity {
                return Err("Label mesh exceeds device buffer limit".into());
            }
            self.labels = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Label vertices"),
                size: self.labels_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let _ = queue;
        if let Some(total) = wgpu::BufferSize::new(size as u64) {
            let encoder = self.upload.get_or_insert_with(|| {
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Label upload"),
                })
            });
            let mut view = self.belt.write_buffer(encoder, &self.labels, 0, total);
            let mut offset = 0;
            for list in &lists {
                let bytes: &[u8] = bytemuck::cast_slice(list);
                view.slice(offset..offset + bytes.len())
                    .copy_from_slice(bytes);
                offset += bytes.len();
            }
        }
        for (index, list) in lists.iter().enumerate() {
            self.label_counts[index] = u32::try_from(list.len())?;
        }
        Ok(())
    }

    pub fn has_labels(&self) -> bool {
        self.label_counts.iter().any(|&count| count > 0)
    }

    /// Labels, in a gamma-space pass that keeps the world's depth.
    pub fn draw_labels(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.entity_bindings, &[]);
        pass.set_vertex_buffer(0, self.labels.slice(..));
        let mut start = 0;
        for (pipeline, &count) in self.label_pipelines.iter().zip(&self.label_counts) {
            if count > 0 {
                pass.set_pipeline(pipeline);
                self.draws.set(self.draws.get() + 1);
                pass.draw(start..start + count, 0..1);
            }
            start += count;
        }
    }

    /// Camera effects for the next `set_camera`.
    /// Render distance changed: fog moves with it.
    pub fn set_view_distance(
        &mut self,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        chunks: u8,
    ) {
        self.fog_end = f32::from(chunks.clamp(2, 16)) * 16.0;
        self.write_camera(queue, config);
    }

    pub fn set_effects(&mut self, effects: Mat4) {
        self.effects = effects;
    }

    /// Replace the first-person hand: view-space vertices drawn with a
    /// 70-degree projection, as `renderHand` uses whatever the FOV setting.
    pub fn set_hand(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        vertices: &[Vertex],
        glint: &[Vertex],
    ) -> Result<(), Box<dyn Error>> {
        let projection = Mat4::perspective_infinite_reverse_rh(
            70f32.to_radians(),
            config.width as f32 / config.height.max(1) as f32,
            0.05,
        );
        let uniforms = Uniforms {
            view_projection: projection.to_cols_array(),
            eye: [0.0, 0.0, 0.0, 1.0e6],
            fog: [0.0, 0.0, 0.0, 1.0e6],
        };
        queue.write_buffer(&self.hand_camera, 0, bytemuck::bytes_of(&uniforms));
        let bytes: &[u8] = bytemuck::cast_slice(vertices);
        let glint_bytes: &[u8] = bytemuck::cast_slice(glint);
        let total = (bytes.len() + glint_bytes.len()) as u64;
        if total > self.hand_capacity {
            self.hand_capacity = total
                .next_power_of_two()
                .min(device.limits().max_buffer_size);
            if total > self.hand_capacity {
                return Err("Hand mesh exceeds device buffer limit".into());
            }
            self.hand = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("First-person vertices"),
                size: self.hand_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.hand, 0, bytes);
        }
        if !glint_bytes.is_empty() {
            queue.write_buffer(&self.hand, bytes.len() as u64, glint_bytes);
        }
        self.hand_count = u32::try_from(vertices.len())?;
        self.hand_glint_count = u32::try_from(glint.len())?;
        Ok(())
    }

    /// The hand, in a pass whose depth was cleared.
    /// Replace the inventory's player model: vertices in entity space and
    /// the matrix taking them to clip space.
    pub fn set_inventory_model(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[Vertex],
        clip: Mat4,
    ) -> Result<(), Box<dyn Error>> {
        let uniforms = Uniforms {
            view_projection: clip.to_cols_array(),
            eye: [0.0, 0.0, 0.0, 1.0e6],
            fog: [0.0, 0.0, 0.0, 1.0e6],
        };
        queue.write_buffer(&self.model_camera, 0, bytemuck::bytes_of(&uniforms));
        let bytes: &[u8] = bytemuck::cast_slice(vertices);
        if bytes.len() as u64 > self.model_capacity {
            self.model_capacity = (bytes.len() as u64)
                .next_power_of_two()
                .min(device.limits().max_buffer_size);
            if bytes.len() as u64 > self.model_capacity {
                return Err("Inventory model exceeds device buffer limit".into());
            }
            self.model = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Inventory model vertices"),
                size: self.model_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.model, 0, bytes);
        }
        self.model_count = u32::try_from(vertices.len())?;
        Ok(())
    }

    pub fn has_inventory_model(&self) -> bool {
        self.model_count > 0
    }

    pub fn draw_inventory_model(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.model_count > 0 {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.model_bindings, &[]);
            pass.set_vertex_buffer(0, self.model.slice(..));
            self.draws.set(self.draws.get() + 1);
            pass.draw(0..self.model_count, 0..1);
        }
    }

    pub fn has_hand(&self) -> bool {
        self.hand_count > 0
    }

    pub fn draw_hand(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.hand_count > 0 {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.hand_bindings, &[]);
            pass.set_vertex_buffer(0, self.hand.slice(..));
            self.draws.set(self.draws.get() + 1);
            pass.draw(0..self.hand_count, 0..1);
            if self.hand_glint_count > 0 {
                pass.set_pipeline(&self.hand_glint);
                self.draws.set(self.draws.get() + 1);
                pass.draw(
                    self.hand_count..self.hand_count + self.hand_glint_count,
                    0..1,
                );
            }
        }
    }

    pub fn origin(&self) -> [i64; 3] {
        self.origin
    }

    /// The entity/item array the interface samples icons from.
    pub fn entity_texture(&self) -> &wgpu::Texture {
        &self.entity_texture
    }

    pub fn entity_layers(&self) -> u32 {
        self.skin_capacity
    }

    /// Keep another scene's geometry after a surface format change.
    pub fn adopt(&mut self, old: Scene) {
        self.columns = old.columns;
        self.reached = None;
        self.quads = old.quads;
        self.quad_capacity = old.quad_capacity;
        self.origin = old.origin;
        self.fixture = old.fixture.or(self.fixture.take());
    }

    /// Drop all geometry from a finished connection while retaining GPU resources.
    pub fn clear_world(&mut self) {
        self.columns.clear();
        self.frame.clear();
        self.reached = None;
        self.fixture = None;
        self.origin = [0; 3];
        self.entity_count = 0;
        self.label_counts = [0; 5];
        self.hand_count = 0;
        self.hand_glint_count = 0;
        self.model_count = 0;
        self.effects = Mat4::IDENTITY;
    }

    /// F3's `C:`: sections drawn this frame that have geometry.
    pub fn drawn_sections(&self) -> u32 {
        self.frame
            .iter()
            .filter_map(|(key, mask)| Some((self.columns.get(key)?, *mask)))
            .map(|(column, mask)| {
                (0..16)
                    .filter(|&s| {
                        mask & (1 << s) != 0
                            && column
                                .passes
                                .iter()
                                .flatten()
                                .any(|pass| pass.sections[s + 1] > pass.sections[s])
                    })
                    .count() as u32
            })
            .sum()
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub fn has_geometry(&self) -> bool {
        self.columns
            .values()
            .any(|column| column.passes.iter().any(Option::is_some) || column.entity.is_some())
    }

    fn depth_view(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Terrain depth"),
                size: wgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    fn projection(&self, config: &wgpu::SurfaceConfiguration) -> Mat4 {
        Self::world_projection(self.fov, config.width as f32 / config.height.max(1) as f32)
    }

    fn write_camera(&self, queue: &wgpu::Queue, config: &wgpu::SurfaceConfiguration) {
        let uniforms = Uniforms {
            view_projection: (self.projection(config) * self.view).to_cols_array(),
            eye: [self.eye.x, self.eye.y, self.eye.z, self.fog_end],
            fog: [
                SKY[0] as f32,
                SKY[1] as f32,
                SKY[2] as f32,
                self.fog_end * 0.75,
            ],
        };
        queue.write_buffer(&self.camera, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Eye pose in world coordinates; the view is relative to the mesh origin.
    pub fn set_camera(
        &mut self,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        pose: PositionLook,
        fov: f32,
    ) {
        let eye = Vec3::new(
            (pose.position[0] - self.origin[0] as f64) as f32,
            (pose.position[1] - self.origin[1] as f64) as f32,
            (pose.position[2] - self.origin[2] as f64) as f32,
        );
        let (sy, cy) = pose.yaw.to_radians().sin_cos();
        let (sp, cp) = pose.pitch.to_radians().sin_cos();
        self.view =
            self.effects * Mat4::look_at_rh(eye, eye + Vec3::new(-sy * cp, -sp, cy * cp), Vec3::Y);
        self.eye = eye;
        self.fov = fov;
        self.write_camera(queue, config);
    }

    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
    ) {
        self.depth = Self::depth_view(device, config);
        self.write_camera(queue, config);
    }

    /// Clip-space planes (left, right, bottom, top, near) of the current view.
    pub fn planes(&self, config: &wgpu::SurfaceConfiguration) -> [Vec4; 5] {
        let m = (self.projection(config) * self.view).transpose();
        let (r0, r1, r2, r3) = (m.x_axis, m.y_axis, m.z_axis, m.w_axis);
        [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r3 - r2]
    }

    fn visible(planes: &[Vec4; 5], min: Vec3, max: Vec3) -> bool {
        planes.iter().all(|plane| {
            let corner = Vec3::new(
                if plane.x >= 0.0 { max.x } else { min.x },
                if plane.y >= 0.0 { max.y } else { min.y },
                if plane.z >= 0.0 { max.z } else { min.z },
            );
            plane.truncate().dot(corner) + plane.w >= 0.0
        })
    }

    /// Cull columns and their sections for this frame, nearest first, and
    /// upload the visible columns' offsets.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
    ) {
        let planes = self.planes(config);
        let camera = [
            ((f64::from(self.eye.x) + self.origin[0] as f64) / 16.0).floor() as i32,
            ((f64::from(self.eye.y) + self.origin[1] as f64) / 16.0).floor() as i32,
            ((f64::from(self.eye.z) + self.origin[2] as f64) / 16.0).floor() as i32,
        ];
        if self
            .reached
            .as_ref()
            .is_none_or(|(from, _)| *from != camera)
        {
            let columns = &self.columns;
            let walk = reachable(camera, |key| columns.get(&key).map(|c| c.visibility));
            self.reached = Some((camera, walk));
        }
        // Outside the loaded world (or above or below it) nothing occludes.
        let occlusion = self
            .reached
            .as_ref()
            .filter(|(_, walk)| !walk.is_empty())
            .map(|(_, walk)| walk);
        let mut visible: Vec<(f32, (i32, i32), u16, Vec3)> = Vec::new();
        for (&(x, z), column) in &self.columns {
            let min = Vec3::new(
                (i64::from(x) * 16 - self.origin[0]) as f32,
                -self.origin[1] as f32,
                (i64::from(z) * 16 - self.origin[2]) as f32,
            );
            if !Self::visible(&planes, min, min + Vec3::new(16.0, 256.0, 16.0)) {
                continue;
            }
            let reach = occlusion.map_or(u16::MAX, |walk| walk.get(&(x, z)).copied().unwrap_or(0));
            let mut mask = 0u16;
            for section in (0..16).filter(|section| reach & (1 << section) != 0) {
                let low = min + Vec3::new(0.0, section as f32 * 16.0, 0.0);
                if Self::visible(&planes, low, low + Vec3::splat(16.0)) {
                    mask |= 1 << section;
                }
            }
            // Block entities are culled with the column, not by section.
            if mask == 0 && column.entity.is_none() {
                continue;
            }
            let centre = min + Vec3::new(8.0, 128.0, 8.0);
            let distance =
                Vec3::new(centre.x - self.eye.x, 0.0, centre.z - self.eye.z).length_squared();
            visible.push((distance, (x, z), mask, min));
        }
        visible.sort_by(|a, b| a.0.total_cmp(&b.0));
        if visible.len() as u64 > self.base_capacity {
            self.base_capacity = (visible.len() as u64).next_power_of_two();
            self.bases = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Column offsets"),
                size: self.base_capacity * 12,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let bases: Vec<[f32; 3]> = visible.iter().map(|entry| entry.3.to_array()).collect();
        if !bases.is_empty() {
            queue.write_buffer(&self.bases, 0, bytemuck::cast_slice(&bases));
        }
        self.frame.clear();
        self.frame
            .extend(visible.iter().map(|&(_, key, mask, _)| (key, mask)));
    }

    fn draw_terrain(&self, pass: &mut wgpu::RenderPass<'_>, which: usize, order: &[usize]) {
        pass.set_pipeline(&self.terrain_pipelines[which]);
        for &instance in order {
            let (key, mask) = self.frame[instance];
            let Some(gpu) = self
                .columns
                .get(&key)
                .and_then(|c| c.passes[which].as_ref())
            else {
                continue;
            };
            pass.set_vertex_buffer(0, gpu.buffer.slice(..));
            let instance = instance as u32;
            for (start, end) in section_runs(mask, &gpu.sections) {
                self.draws.set(self.draws.get() + 1);
                pass.draw_indexed(start * 6..end * 6, 0, instance..instance + 1);
            }
        }
    }

    /// Draw calls since the last call, for the benchmark.
    pub fn take_draws(&self) -> u32 {
        self.draws.replace(0)
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bindings, &[]);
        if let Some(fixture) = &self.fixture {
            pass.set_vertex_buffer(0, fixture.buffer.slice(..));
            self.draws.set(self.draws.get() + 1);
            pass.draw(0..fixture.count, 0..1);
        }
        let near_to_far: Vec<usize> = (0..self.frame.len()).collect();
        if !near_to_far.is_empty() {
            pass.set_index_buffer(self.quads.slice(..), wgpu::IndexFormat::Uint32);
            pass.set_vertex_buffer(1, self.bases.slice(..));
            self.draw_terrain(pass, 0, &near_to_far);
            self.draw_terrain(pass, 1, &near_to_far);
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.entity_bindings, &[]);
        for (key, _) in &self.frame {
            if let Some(mesh) = self.columns.get(key).and_then(|c| c.entity.as_ref()) {
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                self.draws.set(self.draws.get() + 1);
                pass.draw(0..mesh.count, 0..1);
            }
        }
        if self.entity_count > 0 {
            pass.set_vertex_buffer(0, self.entities.slice(..));
            self.draws.set(self.draws.get() + 1);
            pass.draw(0..self.entity_count, 0..1);
        }
        pass.set_bind_group(0, &self.bindings, &[]);
        if !near_to_far.is_empty() {
            pass.set_vertex_buffer(1, self.bases.slice(..));
            let far_to_near: Vec<usize> = near_to_far.into_iter().rev().collect();
            self.draw_terrain(pass, 2, &far_to_near);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_downsampling_ignores_transparent_texels_and_keeps_srgb() {
        let mut texels = vec![0u8; 2 * 2 * 4];
        texels[..4].copy_from_slice(&[200, 100, 50, 255]);
        let half = downsample(&texels, 2);
        // Colour comes only from the opaque texel; alpha is the mean.
        assert_eq!(&half[..3], &[200, 100, 50]);
        assert_eq!(half[3], 64);
        assert_eq!(to_srgb(to_linear(173)), 173);
    }

    #[test]
    fn world_fov_widens_edges_without_moving_the_centre_ray_or_near_plane() {
        let point = Vec3::new(1.0, 0.0, -1.0);
        assert!(Scene::world_projection(70.0, 1.0).project_point3(point).x > 1.0);
        assert!(Scene::world_projection(110.0, 1.0).project_point3(point).x < 1.0);
        for fov in [30.0, 70.0, 110.0] {
            let p = Scene::world_projection(fov, 1.6);
            let near = p.inverse().project_point3(Vec3::new(0.0, 0.0, 1.0));
            assert!((near - Vec3::new(0.0, 0.0, -0.05)).length() < 1e-6);
            assert!((near.normalize() - Vec3::NEG_Z).length() < 1e-6);
        }
    }

    #[test]
    fn entities_behind_or_beside_the_view_are_not_drawn() {
        // Looking along +Z from the origin with a 70-degree FOV.
        let projection = Scene::world_projection(70.0, 16.0 / 10.0);
        let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::Z, Vec3::Y);
        let m = (projection * view).transpose();
        let (r0, r1, r2, r3) = (m.x_axis, m.y_axis, m.z_axis, m.w_axis);
        let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r3 - r2];
        let mut tracked = horizon_world_189::entities::Entities::default();
        tracked.apply(horizon_protocol_47::entities::EntityPacket::SpawnPlayer {
            id: 1,
            uuid: 7,
            position: [0; 3],
            yaw: 0,
            pitch: 0,
            held_item: 0,
            metadata: vec![],
        });
        let base = tracked.get(1).expect("spawned").clone();
        let at = |kind, x: f64, z: f64| {
            let mut entity = base.clone();
            entity.kind = kind;
            entity.position = [x, -1.6, z];
            entity.previous = entity.position;
            entity_in_view(&planes, &entity, 0.5, [0; 3])
        };
        let villager = Kind::Mob(120);
        assert!(at(villager, 0.0, 10.0));
        assert!(!at(villager, 0.0, -10.0));
        assert!(!at(villager, 40.0, 5.0));
        // Just outside the edge, the generous box still reaches in.
        assert!(at(villager, 7.0, 5.0));
        // Oversized mobs and bobbers always draw.
        assert!(at(Kind::Mob(56), 0.0, -10.0));
        assert!(at(Kind::Object(90), 0.0, -10.0));
        // Interpolation uses the partial tick.
        let mut moving = base.clone();
        moving.kind = villager;
        moving.previous = [0.0, -1.6, -10.0];
        moving.position = [0.0, -1.6, 10.0];
        assert!(!entity_in_view(&planes, &moving, 0.1, [0; 3]));
        assert!(entity_in_view(&planes, &moving, 0.9, [0; 3]));
    }

    #[test]
    fn section_runs_merge_adjacent_visible_sections() {
        let mut sections = [0u32; 17];
        // Sections 0..4 hold 1, 2, 0, 3 quads; the rest are empty.
        for (index, start) in [0, 1, 3, 3, 6].into_iter().enumerate() {
            sections[index] = start;
        }
        for start in &mut sections[5..] {
            *start = 6;
        }
        let runs = |mask| section_runs(mask, &sections).collect::<Vec<_>>();
        assert_eq!(runs(u16::MAX), [(0, 6)]);
        assert_eq!(runs(0b1011), [(0, 3), (3, 6)]);
        assert_eq!(runs(0b0101), [(0, 1)]);
        assert_eq!(runs(0b1100), [(3, 6)]);
        assert_eq!(runs(0), []);
    }

    #[test]
    fn occlusion_walk_stops_at_sealed_sections_and_never_turns_back() {
        use crate::terrain::{ALL_VISIBLE, sees};
        let open = [ALL_VISIBLE; 16];
        // Column (1, 0) is solid rock below y = 64: nothing joins there.
        let mut rock = [ALL_VISIBLE; 16];
        for section in &mut rock[..4] {
            *section = 0;
        }
        let world = |key: (i32, i32)| match key {
            (1, 0) => Some(rock),
            (x, 0) if (-1..=3).contains(&x) => Some(open),
            _ => None,
        };
        let reached = reachable([0, 4, 0], world);
        assert_eq!(reached[&(0, 0)], u16::MAX);
        // Sealed sections are entered but not passed through.
        assert_eq!(reached[&(1, 0)], u16::MAX);
        // Below the rock at x = 2 is reached only by going down there,
        // never by turning back up through another column.
        assert_eq!(reached[&(2, 0)], u16::MAX);
        assert!(!reached.contains_key(&(4, 0)));
        // A section seeing only up and down passes a vertical walk but
        // blocks a sideways one.
        let shaft = sees(0, 1) | sees(1, 0);
        let mut tube = [ALL_VISIBLE; 16];
        tube[4] = shaft;
        let column = |key: (i32, i32)| match key {
            (0, 0) => Some(tube),
            (x, 0) if (0..=2).contains(&x) => Some(open),
            _ => None,
        };
        let from_above = reachable([0, 6, 0], column);
        assert_ne!(from_above[&(0, 0)] & (1 << 3), 0);
        // Entered sideways, the shaft leads nowhere; the sections below it
        // are only reachable by turning back, which the walk never does.
        let mut ledge = [ALL_VISIBLE; 16];
        for section in &mut ledge[..4] {
            *section = 0;
        }
        let beside = reachable([1, 4, 0], |key| match key {
            (1, 0) => Some(ledge),
            _ => column(key),
        });
        assert_ne!(beside[&(0, 0)] & (1 << 4), 0);
        assert_eq!(beside[&(0, 0)] & 0b1111, 0);
        assert!(reachable([0, 16, 0], world).is_empty());
        assert!(reachable([9, 4, 0], world).is_empty());
    }

    #[test]
    fn reversed_infinite_frustum_rejects_columns_behind_the_eye() {
        let projection = Mat4::perspective_infinite_reverse_rh(70f32.to_radians(), 1.6, 0.05);
        let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), Vec3::Y);
        let m = (projection * view).transpose();
        let (r0, r1, r2, r3) = (m.x_axis, m.y_axis, m.z_axis, m.w_axis);
        let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r3 - r2];
        let ahead = Scene::visible(
            &planes,
            Vec3::new(-8.0, -8.0, 20.0),
            Vec3::new(8.0, 8.0, 36.0),
        );
        let behind = Scene::visible(
            &planes,
            Vec3::new(-8.0, -8.0, -36.0),
            Vec3::new(8.0, 8.0, -20.0),
        );
        let far = Scene::visible(
            &planes,
            Vec3::new(-8.0, -8.0, 9000.0),
            Vec3::new(8.0, 8.0, 9016.0),
        );
        assert!(ahead && far);
        assert!(!behind);
    }
}
