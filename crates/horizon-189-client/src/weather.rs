//! Rain and snow as `EntityRenderer.renderRainSnow` draws them: one
//! textured quad per column around the player, facing them, from the
//! precipitation height up, scrolling down (`environment/rain.png`) or
//! drifting (`environment/snow.png`), fading with distance and strength.
use horizon_world_189::World;

/// One weather vertex, relative to the camera.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WeatherVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    /// 0 rain, 1 snow.
    pub layer: u32,
    pub color: [f32; 4],
}

/// Biomes without rain (desert, savanna, mesa, the Nether, the End).
fn dry(biome: u8) -> bool {
    matches!(biome & 127, 2 | 8 | 9 | 17 | 35..=39)
}

/// `getFloatTemperature`, without the temperature noise above y 64.
fn temperature(biome: u8, y: i32) -> f32 {
    let base = crate::terrain::temperature(biome);
    if y > 64 {
        base - (y - 64) as f32 * 0.05 / 30.0
    } else {
        base
    }
}

/// `Material.blocksMovement() || isLiquid()`: what stops rain
/// (`Chunk.getPrecipitationHeight`).
fn stops_rain(block: u16) -> bool {
    !matches!(
        block,
        0 | 6
            | 27
            | 28
            | 30..=32
            | 37..=40
            | 50
            | 51
            | 55
            | 59
            | 66
            | 69
            | 75..=78
            | 83
            | 90
            | 93
            | 94
            | 104..=106
            | 115
            | 131
            | 132
            | 141..=143
            | 149
            | 150
            | 157
            | 171
            | 175
    )
}

/// The first height above every block that stops rain in a column; empty
/// sections are skipped whole.
fn precipitation_height(column: &horizon_world_189::ChunkColumn, x: u8, z: u8) -> i32 {
    for index in (0..16).rev() {
        let Some(section) = column.section(index) else {
            continue;
        };
        for y in (0..16u8).rev() {
            if section
                .state(x, y, z)
                .is_some_and(|state| stops_rain(state.block_id()))
            {
                return index as i32 * 16 + i32::from(y) + 1;
            }
        }
    }
    0
}

/// `java.util.Random` with `nextGaussian`, for the snow drift.
struct JavaRandom {
    seed: u64,
    spare: Option<f64>,
}

impl JavaRandom {
    fn new(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ 0x5_DEEC_E66D) & ((1 << 48) - 1),
            spare: None,
        }
    }
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(0x5_DEEC_E66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.seed >> (48 - bits)) as i32
    }
    fn double(&mut self) -> f64 {
        let high = (self.next(26) as u32 as u64) << 27;
        (high + self.next(27) as u32 as u64) as f64 / (1u64 << 53) as f64
    }
    fn gaussian(&mut self) -> f64 {
        if let Some(spare) = self.spare.take() {
            return spare;
        }
        loop {
            let v1 = 2.0 * self.double() - 1.0;
            let v2 = 2.0 * self.double() - 1.0;
            let s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                let multiplier = (-2.0 * s.ln() / s).sqrt();
                self.spare = Some(v2 * multiplier);
                return v1 * multiplier;
            }
        }
    }
}

/// This frame's rain and snow. `feet` is the viewer's interpolated feet
/// position, `camera` the eye the vertices are made relative to, `ticks`
/// the client tick count, `strength` the rain strength, `fancy` the
/// Graphics setting (radius 10, else 5).
#[allow(clippy::too_many_arguments)]
pub fn vertices(
    world: &World,
    feet: [f64; 3],
    camera: [f64; 3],
    ticks: u64,
    partial: f32,
    strength: f32,
    fancy: bool,
    light: impl Fn([i32; 3]) -> u8,
) -> Vec<WeatherVertex> {
    let mut out = Vec::new();
    if strength <= 0.0 {
        return out;
    }
    let radius: i32 = if fancy { 10 } else { 5 };
    let [i, j, k] = feet.map(|v| v.floor() as i32);
    let l = feet[1].floor() as i32;
    let time = ticks as f64 + f64::from(partial);
    let at = |x: f64, y: f64, z: f64| {
        [
            (x - camera[0]) as f32,
            (y - camera[1]) as f32,
            (z - camera[2]) as f32,
        ]
    };
    for z in k - radius..=k + radius {
        for x in i - radius..=i + radius {
            let (dx, dz) = (x - i, z - k);
            // rainXCoords / rainYCoords: half a block across, facing the
            // player; the centre column has none (0 / 0).
            let length = ((dx * dx + dz * dz) as f32).sqrt();
            if length == 0.0 {
                continue;
            }
            let (d3, d4) = (
                f64::from(-dz as f32 / length) * 0.5,
                f64::from(dx as f32 / length) * 0.5,
            );
            let Some(column) = world.column(x.div_euclid(16), z.div_euclid(16)) else {
                continue;
            };
            let biome = column
                .biome(x.rem_euclid(16) as u8, z.rem_euclid(16) as u8)
                .unwrap_or(1);
            if dry(biome) {
                continue;
            }
            let top = precipitation_height(column, x.rem_euclid(16) as u8, z.rem_euclid(16) as u8);
            let bottom = (j - radius).max(top);
            let upper = (j + radius).max(top);
            if bottom == upper {
                continue;
            }
            let lit = top.max(l);
            let hash = (x.wrapping_mul(x).wrapping_mul(3121))
                .wrapping_add(x.wrapping_mul(45_238_971))
                ^ (z.wrapping_mul(z).wrapping_mul(418_711)).wrapping_add(z.wrapping_mul(13761));
            let mut random = JavaRandom::new(i64::from(hash));
            let reach = (((f64::from(x) + 0.5 - feet[0]).powi(2)
                + (f64::from(z) + 0.5 - feet[2]).powi(2))
            .sqrt() as f32)
                / radius as f32;
            let corners = |y0: f64, y1: f64| {
                [
                    at(f64::from(x) - d3 + 0.5, y0, f64::from(z) - d4 + 0.5),
                    at(f64::from(x) + d3 + 0.5, y0, f64::from(z) + d4 + 0.5),
                    at(f64::from(x) + d3 + 0.5, y1, f64::from(z) + d4 + 0.5),
                    at(f64::from(x) - d3 + 0.5, y1, f64::from(z) - d4 + 0.5),
                ]
            };
            let (y0, y1) = (f64::from(bottom), f64::from(upper));
            let (layer, uv, alpha, level) = if temperature(biome, bottom) >= 0.15 {
                // (rendererUpdateCount + the column's hash terms) & 31, in
                // Java int arithmetic.
                let scroll = ((ticks as i32)
                    .wrapping_add(x.wrapping_mul(x).wrapping_mul(3121))
                    .wrapping_add(x.wrapping_mul(45_238_971))
                    .wrapping_add(z.wrapping_mul(z).wrapping_mul(418_711))
                    .wrapping_add(z.wrapping_mul(13761))
                    & 31) as f64;
                let d5 = (scroll + f64::from(partial)) / 32.0 * (3.0 + random.double());
                let alpha = ((1.0 - reach * reach) * 0.5 + 0.5) * strength;
                (
                    0u32,
                    [
                        [0.0, y0 * 0.25 + d5],
                        [1.0, y0 * 0.25 + d5],
                        [1.0, y1 * 0.25 + d5],
                        [0.0, y1 * 0.25 + d5],
                    ],
                    alpha,
                    light([x, lit, z]),
                )
            } else {
                let d8 = (((ticks & 511) as f64) + f64::from(partial)) / 512.0;
                let d9 = random.double() + time * 0.01 * random.gaussian();
                let d10 = random.double() + time * random.gaussian() * 0.001;
                let alpha = ((1.0 - reach * reach) * 0.3 + 0.5) * strength;
                // A quarter of the light, three quarters full brightness.
                let level = (u32::from(light([x, lit, z])) * 3 + 15) / 4;
                (
                    1u32,
                    [
                        [d9, y0 * 0.25 + d8 + d10],
                        [1.0 + d9, y0 * 0.25 + d8 + d10],
                        [1.0 + d9, y1 * 0.25 + d8 + d10],
                        [d9, y1 * 0.25 + d8 + d10],
                    ],
                    alpha,
                    level as u8,
                )
            };
            let brightness = crate::terrain::brightness_of(level);
            let points = corners(y0, y1);
            for index in [0, 1, 2, 0, 2, 3] {
                out.push(WeatherVertex {
                    position: points[index],
                    uv: [uv[index][0] as f32, uv[index][1] as f32],
                    layer,
                    color: [brightness, brightness, brightness, alpha],
                });
            }
        }
    }
    out
}

/// The GPU side: the rain and snow textures (repeating), a uniform and a
/// blended pipeline; this frame's vertices are written each frame it rains.
pub struct Pass {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bindings: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    capacity: u64,
    count: u32,
}

impl Pass {
    /// `None` without the rain and snow images.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        images: Option<&[Vec<u8>; 2]>,
        format: wgpu::TextureFormat,
    ) -> Option<Self> {
        let images = images?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Rain and snow"),
            size: wgpu::Extent3d {
                width: 64,
                height: 256,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, image) in images.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                image,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(64 * 4),
                    rows_per_image: Some(256),
                },
                wgpu::Extent3d {
                    width: 64,
                    height: 256,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Rain and snow sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Weather resources"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Weather uniform"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Weather resources"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Weather shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("weather.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Weather layout"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Rain and snow"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("weather_vertex"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<WeatherVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3, 1=>Float32x2, 2=>Uint32, 3=>Float32x4],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("weather_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let capacity = 4096;
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rain and snow quads"),
            size: capacity * std::mem::size_of::<WeatherVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Self {
            pipeline,
            uniform,
            bindings,
            vertices,
            capacity,
            count: 0,
        })
    }

    /// This frame's quads; `matrix` is the projection times the view's
    /// rotation (the vertices are camera-relative).
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[WeatherVertex],
        matrix: glam::Mat4,
    ) {
        self.count = vertices.len() as u32;
        if vertices.is_empty() {
            return;
        }
        if vertices.len() as u64 > self.capacity {
            self.capacity = (vertices.len() as u64).next_power_of_two();
            self.vertices = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Rain and snow quads"),
                size: self.capacity * std::mem::size_of::<WeatherVertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(vertices));
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::cast_slice(&matrix.to_cols_array()),
        );
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) -> u32 {
        if self.count == 0 {
            return 0;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..self.count, 0..1);
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_gaussian_matches_the_jdk() {
        // new Random(42).nextGaussian() == 1.1419053154730547
        let mut random = JavaRandom::new(42);
        assert!((random.gaussian() - 1.141_905_315_473_054_7).abs() < 1e-12);
        // new Random(42).nextDouble() == 0.7275636800328681
        assert!((JavaRandom::new(42).double() - 0.727_563_680_032_868_1).abs() < 1e-15);
    }

    #[test]
    fn rain_stops_at_solid_blocks_and_liquids_only() {
        assert!(stops_rain(1) && stops_rain(9) && stops_rain(18) && stops_rain(63));
        assert!(!stops_rain(0) && !stops_rain(31) && !stops_rain(50) && !stops_rain(78));
        assert!(dry(2) && dry(130) && dry(37) && !dry(1) && !dry(12));
        assert!(temperature(12, 64) < 0.15, "ice plains snow");
        assert!(temperature(1, 64) >= 0.15, "plains rain");
        assert!(
            temperature(3, 64) >= 0.15 && temperature(3, 120) < 0.15,
            "hills snow up high"
        );
    }
}
