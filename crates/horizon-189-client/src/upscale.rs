//! Render scale: the 3D world (terrain, entities, labels, the hand) draws into
//! a smaller target that one full-screen triangle stretches over the drawable,
//! with the interface drawn on top at full resolution. Filtering is bilinear
//! in linear light (the target and the drawable are both sRGB).

/// Render scale percentages the setting offers.
pub const MIN_PERCENT: u8 = 50;
pub const MAX_PERCENT: u8 = 100;
pub const STEP_PERCENT: u8 = 5;

/// A saved or typed percentage, clamped to the range and rounded to a step.
pub fn percent(value: f64) -> u8 {
    let clamped = value.clamp(f64::from(MIN_PERCENT), f64::from(MAX_PERCENT));
    ((clamped / f64::from(STEP_PERCENT)).round() * f64::from(STEP_PERCENT)) as u8
}

/// The world target's size for a drawable at `percent`: never zero, and the
/// drawable itself at 100%.
pub fn world_size(drawable: (u32, u32), percent: u8) -> (u32, u32) {
    if percent >= MAX_PERCENT {
        return drawable;
    }
    let side = |pixels: u32| ((u64::from(pixels) * u64::from(percent)).div_ceil(100) as u32).max(1);
    (side(drawable.0), side(drawable.1))
}

const SHADER: &str = r#"
@group(0) @binding(0) var world: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// One triangle covering the screen: uv 0..1 over the visible part.
@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> Out {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Out;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

@fragment
fn fragment_main(in: Out) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSampleLevel(world, filtering, in.uv, 0.0).rgb, 1.0);
}
"#;

pub struct Upscale {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The scaled world target, its views and the bind group sampling it.
    target: Option<Target>,
}

pub struct Target {
    pub texture: wgpu::Texture,
    pub size: (u32, u32),
    bindings: wgpu::BindGroup,
}

impl Upscale {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Render scale"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Render scale bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render scale"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render scale"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Render scale"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
            target: None,
        }
    }

    /// The world target for `size`, made (or remade) on first use at that size;
    /// `None` drops it (100%).
    pub fn target(
        &mut self,
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        size: Option<(u32, u32)>,
    ) -> Option<&Target> {
        let Some(size) = size else {
            self.target = None;
            return None;
        };
        if self
            .target
            .as_ref()
            .is_none_or(|target| target.size != size)
        {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Scaled world"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &config.view_formats,
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Render scale"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.target = Some(Target {
                texture,
                size,
                bindings,
            });
        }
        self.target.as_ref()
    }

    /// Stretch the world target over `output`, replacing what was there.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let Some(target) = &self.target else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render scale"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    // Every pixel is written: nothing to load.
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &target.bindings, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_clamp_to_steps_and_sizes_never_vanish() {
        assert_eq!(percent(100.0), 100);
        assert_eq!(percent(140.0), 100);
        assert_eq!(percent(10.0), 50);
        assert_eq!(percent(72.0), 70);
        assert_eq!(percent(73.0), 75);
        assert_eq!(world_size((3440, 1440), 100), (3440, 1440));
        assert_eq!(world_size((3440, 1440), 50), (1720, 720));
        assert_eq!(world_size((2560, 1600), 75), (1920, 1200));
        assert_eq!(world_size((3, 1), 50), (2, 1));
    }
}
