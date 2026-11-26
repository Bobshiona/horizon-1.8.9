//! The overworld sky as `RenderGlobal.renderSky` draws it: the sky plane in
//! the sky colour under linear fog, the sunrise and sunset fan, the sun and
//! moon, the stars, and the dark plane at the horizon below. Colours and the
//! fog colour follow the time of day (`WorldProvider`, `World.getSkyColor`,
//! `EntityRenderer.updateFogColor`). Weather is not drawn yet.
use glam::{Mat4, Vec3};
use std::f32::consts::PI;

/// One sky vertex. `kind`: 0 the sky plane, 1 the plane below, 2 a star,
/// 3 drawn as given (fan, sun and moon).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkyVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
    pub layer: u32,
    pub kind: u32,
}

/// What one frame's sky looks like, in vanilla's (gamma) colour space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    /// `getCelestialAngle`, 0 noon, 0.5 midnight.
    pub celestial: f32,
    pub moon_phase: u32,
    pub sky: [f32; 3],
    pub fog: [f32; 3],
    /// `calcSunriseSunsetColors`: colour and alpha, near dawn and dusk only.
    pub sunrise: Option<[f32; 4]>,
    pub stars: f32,
    /// The lower plane's height relative to the eye (`63 - eye y`).
    pub lower: f32,
    /// Linear fog end for the sky: the render distance in blocks.
    pub far: f32,
}

/// `WorldProvider.calculateCelestialAngle`.
pub fn celestial_angle(time_of_day: i64, partial: f32) -> f32 {
    let ticks = time_of_day.rem_euclid(24_000) as f32;
    let mut f = (ticks + partial) / 24_000.0 - 0.25;
    if f < 0.0 {
        f += 1.0;
    }
    if f > 1.0 {
        f -= 1.0;
    }
    let linear = f;
    let eased = 1.0 - (((f64::from(f) * std::f64::consts::PI).cos() + 1.0) / 2.0) as f32;
    linear + (eased - linear) / 3.0
}

/// `java.awt.Color.HSBtoRGB`.
fn hsb(hue: f32, saturation: f32, brightness: f32) -> [f32; 3] {
    let channel = |v: f32| ((v * 255.0 + 0.5) as u32).min(255) as f32 / 255.0;
    if saturation == 0.0 {
        return [channel(brightness); 3];
    }
    let h = (hue - hue.floor()) * 6.0;
    let f = h - h.floor();
    let p = brightness * (1.0 - saturation);
    let q = brightness * (1.0 - saturation * f);
    let t = brightness * (1.0 - saturation * (1.0 - f));
    let [r, g, b] = match h as u32 {
        0 => [brightness, t, p],
        1 => [q, brightness, p],
        2 => [p, brightness, t],
        3 => [p, q, brightness],
        4 => [t, p, brightness],
        _ => [brightness, p, q],
    };
    [channel(r), channel(g), channel(b)]
}

/// `BiomeGenBase.getSkyColorByTemp`.
pub fn sky_by_temperature(temperature: f32) -> [f32; 3] {
    let t = (temperature / 3.0).clamp(-1.0, 1.0);
    hsb(0.622_222_24 - t * 0.05, 0.5 + t * 0.1, 1.0)
}

/// `cos(angle * 2 pi) * 2 + 0.5`, clamped: the daylight factor.
fn daylight(celestial: f32) -> f32 {
    ((celestial * PI * 2.0).cos() * 2.0 + 0.5).clamp(0.0, 1.0)
}

/// `WorldProvider.calcSunriseSunsetColors`.
pub fn sunrise(celestial: f32) -> Option<[f32; 4]> {
    let cos = (celestial * PI * 2.0).cos();
    if !(-0.4..=0.4).contains(&cos) {
        return None;
    }
    let f3 = cos / 0.4 * 0.5 + 0.5;
    let f4 = 1.0 - (1.0 - (f3 * PI).sin()) * 0.99;
    Some([f3 * 0.3 + 0.7, f3 * f3 * 0.7 + 0.2, 0.2, f4 * f4])
}

/// `World.getStarBrightness`.
pub fn star_brightness(celestial: f32) -> f32 {
    let f = (1.0 - ((celestial * PI * 2.0).cos() * 2.0 + 0.25)).clamp(0.0, 1.0);
    f * f * 0.5
}

/// One frame's sky. `look` is the camera's unit look vector, `eye_y` its
/// height, `render_distance` in chunks, `temperature` the biome's.
pub fn state(
    time_of_day: i64,
    partial: f32,
    temperature: f32,
    render_distance: u8,
    look: Vec3,
    eye_y: f64,
    flat: bool,
) -> State {
    let celestial = celestial_angle(time_of_day, partial);
    let light = daylight(celestial);
    let sky = sky_by_temperature(temperature).map(|c| c * light);
    // WorldProvider.getFogColor.
    let mut fog = [
        0.752_941_2 * (light * 0.94 + 0.06),
        0.847_058_83 * (light * 0.94 + 0.06),
        light * 0.91 + 0.09,
    ];
    // EntityRenderer.updateFogColor: toward the sky colour at short render
    // distances, then toward the sunrise colour when looking at it.
    let reach = 0.25 + 0.75 * f32::from(render_distance) / 32.0;
    let toward_sky = 1.0 - reach.powf(0.25);
    for i in 0..3 {
        fog[i] += (sky[i] - fog[i]) * toward_sky;
    }
    let sunrise = sunrise(celestial);
    if render_distance >= 4
        && let Some(colour) = sunrise
    {
        let side = if (celestial * PI * 2.0).sin() > 0.0 {
            -1.0
        } else {
            1.0
        };
        let facing = (look.x * side).max(0.0) * colour[3];
        for i in 0..3 {
            fog[i] += (colour[i] - fog[i]) * facing;
        }
    }
    // Below y 32 the fog darkens toward the void (`getVoidFogYFactor`; 1 on
    // superflat, so never there).
    let void = eye_y * if flat { 1.0 } else { 0.031_25 };
    if void < 1.0 {
        let scale = (void.max(0.0) * void.max(0.0)) as f32;
        fog = fog.map(|c| c * scale);
    }
    State {
        celestial,
        moon_phase: (time_of_day / 24_000).rem_euclid(8) as u32,
        sky,
        fog,
        sunrise,
        stars: star_brightness(celestial),
        // `WorldType.getHorizon`: 63, or 0 on superflat.
        lower: ((if flat { 0.0 } else { 63.0 }) - eye_y) as f32,
        far: f32::from(render_distance) * 16.0,
    }
}

/// `java.util.Random`, for vanilla's star field.
struct JavaRandom(u64);

impl JavaRandom {
    fn new(seed: u64) -> Self {
        Self((seed ^ 0x5_DEEC_E66D) & ((1 << 48) - 1))
    }
    fn next(&mut self, bits: u32) -> u32 {
        self.0 = (self.0.wrapping_mul(0x5_DEEC_E66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.0 >> (48 - bits)) as u32
    }
    fn float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }
    fn double(&mut self) -> f64 {
        let high = u64::from(self.next(26)) << 27;
        (high + u64::from(self.next(27))) as f64 / (1u64 << 53) as f64
    }
}

fn quad(
    out: &mut Vec<SkyVertex>,
    corners: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    layer: u32,
    colour: [f32; 4],
    kind: u32,
) {
    for index in [0, 1, 2, 0, 2, 3] {
        out.push(SkyVertex {
            position: corners[index],
            color: colour,
            uv: uv[index],
            layer,
            kind,
        });
    }
}

/// `RenderGlobal.renderSky(WorldRenderer, float, boolean)`: 64-block quads
/// from -384 to 384 at height `y`.
fn plane(out: &mut Vec<SkyVertex>, y: f32, kind: u32) {
    for k in (-384..=384).step_by(64) {
        for l in (-384..=384).step_by(64) {
            let (x0, x1) = (k as f32, (k + 64) as f32);
            let (z0, z1) = (l as f32, (l + 64) as f32);
            quad(
                out,
                [[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]],
                [[0.0; 2]; 4],
                0,
                [1.0; 4],
                kind,
            );
        }
    }
}

/// `RenderGlobal.renderStars`: 1500 tries from seed 10842, on a sphere of
/// radius 100.
fn stars(out: &mut Vec<SkyVertex>) {
    let mut random = JavaRandom::new(10842);
    for _ in 0..1500 {
        let mut d0 = f64::from(random.float() * 2.0 - 1.0);
        let mut d1 = f64::from(random.float() * 2.0 - 1.0);
        let mut d2 = f64::from(random.float() * 2.0 - 1.0);
        let d3 = f64::from(0.15 + random.float() * 0.1);
        let mut d4 = d0 * d0 + d1 * d1 + d2 * d2;
        if !(d4 < 1.0 && d4 > 0.01) {
            continue;
        }
        d4 = 1.0 / d4.sqrt();
        d0 *= d4;
        d1 *= d4;
        d2 *= d4;
        let (d5, d6, d7) = (d0 * 100.0, d1 * 100.0, d2 * 100.0);
        let d8 = d0.atan2(d2);
        let (d9, d10) = (d8.sin(), d8.cos());
        let d11 = (d0 * d0 + d2 * d2).sqrt().atan2(d1);
        let (d12, d13) = (d11.sin(), d11.cos());
        let d14 = random.double() * std::f64::consts::PI * 2.0;
        let (d15, d16) = (d14.sin(), d14.cos());
        let corners: [[f32; 3]; 4] = std::array::from_fn(|j| {
            let j = j as i32;
            let d18 = f64::from((j & 2) - 1) * d3;
            let d19 = f64::from(((j + 1) & 2) - 1) * d3;
            let d21 = d18 * d16 - d19 * d15;
            let d22 = d19 * d16 + d18 * d15;
            let d23 = d21 * d12;
            let d24 = -d21 * d13;
            let d25 = d24 * d9 - d22 * d10;
            let d26 = d22 * d9 + d24 * d10;
            [(d5 + d25) as f32, (d6 + d23) as f32, (d7 + d26) as f32]
        });
        quad(out, corners, [[0.0; 2]; 4], 0, [1.0; 4], 2);
    }
}

/// The geometry that never changes: the sky plane, the plane below and the
/// stars, with each part's range.
pub struct Fixed {
    pub vertices: Vec<SkyVertex>,
    pub upper: std::ops::Range<u32>,
    pub lower: std::ops::Range<u32>,
    pub stars: std::ops::Range<u32>,
}

pub fn fixed() -> Fixed {
    let mut vertices = Vec::new();
    plane(&mut vertices, 16.0, 0);
    let upper = 0..vertices.len() as u32;
    // `generateSky2` mirrors the quads; winding does not matter here.
    plane(&mut vertices, -16.0, 1);
    let lower = upper.end..vertices.len() as u32;
    stars(&mut vertices);
    let stars = lower.end..vertices.len() as u32;
    Fixed {
        vertices,
        upper,
        lower,
        stars,
    }
}

/// `rotate(-90, Y)` then `rotate(angle * 360, X)`: the sun, moon and stars.
pub fn celestial_rotation(celestial: f32) -> Mat4 {
    Mat4::from_rotation_y((-90.0f32).to_radians()) * Mat4::from_rotation_x(celestial * 2.0 * PI)
}

/// This frame's sunrise fan (blended) and sun and moon (added), already
/// placed. `sun` and `moon` are their texture layers.
pub fn dynamic(state: &State, sun: u32, moon: u32) -> (Vec<SkyVertex>, Vec<SkyVertex>) {
    let mut fan = Vec::new();
    if let Some([r, g, b, a]) = state.sunrise {
        let flip = if (state.celestial * PI * 2.0).sin() < 0.0 {
            PI
        } else {
            0.0
        };
        let matrix = Mat4::from_rotation_x(PI / 2.0)
            * Mat4::from_rotation_z(flip)
            * Mat4::from_rotation_z(PI / 2.0);
        let at = |p: [f32; 3]| matrix.transform_point3(Vec3::from(p)).to_array();
        let centre = SkyVertex {
            position: at([0.0, 100.0, 0.0]),
            color: [r, g, b, a],
            uv: [0.0; 2],
            layer: 0,
            kind: 3,
        };
        let rim = |j: u32| {
            let angle = j as f32 * PI * 2.0 / 16.0;
            let (sin, cos) = angle.sin_cos();
            SkyVertex {
                position: at([sin * 120.0, cos * 120.0, -cos * 40.0 * a]),
                color: [r, g, b, 0.0],
                ..centre
            }
        };
        for j in 0..16 {
            fan.extend([centre, rim(j), rim(j + 1)]);
        }
    }
    let mut lights = Vec::new();
    let matrix = celestial_rotation(state.celestial);
    let at = |p: [f32; 3]| matrix.transform_point3(Vec3::from(p)).to_array();
    let s = 30.0;
    // The sun is 32 pixels of the 128-pixel layer, the moon sheet 128x64.
    let cell = 32.0 / crate::models::LAYER_SIZE as f32;
    quad(
        &mut lights,
        [
            [-s, 100.0, -s],
            [s, 100.0, -s],
            [s, 100.0, s],
            [-s, 100.0, s],
        ]
        .map(at),
        [[0.0, 0.0], [cell, 0.0], [cell, cell], [0.0, cell]],
        sun,
        [1.0; 4],
        3,
    );
    let m = 20.0;
    let phase = state.moon_phase;
    let (column, row) = ((phase % 4) as f32, (phase / 4 % 2) as f32);
    let scale = 128.0 / crate::models::LAYER_SIZE as f32;
    let (u0, v0) = (column / 4.0 * scale, row / 4.0 * scale);
    let (u1, v1) = ((column + 1.0) / 4.0 * scale, (row + 1.0) / 4.0 * scale);
    quad(
        &mut lights,
        [
            [-m, -100.0, m],
            [m, -100.0, m],
            [m, -100.0, -m],
            [-m, -100.0, -m],
        ]
        .map(at),
        [[u1, v1], [u0, v1], [u0, v0], [u1, v0]],
        moon,
        [1.0; 4],
        3,
    );
    (fan, lights)
}

/// Gamma colour to linear light, as the sRGB targets store it.
pub fn linear(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| {
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn celestial_angle_and_day_colours_follow_vanilla() {
        // Noon is 6000: angle 0; midnight 18000: 0.5; dawn and dusk between.
        assert!(celestial_angle(6000, 0.0).abs() < 1e-6);
        assert!((celestial_angle(18000, 0.0) - 0.5).abs() < 1e-6);
        assert!((celestial_angle(0, 0.0) - 0.7845).abs() < 1e-3);
        // Plains (0.8) at noon: Color.getHSBColor(0.6089, 0.5267, 1).
        let noon = state(6000, 0.0, 0.8, 12, Vec3::Z, 70.0, false);
        assert_eq!(noon.sky, sky_by_temperature(0.8));
        assert_eq!(
            sky_by_temperature(0.8).map(|c| (c * 255.0).round() as u32),
            [121, 167, 255]
        );
        assert!(noon.sunrise.is_none());
        assert_eq!(noon.stars, 0.0);
        let midnight = state(18000, 0.0, 0.8, 12, Vec3::Z, 70.0, false);
        assert_eq!(midnight.sky, [0.0; 3]);
        assert!((midnight.stars - 0.5).abs() < 1e-6);
        assert!(midnight.fog[2] < 0.1);
        // Dawn: the sunrise colours, and fog toward them when looking east.
        let dawn = celestial_angle(23500, 0.0);
        assert!(sunrise(dawn).is_some());
        let east = state(23500, 0.0, 0.8, 12, Vec3::X, 70.0, false);
        let west = state(23500, 0.0, 0.8, 12, -Vec3::X, 70.0, false);
        assert!(east.fog[0] > west.fog[0]);
        // Moon phases advance a day at a time.
        assert_eq!(
            state(24000 * 9 + 6000, 0.0, 0.8, 12, Vec3::Z, 70.0, false).moon_phase,
            1
        );
        // Low down the fog darkens.
        assert!(state(6000, 0.0, 0.8, 12, Vec3::Z, 16.0, false).fog[2] < noon.fog[2]);
        assert_eq!(
            state(6000, 0.0, 0.8, 12, Vec3::Z, 5.6, true).fog,
            noon.fog,
            "superflat"
        );
    }

    #[test]
    fn the_star_field_matches_java_random() {
        // java.util.Random(10842): the first float.
        let mut random = JavaRandom::new(10842);
        let first = random.float();
        assert!((0.0..1.0).contains(&first));
        assert_eq!(JavaRandom::new(0).next(32), 0xbb20_b460);
        let fixed = fixed();
        assert_eq!(fixed.upper.len(), 13 * 13 * 6);
        assert_eq!(fixed.lower.len(), 13 * 13 * 6);
        let stars = fixed.stars.len() / 6;
        // About pi/6 of the cube's points fall inside the sphere.
        assert_eq!(stars, 780);
        for vertex in &fixed.vertices[fixed.stars.start as usize..] {
            let radius = Vec3::from(vertex.position).length();
            assert!((99.0..101.0).contains(&radius));
        }
    }
}

/// The GPU side: fixed geometry, this frame's fan and lights, a uniform,
/// and two pipelines (blended and added).
pub struct Pass {
    blended: wgpu::RenderPipeline,
    added: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bindings: wgpu::BindGroup,
    fixed: wgpu::Buffer,
    ranges: [std::ops::Range<u32>; 3],
    dynamic: wgpu::Buffer,
    fan: u32,
    lights: u32,
    pub state: Option<State>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    matrix: [f32; 16],
    celestial: [f32; 16],
    sky: [f32; 4],
    fog: [f32; 4],
    lower: [f32; 4],
    stars: [f32; 4],
}

/// Room for the fan (48 vertices) and the sun and moon (12).
const DYNAMIC: u64 = 64;

impl Pass {
    pub fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        texture: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        format: wgpu::TextureFormat,
    ) -> Self {
        use wgpu::util::DeviceExt;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sky shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sky uniform"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Sky resources"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sky layout"),
            bind_group_layouts: &[Some(layout)],
            ..Default::default()
        });
        let make = |label: &str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("sky_vertex"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<SkyVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0=>Float32x3, 1=>Float32x4, 2=>Float32x2, 3=>Uint32, 4=>Uint32],
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
                    entry_point: Some("sky_fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::COLOR,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let blended = make("Sky (blended)", wgpu::BlendState::ALPHA_BLENDING);
        // glBlendFunc(GL_SRC_ALPHA, GL_ONE) for the sun, moon and stars.
        let added = make(
            "Sky (added)",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            },
        );
        let geometry = fixed();
        let fixed = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Sky geometry"),
            contents: bytemuck::cast_slice(&geometry.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let dynamic = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sky fan, sun and moon"),
            size: DYNAMIC * std::mem::size_of::<SkyVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            blended,
            added,
            uniform,
            bindings,
            fixed,
            ranges: [geometry.upper, geometry.lower, geometry.stars],
            dynamic,
            fan: 0,
            lights: 0,
            state: None,
        }
    }

    /// This frame's sky; `rotation` is the projection times the view's
    /// rotation.
    pub fn update(&mut self, queue: &wgpu::Queue, state: State, rotation: Mat4) {
        let (fan, lights) = dynamic(
            &state,
            crate::models::Texture::Sun as u32,
            crate::models::Texture::Moon as u32,
        );
        let mut vertices = fan;
        self.fan = vertices.len() as u32;
        vertices.extend(lights);
        self.lights = vertices.len() as u32 - self.fan;
        vertices.truncate(DYNAMIC as usize);
        queue.write_buffer(&self.dynamic, 0, bytemuck::cast_slice(&vertices));
        let [r, g, b] = linear(state.sky);
        let [fr, fg, fb] = linear(state.fog);
        // The plane below: sky * 0.2 + 0.04, blue * 0.6 + 0.1 (isSkyColored).
        let [lr, lg, lb] = linear([
            state.sky[0] * 0.2 + 0.04,
            state.sky[1] * 0.2 + 0.04,
            state.sky[2] * 0.6 + 0.1,
        ]);
        let uniform = Uniform {
            matrix: rotation.to_cols_array(),
            celestial: celestial_rotation(state.celestial).to_cols_array(),
            sky: [r, g, b, state.far],
            fog: [fr, fg, fb, state.lower + 16.0],
            lower: [lr, lg, lb, 1.0],
            stars: [state.stars, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        self.state = Some(state);
    }

    /// Linear fog colour, if a sky is set.
    pub fn fog(&self) -> Option<[f32; 3]> {
        self.state.map(|state| linear(state.fog))
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) -> u32 {
        let Some(state) = self.state else {
            return 0;
        };
        let [upper, lower, stars] = &self.ranges;
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.set_pipeline(&self.blended);
        pass.set_vertex_buffer(0, self.fixed.slice(..));
        pass.draw(upper.clone(), 0..1);
        let mut draws = 1;
        pass.set_vertex_buffer(0, self.dynamic.slice(..));
        if self.fan > 0 {
            pass.draw(0..self.fan, 0..1);
            draws += 1;
        }
        pass.set_pipeline(&self.added);
        pass.draw(self.fan..self.fan + self.lights, 0..1);
        draws += 1;
        pass.set_vertex_buffer(0, self.fixed.slice(..));
        if state.stars > 0.0 {
            pass.draw(stars.clone(), 0..1);
            draws += 1;
        }
        // Last, so it hides the sun and moon below the horizon.
        pass.set_pipeline(&self.blended);
        pass.draw(lower.clone(), 0..1);
        draws + 1
    }
}
