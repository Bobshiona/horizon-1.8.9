//! Portable terrain command descriptions; native APIs remain in the macOS leaf.
#![deny(unsafe_code)]
use std::ops::Range;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Shared triangle indices for four-vertex quads. The native encoder owns this
/// immutable data so it can validate the complete hardware-fetch address range.
pub(crate) fn quad_indices(quads: u32) -> Vec<u32> {
    (0..quads)
        .flat_map(|quad| {
            let base = quad * 4;
            [base, base + 1, base + 2, base, base + 2, base + 3]
        })
        .collect()
}

pub struct Draw {
    pub vertices: wgpu::Buffer,
    pub low: [f32; 3],
    pub high: [f32; 3],
    /// Triangle range in canonical quad indices; both ends must be multiples of six.
    pub indices: Range<u32>,
    pub base_vertex: i32,
    pub instance: u32,
}
pub struct Frame<'a> {
    pub target: &'a wgpu::TextureView,
    pub depth: &'a wgpu::TextureView,
    pub size: [u32; 2],
    pub clear: [f64; 4],
    pub camera: &'a wgpu::Buffer,
    pub bases: &'a wgpu::Buffer,
    pub planes: &'a [[f32; 4]; 5],
    /// Solid, cutout, translucent commands, retaining their required order.
    pub draws: &'a [Draw],
    pub passes: [Range<usize>; 3],
    /// How many solid commands, from the start of `passes[0]`, are drawn first
    /// as occluders. Every later command is then tested against their depth
    /// (`hiz_seed`, `hiz_reduce`, `occlude`). Zero draws in one pass without
    /// occlusion. The depth view must allow shader reads for occlusion.
    pub occluders: usize,
}
/// Prepared deferred GPU work. The encoder and callbacks own all resources;
/// wgpu retains each callback through its submission fence, including teardown.
pub struct Encoded {
    encoder: wgpu::CommandEncoder,
    /// With occluders: the occluder draws (own pass) and the depth pyramid plus
    /// occlusion test that run between that pass and the rest.
    occluding: Option<(
        std::sync::Arc<dyn wgpu::wgc::command::NativeCommand>,
        std::sync::Arc<dyn wgpu::wgc::command::NativeCommand>,
    )>,
    opaque: std::sync::Arc<dyn wgpu::wgc::command::NativeCommand>,
    translucent: std::sync::Arc<dyn wgpu::wgc::command::NativeCommand>,
    target: wgpu::TextureView,
    depth: wgpu::TextureView,
    clear: [f64; 4],
    size: [u32; 2],
    calls: u32,
}
impl Encoded {
    pub fn calls(&self) -> u32 {
        self.calls
    }
    /// Records opaque terrain, ordinary entities and translucent terrain in one
    /// wgpu-tracked render pass, then returns the encoder for labels/hands/UI.
    pub fn render(self, entities: impl FnOnce(&mut wgpu::RenderPass<'_>)) -> wgpu::CommandEncoder {
        let Self {
            mut encoder,
            occluding,
            opaque,
            translucent,
            target,
            depth,
            clear,
            size,
            ..
        } = self;
        let clear_color = wgpu::LoadOp::Clear(wgpu::Color {
            r: clear[0],
            g: clear[1],
            b: clear[2],
            a: clear[3],
        });
        let (color_load, depth_load) = if let Some((occluders, occlusion)) = occluding {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Native terrain occluders"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: clear_color,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
                pass.set_scissor_rect(0, 0, size[0], size[1]);
                record_render(&mut pass, occluders);
            }
            // Between the two passes: reads the depth the occluders just stored
            // and rewrites only this frame's own commands.
            record_standalone(&mut encoder, occlusion);
            (wgpu::LoadOp::Load, wgpu::LoadOp::Load)
        } else {
            (clear_color, wgpu::LoadOp::Clear(0.0))
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Native terrain and entities"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: color_load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: depth_load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
            pass.set_scissor_rect(0, 0, size[0], size[1]);
            // SAFETY: the backend prepared compatible immutable resources and
            // callbacks for exactly these attachments. They preserve the pass;
            // normal entity drawing rebinds its complete pipeline/buffer state.
            record_render(&mut pass, opaque);
            entities(&mut pass);
            record_render(&mut pass, translucent);
        }
        encoder
    }
}
pub trait TerrainEncoder {
    /// Prepares frame-local compute and render callbacks in deferred order.
    fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame<'_>,
    ) -> Result<Encoded, String>;
    /// Commands the occlusion test has dropped since the last call, over
    /// completed GPU work (wait for it first for an exact count).
    fn take_occluded(&mut self) -> u32;
}
pub fn create(
    device: &wgpu::Device,
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
) -> Result<Option<Box<dyn TerrainEncoder>>, String> {
    #[cfg(target_os = "macos")]
    {
        macos::create(device, texture, format)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (device, texture, format);
        Ok(None)
    }
}

fn record_standalone(
    encoder: &mut wgpu::CommandEncoder,
    command: std::sync::Arc<dyn wgpu::wgc::command::NativeCommand>,
) {
    #[cfg(target_os = "macos")]
    macos::record_standalone(encoder, command);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (encoder, command);
        unreachable!("native terrain is unavailable on this platform");
    }
}

fn record_render(
    pass: &mut wgpu::RenderPass<'_>,
    command: std::sync::Arc<dyn wgpu::wgc::command::NativeCommand>,
) {
    #[cfg(target_os = "macos")]
    macos::record_render(pass, command);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pass, command);
        unreachable!("native terrain is unavailable on this platform");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{error::Error, time::Duration};
    fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        usage: wgpu::BufferUsages,
    ) -> wgpu::Buffer {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes.len() as u64,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, bytes);
        buffer
    }
    #[test]
    #[ignore = "Requires native Metal and GPU-generated indirect render commands"]
    fn native_indexed_frustum_commands_preserve_pixels_and_reset_between_frames()
    -> Result<(), Box<dyn Error>> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            texture.size(),
        );
        let mut native = create(&device, &texture, wgpu::TextureFormat::Rgba8Unorm)?
            .ok_or("Native terrain backend unavailable")?;
        let mut words = Vec::new();
        for (draw, (low, high)) in [(-0.875f32, -0.375f32), (0.375, 0.875), (-0.25, 0.25)]
            .into_iter()
            .enumerate()
        {
            for (x, y) in [(low, -0.75f32), (high, -0.75), (high, 0.75), (low, 0.75)] {
                let x = ((x + 8.0) * 64.0).round() as u32;
                let y = ((y + 8.0) * 64.0).round() as u32;
                let z = ((0.5f32 + 8.0) * 64.0).round() as u32;
                let color = 255u32 << (8 + 8 * draw);
                words.extend([x | z << 11, y, color]);
            }
        }
        let vertices = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&words),
            wgpu::BufferUsages::VERTEX,
        );
        let alternate = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&words),
            wgpu::BufferUsages::VERTEX,
        );
        let bases = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&[0.0f32; 9]),
            wgpu::BufferUsages::VERTEX,
        );
        let mut camera = [0.0f32; 24];
        for index in [0, 5, 10, 15] {
            camera[index] = 1.0;
        }
        camera[19] = 1000.0;
        camera[23] = 900.0;
        let camera = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&camera),
            wgpu::BufferUsages::UNIFORM,
        );
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: target.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let draws: Vec<_> = (0..3)
            .map(|index| Draw {
                vertices: if index == 1 {
                    alternate.clone()
                } else {
                    vertices.clone()
                },
                low: if index == 2 {
                    [10.0, -0.75, 0.5]
                } else {
                    [-0.875, -0.75, 0.5]
                },
                high: if index == 2 {
                    [11.0, 0.75, 0.5]
                } else {
                    [0.875, 0.75, 0.5]
                },
                indices: if index == 1 { 6..12 } else { 0..6 },
                base_vertex: if index == 1 { 0 } else { index * 4 },
                instance: index as u32,
            })
            .collect();
        let view = target.create_view(&Default::default());
        let depth = depth.create_view(&Default::default());
        let all_planes = [[0.0, 0.0, 0.0, 1.0]; 5];
        {
            let mapped = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Mapped native input regression"),
                size: vertices.size(),
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: true,
            });
            let mut isolated = create(&device, &texture, wgpu::TextureFormat::Rgba8Unorm)?
                .ok_or("Native terrain backend unavailable")?;
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let invalid = Draw {
                vertices: mapped.clone(),
                low: [0.0; 3],
                high: [1.0; 3],
                indices: 0..6,
                base_vertex: 0,
                instance: 0,
            };
            let encoded = isolated.encode(
                &device,
                &queue,
                &Frame {
                    target: &view,
                    depth: &depth,
                    size: [64, 64],
                    clear: [0.0; 4],
                    camera: &camera,
                    bases: &bases,
                    planes: &all_planes,
                    draws: &[invalid],
                    passes: [0..1, 1..1, 1..1],
                    occluders: 0,
                },
            )?;
            queue.submit([encoded.render(|_| {}).finish()]);
            let error =
                pollster::block_on(scope.pop()).expect("mapped native input reached submission");
            assert!(
                error.to_string().to_lowercase().contains("mapped"),
                "{error}"
            );
            mapped.unmap();
        }
        for (range, base_vertex, instance) in [
            (1..6, 0, 0),
            (0..7, 0, 0),
            (0..24, 0, 0),
            (0..6, -1, 0),
            (0..6, i32::MAX, 0),
            (0..6, 0, 3),
        ] {
            let invalid = Draw {
                vertices: vertices.clone(),
                low: [0.0; 3],
                high: [1.0; 3],
                indices: range,
                base_vertex,
                instance,
            };
            assert!(
                native
                    .encode(
                        &device,
                        &queue,
                        &Frame {
                            target: &view,
                            depth: &depth,
                            size: [64, 64],
                            clear: [0.0; 4],
                            camera: &camera,
                            bases: &bases,
                            planes: &all_planes,
                            draws: &[invalid],
                            passes: [0..1, 1..1, 1..1],
                            occluders: 0,
                        }
                    )
                    .is_err(),
                "invalid hardware-fetch address range accepted"
            );
        }
        let mut pending = Vec::new();
        let mut submitted = Vec::new();
        for (frame, bound) in [100.0f32, 1.0, 1.0, 1.0].into_iter().enumerate() {
            let planes = [
                [1.0, 0.0, 0.0, bound],
                [-1.0, 0.0, 0.0, bound],
                [0.0, 1.0, 0.0, bound],
                [0.0, -1.0, 0.0, bound],
                [0.0, 0.0, 1.0, 0.0],
            ];
            let mut extended = Vec::new();
            if frame == 2 {
                for _ in 0..6 {
                    extended.push(Draw {
                        vertices: vertices.clone(),
                        low: [1000.0, 0.0, 0.5],
                        high: [1001.0, 1.0, 0.5],
                        indices: 0..6,
                        base_vertex: 0,
                        instance: 0,
                    });
                }
            }
            let active: Vec<Draw> = draws
                .iter()
                .chain(&extended)
                .map(|d| Draw {
                    vertices: if frame == 2 {
                        vertices.clone()
                    } else {
                        d.vertices.clone()
                    },
                    low: d.low,
                    high: d.high,
                    indices: d.indices.clone(),
                    base_vertex: d.base_vertex,
                    instance: d.instance,
                })
                .collect();
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 64 * 64 * 4,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            let description = Frame {
                target: &view,
                depth: &depth,
                size: [64, 64],
                clear: [0.0, 0.0, 0.0, 1.0],
                camera: &camera,
                bases: &bases,
                planes: &planes,
                draws: &active,
                passes: [
                    0..active.len(),
                    active.len()..active.len(),
                    active.len()..active.len(),
                ],
                occluders: 0,
            };
            let calls = native.encode(&device, &queue, &description)?;
            assert_eq!(calls.calls(), 1);
            encoder.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: None,
                    },
                },
                target.size(),
            );
            let world = calls.render(|_| {});
            pending.push((frame, readback, world.finish(), encoder.finish()));
            if frame == 2 {
                assert!(
                    native.encode(&device, &queue, &description).is_err(),
                    "three live unsubmitted slots were overwritten"
                );
                for (frame, readback, world, copy) in pending.drain(..) {
                    let submission = queue.submit([world, copy]);
                    submitted.push((frame, readback, submission));
                }
                device.poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(5)),
                })?;
            }
        }
        // Retired native command/metadata allocations and their resources must
        // survive capacity growth and dropping the producer before readback.
        drop((native, draws, vertices, alternate, bases, camera, texture));
        for (frame, readback, world, copy) in pending {
            let submission = queue.submit([world, copy]);
            submitted.push((frame, readback, submission));
        }
        for (frame, readback, submission) in submitted {
            let (send, receive) = std::sync::mpsc::channel();
            readback.map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = send.send(result);
            });
            device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(5)),
            })?;
            receive.recv_timeout(Duration::from_secs(5))??;
            let pixels = readback.get_mapped_range(..)?;
            let counts = [0, 1, 2].map(|channel| {
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[channel] > 200)
                    .count()
            });
            assert!(
                counts[0] > 100 && counts[1] > 100,
                "missing inherited indexed geometry: {counts:?}"
            );
            if frame == 0 {
                assert!(counts[2] > 100, "GPU include failed: {counts:?}");
            } else {
                assert_eq!(counts[2], 0, "GPU cull/reset failed");
            }
            assert!(pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
            drop(pixels);
            readback.unmap();
            eprintln!("NATIVE_TERRAIN_FRAME_PASS frame={frame} pixels={counts:?}");
        }
        Ok(())
    }
    #[test]
    #[ignore = "Requires native Metal; verifies translucent composition and ownership through submission"]
    fn native_translucency_follows_entities_and_survives_producer_teardown()
    -> Result<(), Box<dyn Error>> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &[255, 255, 255, 255, 255, 255, 255, 128],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            texture.size(),
        );
        let mut native = create(&device, &texture, wgpu::TextureFormat::Rgba8Unorm)?
            .ok_or("Native backend unavailable")?;
        let mut words = Vec::new();
        for (layer, z, color) in [(0u32, 0.5f32, 255u32 << 8), (1, 0.796875, 255 << 24)] {
            for (x, y) in [
                (-0.75f32, -0.75f32),
                (0.75, -0.75),
                (0.75, 0.75),
                (-0.75, 0.75),
            ] {
                let x = ((x + 8.0) * 64.0) as u32;
                let y = ((y + 8.0) * 64.0) as u32;
                let z = ((z + 8.0) * 64.0) as u32;
                words.extend([x | z << 11 | layer << 22, y, color]);
            }
        }
        let vertices = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&words),
            wgpu::BufferUsages::VERTEX,
        );
        let bases = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&[0.0f32; 3]),
            wgpu::BufferUsages::VERTEX,
        );
        let mut camera = [0.0f32; 24];
        for index in [0, 5, 10, 15] {
            camera[index] = 1.0;
        }
        camera[19] = 1000.0;
        camera[23] = 900.0;
        let camera = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&camera),
            wgpu::BufferUsages::UNIFORM,
        );
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: target.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let draws: Vec<_> = [0, 4]
            .into_iter()
            .map(|base_vertex| Draw {
                vertices: vertices.clone(),
                low: [-0.75, -0.75, 0.5],
                high: [0.75, 0.75, 0.8],
                indices: 0..6,
                base_vertex,
                instance: 0,
            })
            .collect();
        let planes = [[0.0, 0.0, 0.0, 1.0]; 5];
        let batch = native.encode(
            &device,
            &queue,
            &Frame {
                target: &view,
                depth: &depth_view,
                size: [64, 64],
                clear: [0.0, 0.0, 0.0, 1.0],
                camera: &camera,
                bases: &bases,
                planes: &planes,
                draws: &draws,
                passes: [0..1, 1..1, 1..2],
                occluders: 0,
            },
        )?;
        assert_eq!(batch.calls(), 2);
        // Only the private batch keeps the native pipeline, textures and all
        // buffer generations alive from this point until GPU completion.
        drop((native, draws, vertices, bases, camera, texture));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl("@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> { let positions = array(vec2(-0.25,-0.5),vec2(0.25,-0.5),vec2(0.25,0.5),vec2(-0.25,-0.5),vec2(0.25,0.5),vec2(-0.25,0.5)); return vec4(positions[i],0.7,1.0); } @fragment fn fragment() -> @location(0) vec4<f32> { return vec4(0.0,1.0,0.0,1.0); }".into()) });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let world = batch.render(|pass| {
            pass.set_pipeline(&pipeline);
            pass.draw(0..6, 0..1);
        });
        let mut after = device.create_command_encoder(&Default::default());
        let mut readbacks = Vec::new();
        for (texture, aspect) in [
            (&target, wgpu::TextureAspect::All),
            (&depth, wgpu::TextureAspect::DepthOnly),
        ] {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 64 * 64 * 4,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            after.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: None,
                    },
                },
                texture.size(),
            );
            readbacks.push(buffer);
        }
        let submission = queue.submit([world.finish(), after.finish()]);
        for (kind, readback) in readbacks.into_iter().enumerate() {
            let (send, receive) = std::sync::mpsc::channel();
            readback.map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = send.send(result);
            });
            device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission.clone()),
                timeout: Some(Duration::from_secs(5)),
            })?;
            receive.recv_timeout(Duration::from_secs(5))??;
            let bytes = readback.get_mapped_range(..)?;
            for (x, expected) in [(32usize, [0u8, 127, 128, 255]), (16, [127, 0, 128, 255])] {
                let offset = (32 * 64 + x) * 4;
                if kind == 0 {
                    assert!(
                        bytes[offset..offset + 4]
                            .iter()
                            .zip(expected)
                            .all(|(&a, b)| a.abs_diff(b) <= 2),
                        "translucency order wrong: {:?}",
                        &bytes[offset..offset + 4]
                    );
                } else {
                    let value = f32::from_le_bytes(bytes[offset..offset + 4].try_into()?);
                    let expected = if x == 32 { 0.7f32 } else { 0.5 };
                    assert!(
                        (value - expected).abs() < 0.0001,
                        "translucent terrain wrote depth: {value}"
                    );
                }
            }
            drop(bytes);
            readback.unmap();
        }
        Ok(())
    }
    #[test]
    #[ignore = "Requires native Metal with 2048 array layers; exercises production packed vertex shader"]
    fn native_packed_vertex_fields_cover_layer_uv_and_position_boundaries()
    -> Result<(), Box<dyn Error>> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_texture_array_layers: 2048,
                    ..Default::default()
                },
                ..Default::default()
            }))?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 2048,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let layer_color = |layer| match layer {
            0 => 32u8,
            1023 => 96,
            1024 => 160,
            2047 => 224,
            _ => 0,
        };
        let uv_color = |cell| match cell {
            0 => 32u8,
            1 => 96,
            2 => 160,
            15 => 224,
            _ => 0,
        };
        let mut texels = Vec::with_capacity(2048 * 16 * 16 * 4);
        for layer in 0..2048 {
            for y in 0..16 {
                for x in 0..16 {
                    texels.extend([layer_color(layer), uv_color(x), uv_color(y), 255]);
                }
            }
        }
        queue.write_texture(
            texture.as_image_copy(),
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(64),
                rows_per_image: Some(16),
            },
            texture.size(),
        );
        let mut native = create(&device, &texture, wgpu::TextureFormat::Rgba8Unorm)?
            .ok_or("Native packed shader unavailable")?;
        let bases = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&[0.0f32; 3]),
            wgpu::BufferUsages::VERTEX,
        );
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: target.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let planes = [[0.0f32, 0.0, 0.0, 1.0]; 5];
        for (case, (layer, u, v)) in [
            (0u32, 0u32, 0u32),
            (1023, 4095, 0),
            (1024, 0, 4095),
            (2047, 4095, 4095),
            (2047, 15, 16),
            (1024, 16, 15),
        ]
        .into_iter()
        .enumerate()
        {
            let x = if case & 1 == 0 { -8.0f32 } else { 23.0 };
            let y = if case & 2 == 0 { -8.0f32 } else { 503.0 };
            let z = if case & 1 == 0 { -8.0f32 } else { 23.984375 };
            let width = 0.984375f32;
            let colors = [255u8, 128, 192];
            let mut words = Vec::new();
            for (px, py) in [
                (x, y),
                (x + width, y),
                (x + width, y + width),
                (x, y + width),
            ] {
                let px = ((px + 8.0) * 64.0) as u32;
                let py = ((py + 8.0) * 64.0) as u32;
                let pz = ((z + 8.0) * 64.0) as u32;
                words.extend([
                    px | pz << 11 | (layer & 1023) << 22,
                    py | (layer >> 10) << 15 | u << 16 | (v & 15) << 28,
                    (v >> 4)
                        | u32::from(colors[0]) << 8
                        | u32::from(colors[1]) << 16
                        | u32::from(colors[2]) << 24,
                ]);
            }
            let vertices = upload(
                &device,
                &queue,
                bytemuck::cast_slice(&words),
                wgpu::BufferUsages::VERTEX,
            );
            let mut camera = [0.0f32; 24];
            camera[0] = 1.5;
            camera[5] = 1.5;
            camera[10] = 0.25;
            camera[12] = -(x + width * 0.5) * 1.5;
            camera[13] = -(y + width * 0.5) * 1.5;
            camera[14] = 0.5 - z * 0.25;
            camera[15] = 1.0;
            camera[19] = 1000.0;
            camera[23] = 900.0;
            let camera = upload(
                &device,
                &queue,
                bytemuck::cast_slice(&camera),
                wgpu::BufferUsages::UNIFORM,
            );
            let draw = Draw {
                vertices,
                low: [x, y, z],
                high: [x + width, y + width, z],
                indices: 0..6,
                base_vertex: 0,
                instance: 0,
            };
            let batch = native.encode(
                &device,
                &queue,
                &Frame {
                    target: &view,
                    depth: &depth_view,
                    size: [64, 64],
                    clear: [0.0; 4],
                    camera: &camera,
                    bases: &bases,
                    planes: &planes,
                    draws: &[draw],
                    passes: [0..1, 1..1, 1..1],
                    occluders: 0,
                },
            )?;
            let mut after = device.create_command_encoder(&Default::default());
            let mut readbacks = Vec::new();
            for (texture, aspect) in [
                (&target, wgpu::TextureAspect::All),
                (&depth, wgpu::TextureAspect::DepthOnly),
            ] {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 64 * 64 * 4,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                after.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(256),
                            rows_per_image: None,
                        },
                    },
                    texture.size(),
                );
                readbacks.push(buffer);
            }
            let world = batch.render(|_| {});
            let submission = queue.submit([world.finish(), after.finish()]);
            for (kind, readback) in readbacks.into_iter().enumerate() {
                let (send, receive) = std::sync::mpsc::channel();
                readback.map_async(wgpu::MapMode::Read, .., move |result| {
                    let _ = send.send(result);
                });
                device.poll(wgpu::PollType::Wait {
                    submission_index: Some(submission.clone()),
                    timeout: Some(Duration::from_secs(5)),
                })?;
                receive.recv_timeout(Duration::from_secs(5))??;
                let bytes = readback.get_mapped_range(..)?;
                if kind == 0 {
                    let linear = |v: u8| {
                        let v = f64::from(v) / 255.0;
                        if v <= 0.04045 {
                            v / 12.92
                        } else {
                            ((v + 0.055) / 1.055).powf(2.4)
                        }
                    };
                    let texel = [
                        layer_color(layer),
                        uv_color((u % 128) / 8),
                        uv_color((v % 128) / 8),
                    ];
                    let expected: [u8; 3] = std::array::from_fn(|i| {
                        (255.0 * linear(texel[i]) * linear(colors[i])).round() as u8
                    });
                    for (pixel, rgba) in bytes.as_chunks::<4>().0.iter().enumerate() {
                        let inside =
                            (8..56).contains(&(pixel % 64)) && (8..56).contains(&(pixel / 64));
                        if inside {
                            assert_eq!(rgba[3], 255, "case {case} lost packed position coverage");
                            assert!(
                                rgba[..3]
                                    .iter()
                                    .zip(expected)
                                    .all(|(&a, b)| a.abs_diff(b) <= 2),
                                "case {case} layer/UV/colour mismatch: {rgba:?} vs {expected:?}"
                            );
                        } else {
                            assert_eq!(
                                *rgba, [0; 4],
                                "case {case} expanded packed position bounds"
                            );
                        }
                    }
                } else {
                    let offset = (32 * 64 + 32) * 4;
                    let value = f32::from_le_bytes(bytes[offset..offset + 4].try_into()?);
                    assert!(
                        (value - 0.5).abs() < 0.0001,
                        "case {case} packed z mismatch: {value}"
                    );
                }
                drop(bytes);
                readback.unmap();
            }
            eprintln!("NATIVE_PACKED_FIELDS_PASS case={case} layer={layer} u={u} v={v}");
        }
        Ok(())
    }

    #[test]
    #[ignore = "Requires native Metal; proves GPU occlusion culls only hidden commands"]
    fn native_occlusion_drops_only_commands_hidden_behind_the_occluders()
    -> Result<(), Box<dyn Error>> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            texture.size(),
        );
        let mut native = create(&device, &texture, wgpu::TextureFormat::Rgba8Unorm)?
            .ok_or("Native terrain backend unavailable")?;
        // Identity camera: x and y are the viewport, z the reversed depth (near = 1).
        // 0: the occluder, a near square over the middle of the view.
        // 1: a far square wholly behind it (must be dropped).
        // 2: a far square reaching past the occluder's right edge (stays).
        // 3: a square in front of the occluder (stays).
        // 4: a far square outside the occluder entirely (stays).
        let quads: [([f32; 4], f32, [u8; 3]); 5] = [
            ([-0.5, -0.5, 0.5, 0.5], 0.9, [255, 0, 0]),
            ([-0.25, -0.25, 0.25, 0.25], 0.3, [0, 255, 0]),
            ([0.25, -0.25, 0.75, 0.25], 0.3, [0, 0, 255]),
            ([-0.25, -0.75, 0.25, -0.375], 0.95, [255, 255, 0]),
            ([-0.875, 0.625, -0.625, 0.875], 0.2, [0, 255, 255]),
        ];
        let mut words = Vec::new();
        for ([x0, y0, x1, y1], z, [r, g, b]) in quads {
            for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
                let pack = |v: f32| ((v + 8.0) * 64.0).round() as u32;
                let color = u32::from(r) << 8 | u32::from(g) << 16 | u32::from(b) << 24;
                words.extend([pack(x) | pack(z) << 11, pack(y), color]);
            }
        }
        let vertices = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&words),
            wgpu::BufferUsages::VERTEX,
        );
        let bases = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&[0.0f32; 3]),
            wgpu::BufferUsages::VERTEX,
        );
        let mut camera = [0.0f32; 24];
        for index in [0, 5, 10, 15] {
            camera[index] = 1.0;
        }
        camera[19] = 1000.0;
        camera[23] = 900.0;
        let camera = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&camera),
            wgpu::BufferUsages::UNIFORM,
        );
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: target.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let view = target.create_view(&Default::default());
        let draws: Vec<Draw> = quads
            .iter()
            .enumerate()
            .map(|(index, &([x0, y0, x1, y1], z, _))| Draw {
                vertices: vertices.clone(),
                low: [x0, y0, z],
                high: [x1, y1, z],
                indices: index as u32 * 6..index as u32 * 6 + 6,
                base_vertex: 0,
                instance: 0,
            })
            .collect();
        let planes = [
            [1.0, 0.0, 0.0, 1.0],
            [-1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, -1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let mut images = Vec::new();
        for occluders in [1usize, 0] {
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 64 * 64 * 4,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let encoded = native.encode(
                &device,
                &queue,
                &Frame {
                    target: &view,
                    depth: &depth,
                    size: [64, 64],
                    clear: [0.0, 0.0, 0.0, 1.0],
                    camera: &camera,
                    bases: &bases,
                    planes: &planes,
                    draws: &draws,
                    passes: [0..5, 5..5, 5..5],
                    occluders,
                },
            )?;
            assert_eq!(encoded.calls(), 1 + occluders as u32);
            let mut copy = device.create_command_encoder(&Default::default());
            copy.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: None,
                    },
                },
                target.size(),
            );
            let world = encoded.render(|_| {});
            let submission = queue.submit([world.finish(), copy.finish()]);
            let (send, receive) = std::sync::mpsc::channel();
            readback.map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = send.send(result);
            });
            device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(5)),
            })?;
            receive.recv_timeout(Duration::from_secs(5))??;
            let dropped = native.take_occluded();
            // Only the square wholly behind the occluder is dropped, and only
            // when occlusion runs.
            assert_eq!(
                dropped,
                if occluders == 1 { 1 } else { 0 },
                "occluders={occluders}"
            );
            images.push(readback.get_mapped_range(..)?.to_vec());
        }
        // Dropping hidden commands never changes a pixel.
        assert_eq!(images[0], images[1]);
        let count = |rgb: [u8; 3]| {
            images[0]
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[..3] == rgb)
                .count()
        };
        assert!(count([255, 0, 0]) > 200, "occluder missing");
        assert!(count([0, 0, 255]) > 50, "partly visible square lost");
        assert!(count([255, 255, 0]) > 50, "nearer square lost");
        assert!(count([0, 255, 255]) > 30, "unoccluded square lost");
        assert_eq!(count([0, 255, 0]), 0);
        eprintln!("NATIVE_OCCLUSION_PASS dropped=1 pixels_identical=true");
        Ok(())
    }
    type OcclusionQuad = ([[f32; 3]; 4], [u8; 3]);

    fn occlusion_quad(bounds: [f32; 4], z: f32, color: [u8; 3]) -> OcclusionQuad {
        let [x0, y0, x1, y1] = bounds;
        ([[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]], color)
    }

    // Compare the same geometry with same-frame HIZ enabled and disabled. Each
    // scene includes exactly one fully hidden control so a disabled/broken cull
    // path cannot satisfy pixel equality by simply retaining every command.
    fn assert_conservative_occlusion(
        quads: &[OcclusionQuad],
        matrix: [f32; 16],
        occluder_count: usize,
        visible_color: [u8; 3],
        minimum_visible_pixels: usize,
    ) -> Result<(), Box<dyn Error>> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            texture.size(),
        );
        let mut native = create(&device, &texture, wgpu::TextureFormat::Rgba8Unorm)?
            .ok_or("Native terrain backend unavailable")?;
        let mut words = Vec::new();
        for (points, [r, g, b]) in quads {
            for &[x, y, z] in points {
                let pack = |v: f32| ((v + 8.0) * 64.0).round() as u32;
                let color = u32::from(*r) << 8 | u32::from(*g) << 16 | u32::from(*b) << 24;
                words.extend([pack(x) | pack(z) << 11, pack(y), color]);
            }
        }
        let vertices = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&words),
            wgpu::BufferUsages::VERTEX,
        );
        let bases = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&[0.0f32; 3]),
            wgpu::BufferUsages::VERTEX,
        );
        let mut camera = [0.0f32; 24];
        camera[..16].copy_from_slice(&matrix);
        camera[19] = 1000.0;
        camera[23] = 900.0;
        let camera = upload(
            &device,
            &queue,
            bytemuck::cast_slice(&camera),
            wgpu::BufferUsages::UNIFORM,
        );
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: target.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let view = target.create_view(&Default::default());
        let draws: Vec<Draw> = quads
            .iter()
            .enumerate()
            .map(|(index, (points, _))| Draw {
                vertices: vertices.clone(),
                low: std::array::from_fn(|axis| {
                    points.iter().map(|p| p[axis]).fold(f32::INFINITY, f32::min)
                }),
                high: std::array::from_fn(|axis| {
                    points
                        .iter()
                        .map(|p| p[axis])
                        .fold(f32::NEG_INFINITY, f32::max)
                }),
                indices: index as u32 * 6..index as u32 * 6 + 6,
                base_vertex: 0,
                instance: 0,
            })
            .collect();
        // Isolate HIZ: all commands pass frustum culling, including eye-crossing boxes.
        let planes = [[0.0, 0.0, 0.0, 1.0]; 5];
        let mut images = Vec::new();
        for occluders in [occluder_count, 0] {
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 64 * 64 * 4,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let encoded = native.encode(
                &device,
                &queue,
                &Frame {
                    target: &view,
                    depth: &depth,
                    size: [64, 64],
                    clear: [0.0, 0.0, 0.0, 1.0],
                    camera: &camera,
                    bases: &bases,
                    planes: &planes,
                    draws: &draws,
                    passes: [
                        0..draws.len(),
                        draws.len()..draws.len(),
                        draws.len()..draws.len(),
                    ],
                    occluders,
                },
            )?;
            assert_eq!(encoded.calls(), 1 + u32::from(occluders > 0));
            assert_eq!(
                native.take_occluded(),
                0,
                "unsubmitted frame reported drops"
            );
            let mut copy = device.create_command_encoder(&Default::default());
            copy.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: None,
                    },
                },
                target.size(),
            );
            let world = encoded.render(|_| {});
            let submission = queue.submit([world.finish(), copy.finish()]);
            let (send, receive) = std::sync::mpsc::channel();
            readback.map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = send.send(result);
            });
            device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(5)),
            })?;
            receive.recv_timeout(Duration::from_secs(5))??;
            let dropped = native.take_occluded();
            // The hidden control must be dropped; every other candidate must stay.
            assert_eq!(
                dropped,
                if occluders > 0 { 1 } else { 0 },
                "occluders={occluders}"
            );
            let bytes = readback.get_mapped_range(..)?;
            images.push(bytes.to_vec());
            drop(bytes);
            readback.unmap();
        }
        assert_eq!(images[0], images[1], "HIZ changed visible pixels");
        let visible = images[0]
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[..3] == visible_color)
            .count();
        assert!(
            visible >= minimum_visible_pixels,
            "candidate never exercised visible coverage: {visible} pixels"
        );
        Ok(())
    }

    #[test]
    #[ignore = "Requires native Metal; proves perspective clipping stays conservative"]
    fn native_occlusion_preserves_eye_and_near_plane_crossing_commands()
    -> Result<(), Box<dyn Error>> {
        // Infinite reversed-Z perspective: clip = (x, y, 1/8, z). The near
        // plane is world z = 1/8 and the eye plane is world z = 0.
        let matrix = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.125, 0.0,
        ];
        for crossing_z in [-0.5, 0.0625] {
            let quads = [
                // Covers NDC [-.75, .75], with reversed depth .5.
                occlusion_quad([-0.1875, -0.1875, 0.1875, 0.1875], 0.25, [255, 0, 0]),
                // This side-facing quad crosses the eye or near plane. Its
                // clipped portion reaches the right viewport edge, outside the
                // wall. For the eye crossing, dividing only the eight corners
                // falsely puts the whole box behind the middle of the wall:
                // only the clip.w guard prevents a false occlusion.
                (
                    [
                        [0.125, -0.125, crossing_z],
                        [0.125, 0.125, crossing_z],
                        [0.125, 0.125, 0.5],
                        [0.125, -0.125, 0.5],
                    ],
                    [0, 255, 0],
                ),
                occlusion_quad([-0.0625, -0.0625, 0.0625, 0.0625], 0.5, [0, 0, 255]),
            ];
            assert_conservative_occlusion(&quads, matrix, 1, [0, 255, 0], 30)?;
        }
        Ok(())
    }

    #[test]
    #[ignore = "Requires native Metal; proves HIZ preserves small occluder holes"]
    fn native_occlusion_preserves_commands_visible_through_an_occluder_hole()
    -> Result<(), Box<dyn Error>> {
        let matrix = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        // A two-by-two pixel hole at x=37..39, y=29..31: off-center and
        // inside a coarse HIZ texel, away from the candidate's corner samples.
        // Taking maximum depth during reduction or sampling only its corners
        // would incorrectly hide the blue quad behind this otherwise solid wall.
        let quads = [
            occlusion_quad([-0.875, -0.875, 0.15625, 0.875], 0.75, [255, 0, 0]),
            occlusion_quad([0.21875, -0.875, 0.875, 0.875], 0.75, [255, 0, 0]),
            occlusion_quad([0.15625, -0.875, 0.21875, 0.03125], 0.75, [255, 0, 0]),
            occlusion_quad([0.15625, 0.09375, 0.21875, 0.875], 0.75, [255, 0, 0]),
            occlusion_quad([-0.5, -0.5, 0.5, 0.5], 0.25, [0, 0, 255]),
            occlusion_quad([-0.625, -0.125, -0.5, 0.125], 0.25, [0, 255, 0]),
        ];
        assert_conservative_occlusion(&quads, matrix, 4, [0, 0, 255], 4)
    }
}
