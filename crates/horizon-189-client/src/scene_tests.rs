use crate::scene::Scene;
use std::{error::Error, path::PathBuf, time::Duration};

#[test]
#[ignore = "Requires a Metal GPU and HORIZON_189_TEST_JAR pointing at an owned 1.8.9 jar"]
fn owned_model_renders_non_background_pixels_on_metal() -> Result<(), Box<dyn Error>> {
    let jar =
        PathBuf::from(std::env::var_os("HORIZON_189_TEST_JAR").ok_or("Set HORIZON_189_TEST_JAR")?);
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
    assert_eq!(adapter.get_info().backend, wgpu::Backend::Metal);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_texture_array_layers: adapter.limits().max_texture_array_layers,
            ..Default::default()
        },
        ..Default::default()
    }))?;
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Rgba8Unorm,
        color_space: wgpu::SurfaceColorSpace::Auto,
        width: 256,
        height: 256,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![],
    };
    let mut scene = Scene::new(&device, &queue, &config, &jar)?;
    let size = wgpu::Extent3d {
        width: 256,
        height: 256,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Fixture readback target"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fixture readback"),
        size: 256 * 256 * 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    scene.prepare(&device, &queue, &config);
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Owned model verification"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &scene.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        scene.draw(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1024),
                rows_per_image: None,
            },
        },
        size,
    );
    let submission = queue.submit([encoder.finish()]);
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
    let visible = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[..3] != [0, 0, 0])
        .count();
    assert!(
        (2000..40000).contains(&visible),
        "Unexpected model coverage: {visible}"
    );
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    if let Some(path) = std::env::var_os("HORIZON_189_CAPTURE") {
        let file = std::fs::File::create(path)?;
        let mut encoder = png::Encoder::new(file, 256, 256);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&pixels)?;
    }
    eprintln!("METAL_READBACK_PASS visible_pixels={visible}");
    drop(pixels);
    readback.unmap();
    // Reusing the window after a lost connection must not render its old fixture.
    scene.clear_world();
    assert_eq!(scene.column_count(), 0);
    assert!(!scene.has_geometry());
    scene.prepare(&device, &queue, &config);
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Disconnected world verification"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &scene.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        scene.draw(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1024),
                rows_per_image: None,
            },
        },
        size,
    );
    let submission = queue.submit([encoder.finish()]);
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
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 0, 255]),
        "stale world survived disconnect"
    );
    drop(pixels);
    readback.unmap();
    eprintln!("METAL_DISCONNECT_CLEAR_PASS");

    let resources = crate::ui::Resources::load(&horizon_assets_189::Assets::open(&jar)?)?;
    let mut ui = crate::ui::Pass::new(
        &device,
        &queue,
        config.format,
        &resources,
        scene.entity_texture(),
    );
    let layout = crate::ui::Layout::new((256, 256), 1.0, 1);
    let mut canvas = crate::ui::Canvas::new(&resources, layout, 0, &ui.pages);
    let mut screen = crate::connection::Screen::default();
    screen.lost(r#"{"text":"Reference server closed","color":"red"}"#.into());
    screen.draw(&mut canvas, (128.0, 205.0));
    let (vertices, inverted, missing) = canvas.finish();
    ui.upload(
        &device,
        &queue,
        layout,
        scene.entity_layers(),
        &inverted,
        &vertices,
        None,
    )?;
    ui.load_pages(&queue, &resources, &missing);
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Connection recovery screen"),
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
        ui.draw_part(&mut pass, 0);
        ui.draw_part(&mut pass, 1);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1024),
                rows_per_image: None,
            },
        },
        size,
    );
    let submission = queue.submit([encoder.finish()]);
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
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 150 && p[1] < 120)
            .count()
            > 100,
        "formatted reason absent"
    );
    if let Some(path) = std::env::var_os("HORIZON_189_CONNECTION_CAPTURE") {
        let file = std::fs::File::create(path)?;
        let mut encoder = png::Encoder::new(file, 256, 256);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&pixels)?;
    }
    drop(pixels);
    readback.unmap();
    eprintln!("METAL_CONNECTION_SCREEN_PASS");
    Ok(())
}

#[test]
#[ignore = "Requires a Metal GPU"]
fn packed_terrain_fields_decode_on_metal() -> Result<(), Box<dyn Error>> {
    use crate::terrain::TerrainVertex;
    use wgpu::util::DeviceExt;
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;
    let mut packed = Vec::new();
    let mut expected = Vec::new();
    for layer in [0, 1, 360, 1023, 1024, 2047] {
        for (position, uv, color) in [
            ([-8.0, -8.0, -8.0], [0.0, 0.0], [0.0, 0.0, 0.0]),
            ([16.0, 256.0, 16.0], [16.0, 16.0], [1.0, 0.5, 0.25]),
            ([23.9, 503.9, 1.25], [31.99, 0.125], [0.2, 0.0, 1.0]),
            ([0.0, 63.0 + 8.0 / 9.0, 0.0], [0.5, 0.75], [0.8; 3]),
        ] {
            packed.extend(TerrainVertex::new(position, uv, layer, color).packed);
            let p = position.map(|v| ((v + 8.0) * 64.0).round() / 64.0 - 8.0);
            let uv = uv.map(|v| (v * 128.0).round() / 128.0);
            let color = color.map(|v| (v * 255.0).round() / 255.0);
            expected.push([
                p[0],
                p[1],
                p[2],
                uv[0],
                uv[1],
                layer as f32,
                color[0],
                color[1],
                color[2],
            ]);
        }
    }
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Packed terrain inputs"),
        contents: bytemuck::cast_slice(&packed),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let bytes = (expected.len() * 9 * 4) as u64;
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Decoded terrain output"),
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Decoded terrain readback"),
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    // Execute the production decoder, not a test copy of its masks/shifts.
    let source = format!(
        "{}{}",
        include_str!("scene.wgsl"),
        r#"
@group(0) @binding(3) var<storage, read> probe_input: array<u32>;
@group(0) @binding(4) var<storage, read_write> probe_output: array<f32>;
@compute @workgroup_size(1)
fn packing_probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    let decoded = decode_terrain(vec3<u32>(probe_input[i*3u], probe_input[i*3u+1u], probe_input[i*3u+2u]));
    for (var c = 0u; c < 3u; c++) {
        probe_output[i*9u+c] = decoded.position[c];
        probe_output[i*9u+6u+c] = decoded.color[c];
    }
    probe_output[i*9u+3u] = decoded.uv.x;
    probe_output[i*9u+4u] = decoded.uv.y;
    probe_output[i*9u+5u] = f32(decoded.layer);
}
"#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Production terrain decoder probe"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Terrain packing parity"),
        layout: None,
        module: &shader,
        entry_point: Some("packing_probe"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Terrain probe buffers"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 3,
                resource: input.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(expected.len() as u32, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
    let submission = queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = send.send(result);
    });
    device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(Duration::from_secs(5)),
    })?;
    receive.recv_timeout(Duration::from_secs(5))??;
    let mapped = readback.get_mapped_range(..)?;
    let actual: &[f32] = bytemuck::cast_slice(&mapped);
    for (index, (&actual, expected)) in actual.iter().zip(expected.iter().flatten()).enumerate() {
        assert!(
            (actual - expected).abs() < 0.00001,
            "decoded component {index}: {actual} != {expected}"
        );
    }
    drop(mapped);
    readback.unmap();
    eprintln!("METAL_TERRAIN_PACKING_PASS cases={}", expected.len());
    Ok(())
}
