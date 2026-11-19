//! Persistent Metal resources for the baked-model fixture and live terrain.
//! Terrain lives in one vertex buffer per column, frustum-culled per frame;
//! opaque columns draw near to far, translucent columns far to near. Depth is
//! reversed-Z with an infinite far plane, as upstream Horizon's terrain.
use crate::{
    network::MeshUpdate,
    terrain::{Registry, Vertex},
};
use glam::{Mat4, Vec3, Vec4};
use horizon_protocol_47::play::PositionLook;
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

struct GpuColumn {
    opaque: Option<GpuMesh>,
    translucent: Option<GpuMesh>,
    entity: Option<GpuMesh>,
}

pub struct Scene {
    pipeline: wgpu::RenderPipeline,
    translucent_pipeline: wgpu::RenderPipeline,
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
        let translucent_pipeline = make_pipeline(true);
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        // GL_NEAREST_MIPMAP_LINEAR, as vanilla's mipmapped terrain atlas.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Legacy terrain sampler"),
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
        let entity_capacity = 64 * 1024;
        let entities = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Entity vertices"),
            size: entity_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let eye = Vec3::new(2.5, 1.8, 2.5);
        let mut scene = Self {
            pipeline,
            translucent_pipeline,
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
        }
        match update.mesh {
            None => {
                self.columns.remove(&update.key);
            }
            Some(mesh) => {
                let column = GpuColumn {
                    opaque: Self::mesh_buffer(device, queue, &mesh.opaque, "Column opaque")?,
                    translucent: Self::mesh_buffer(
                        device,
                        queue,
                        &mesh.translucent,
                        "Column translucent",
                    )?,
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
        if !bytes.is_empty() {
            queue.write_buffer(&self.entities, 0, bytes);
        }
        self.entity_count = u32::try_from(vertices.len())?;
        Ok(())
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
        self.origin = old.origin;
        self.fixture = old.fixture.or(self.fixture.take());
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub fn has_geometry(&self) -> bool {
        self.columns.values().any(|column| {
            column.opaque.is_some() || column.translucent.is_some() || column.entity.is_some()
        })
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
        Mat4::perspective_infinite_reverse_rh(
            self.fov.to_radians(),
            config.width as f32 / config.height.max(1) as f32,
            0.05,
        )
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
    ) {
        let eye = Vec3::new(
            (pose.position[0] - self.origin[0] as f64) as f32,
            (pose.position[1] - self.origin[1] as f64) as f32,
            (pose.position[2] - self.origin[2] as f64) as f32,
        );
        let (sy, cy) = pose.yaw.to_radians().sin_cos();
        let (sp, cp) = pose.pitch.to_radians().sin_cos();
        self.view = Mat4::look_at_rh(eye, eye + Vec3::new(-sy * cp, -sp, cy * cp), Vec3::Y);
        self.eye = eye;
        self.fov = 70.0;
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
    fn planes(&self, config: &wgpu::SurfaceConfiguration) -> [Vec4; 5] {
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

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, config: &wgpu::SurfaceConfiguration) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bindings, &[]);
        if let Some(fixture) = &self.fixture {
            pass.set_vertex_buffer(0, fixture.buffer.slice(..));
            pass.draw(0..fixture.count, 0..1);
        }
        let planes = self.planes(config);
        let mut visible: Vec<(f32, &GpuColumn)> = self
            .columns
            .iter()
            .filter_map(|(&(x, z), column)| {
                let min = Vec3::new(
                    (i64::from(x) * 16 - self.origin[0]) as f32,
                    -self.origin[1] as f32,
                    (i64::from(z) * 16 - self.origin[2]) as f32,
                );
                let max = min + Vec3::new(16.0, 256.0, 16.0);
                Self::visible(&planes, min, max).then(|| {
                    let centre = (min + max) * 0.5;
                    let distance = Vec3::new(centre.x - self.eye.x, 0.0, centre.z - self.eye.z)
                        .length_squared();
                    (distance, column)
                })
            })
            .collect();
        visible.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, column) in &visible {
            if let Some(mesh) = &column.opaque {
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                pass.draw(0..mesh.count, 0..1);
            }
        }
        pass.set_bind_group(0, &self.entity_bindings, &[]);
        for (_, column) in &visible {
            if let Some(mesh) = &column.entity {
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                pass.draw(0..mesh.count, 0..1);
            }
        }
        if self.entity_count > 0 {
            pass.set_vertex_buffer(0, self.entities.slice(..));
            pass.draw(0..self.entity_count, 0..1);
        }
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.set_pipeline(&self.translucent_pipeline);
        for (_, column) in visible.iter().rev() {
            if let Some(mesh) = &column.translucent {
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                pass.draw(0..mesh.count, 0..1);
            }
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
