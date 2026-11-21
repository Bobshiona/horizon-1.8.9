//! Liquid-glass panels drawn over the finished frame: the frame behind them
//! is copied, blurred with a dual-Kawase chain at half, quarter and eighth
//! resolution, then each panel samples the blur through a rounded-rect lens
//! with a tint, a specular rim and a soft shadow (see `glass.wgsl`).
//!
//! Panels come in two layers: layer 0 sits under the whole interface (HUD
//! cards), layer 1 under the interface drawn after `Canvas::overlay` (menus),
//! so menu glass frosts the HUD as well as the world.
use std::error::Error;

/// One glass rectangle in GUI pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    pub rect: [f32; 4],
    pub radius: f32,
    /// Rim and sheen strength, 0 to 1.
    pub highlight: f32,
    /// Drop shadow opacity, 0 to 1.
    pub shadow: f32,
    /// sRGB tint; alpha is how much of it replaces the frost.
    pub tint: [f32; 4],
    /// The whole panel's opacity, shadow included, for fades.
    pub opacity: f32,
}

impl Panel {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self {
            rect: [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)],
            radius: 6.0,
            highlight: 0.55,
            shadow: 0.28,
            tint: [0.08, 0.09, 0.12, 0.42],
            opacity: 1.0,
        }
    }
}

/// Shadow margin around each panel, in GUI pixels.
const MARGIN: f32 = 10.0;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct PanelVertex {
    position: [f32; 2],
    center: [f32; 2],
    half: [f32; 2],
    params: [f32; 4],
    tint: [f32; 4],
}

fn vertices(panels: &[Panel], out: &mut Vec<PanelVertex>) {
    for panel in panels {
        let [x0, y0, x1, y1] = panel.rect;
        if !(x1 > x0 && y1 > y0) || !panel.rect.iter().all(|v| v.is_finite()) {
            continue;
        }
        let center = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let half = [(x1 - x0) / 2.0, (y1 - y0) / 2.0];
        let params = [
            panel.radius.max(0.0),
            panel.highlight,
            panel.shadow,
            panel.opacity.clamp(0.0, 1.0),
        ];
        let (a, b) = ([x0 - MARGIN, y0 - MARGIN], [x1 + MARGIN, y1 + MARGIN]);
        let corners = [a, [b[0], a[1]], b, [a[0], b[1]]];
        for i in [0, 1, 2, 0, 2, 3] {
            out.push(PanelVertex {
                position: corners[i],
                center,
                half,
                params,
                tint: panel.tint,
            });
        }
    }
}

struct Targets {
    size: (u32, u32),
    backdrop: wgpu::Texture,
    /// Half, quarter and eighth resolution.
    chain: [wgpu::Texture; 3],
    /// Down 0..3 then up 3..5, then the panel group last.
    groups: Vec<wgpu::BindGroup>,
    /// The panel pass's screen uniform.
    screen: wgpu::Buffer,
}

pub struct GlassPass {
    layout: wgpu::BindGroupLayout,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    panel: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    frame_format: wgpu::TextureFormat,
    targets: Option<Targets>,
    vertices: wgpu::Buffer,
    capacity: u64,
    ranges: [std::ops::Range<u32>; 2],
    staging: Vec<PanelVertex>,
}

impl GlassPass {
    pub fn new(device: &wgpu::Device, frame_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Glass"),
            source: wgpu::ShaderSource::Wgsl(include_str!("glass.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Glass layout"),
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
                        view_dimension: wgpu::TextureViewDimension::D2,
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
            label: Some("Glass pipeline"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let full = |entry: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Glass blur"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let down = full("fragment_down", FORMAT);
        let up = full("fragment_up", FORMAT);
        let panel = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Glass panels"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_panel"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<PanelVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x2, 1=>Float32x2, 2=>Float32x2, 3=>Float32x4, 4=>Float32x4],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_panel"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: frame_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Glass sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let capacity = 64 * 1024;
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Glass vertices"),
            size: capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            layout,
            down,
            up,
            panel,
            sampler,
            frame_format,
            targets: None,
            vertices,
            capacity,
            ranges: [0..0, 0..0],
            staging: Vec::new(),
        }
    }

    fn uniform(device: &wgpu::Device, value: [f32; 4]) -> wgpu::Buffer {
        use wgpu::util::DeviceExt;
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Glass parameters"),
            contents: bytemuck::cast_slice(&value),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })
    }

    fn targets(&mut self, device: &wgpu::Device, size: (u32, u32)) -> &Targets {
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            let texture = |label, width: u32, height: u32, format, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: width.max(1),
                        height: height.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let backdrop = texture(
                "Glass backdrop",
                size.0,
                size.1,
                self.frame_format,
                wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let usage =
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            let chain =
                [2, 4, 8].map(|d| texture("Glass blur", size.0 / d, size.1 / d, FORMAT, usage));
            let group = |source: &wgpu::Texture, uniform: &wgpu::Buffer| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Glass pass"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(
                                &source.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                })
            };
            let texel = |t: &wgpu::Texture| {
                Self::uniform(
                    device,
                    [1.0 / t.width() as f32, 1.0 / t.height() as f32, 0.0, 0.0],
                )
            };
            // Down: backdrop -> half -> quarter -> eighth; up: eighth ->
            // quarter -> half. Each samples its source texel size.
            let sources = [&backdrop, &chain[0], &chain[1], &chain[2], &chain[1]];
            let mut groups: Vec<_> = sources.iter().map(|s| group(s, &texel(s))).collect();
            let screen = Self::uniform(device, [size.0 as f32, size.1 as f32, 1.0, 0.0]);
            groups.push(group(&chain[0], &screen));
            self.targets = Some(Targets {
                size,
                backdrop,
                chain,
                groups,
                screen,
            });
        }
        self.targets.as_ref().expect("targets were just created")
    }

    /// Replace this frame's panels for both layers.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        factor: f64,
        size: (u32, u32),
        layers: [&[Panel]; 2],
    ) -> Result<(), Box<dyn Error>> {
        let mut staging = std::mem::take(&mut self.staging);
        staging.clear();
        vertices(layers[0], &mut staging);
        let split = u32::try_from(staging.len())?;
        vertices(layers[1], &mut staging);
        self.ranges = [0..split, split..u32::try_from(staging.len())?];
        if !staging.is_empty() {
            let bytes = std::mem::size_of_val(staging.as_slice()) as u64;
            if bytes > self.capacity {
                self.capacity = bytes.next_power_of_two();
                self.vertices = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Glass vertices"),
                    size: self.capacity,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&staging));
            let screen = [size.0 as f32, size.1 as f32, factor as f32, 0.0];
            let targets = self.targets(device, size);
            queue.write_buffer(&targets.screen, 0, bytemuck::cast_slice(&screen));
        }
        self.staging = staging;
        Ok(())
    }

    pub fn has_layer(&self, layer: usize) -> bool {
        !self.ranges[layer].is_empty()
    }

    /// Copy the frame behind `layer`, blur it and draw that layer's panels
    /// into `target` (an sRGB view of `frame`).
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        layer: usize,
        frame: &wgpu::Texture,
        copyable: bool,
        target: &wgpu::TextureView,
    ) {
        let (Some(targets), true) = (&self.targets, self.has_layer(layer)) else {
            return;
        };
        if (frame.width(), frame.height()) != targets.size {
            return;
        }
        if copyable {
            encoder.copy_texture_to_texture(
                frame.as_image_copy(),
                targets.backdrop.as_image_copy(),
                frame.size(),
            );
        }
        let passes = [
            (&self.down, &targets.chain[0], 0),
            (&self.down, &targets.chain[1], 1),
            (&self.down, &targets.chain[2], 2),
            (&self.up, &targets.chain[1], 3),
            (&self.up, &targets.chain[0], 4),
        ];
        for (pipeline, output, group) in passes {
            let view = output.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Glass blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &targets.groups[group], &[]);
            pass.draw(0..3, 0..1);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Glass panels"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.panel);
        pass.set_bind_group(0, &targets.groups[5], &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(self.ranges[layer].clone(), 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panels_expand_for_their_shadow_and_skip_degenerate_rects() {
        let mut out = Vec::new();
        let mut empty = Panel::new(5.0, 5.0, 5.0, 9.0);
        vertices(&[Panel::new(10.0, 20.0, 50.0, 40.0), empty], &mut out);
        assert_eq!(out.len(), 6);
        assert_eq!(out[0].position, [10.0 - MARGIN, 20.0 - MARGIN]);
        assert_eq!(out[2].position, [50.0 + MARGIN, 40.0 + MARGIN]);
        assert_eq!(out[0].center, [30.0, 30.0]);
        assert_eq!(out[0].half, [20.0, 10.0]);
        empty.rect[0] = f32::NAN;
        out.clear();
        vertices(&[empty], &mut out);
        assert!(out.is_empty());
    }
}
