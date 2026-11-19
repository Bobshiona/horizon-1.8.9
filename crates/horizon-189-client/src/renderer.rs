//! Reduced device/surface boundary following Horizon's gfx/device.rs ownership
//! pattern. This path needs no modern registry or prepared asset tables.
use crate::{
    network::MeshUpdate,
    scene::{SKY, Scene},
    terrain::Registry,
};
use horizon_protocol_47::play::PositionLook;
use std::{error::Error, path::PathBuf, sync::Arc};
use winit::{dpi::PhysicalSize, window::Window};

pub struct Renderer {
    pub window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    drawable: bool,
    scene: Option<Scene>,
    pub registry: Option<Arc<Registry>>,
    world_view: bool,
    view_distance: u8,
    pose: PositionLook,
    entity_vertices: Vec<crate::terrain::Vertex>,
    cursor: [f32; 2],
    offscreen: Option<wgpu::Texture>,
    pub offscreen_frames: u64,
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        assets: Option<PathBuf>,
        world_view: bool,
        view_distance: u8,
    ) -> Result<Self, Box<dyn Error>> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        // Enforce Metal explicitly. Environment backend overrides cannot select
        // an alternative API for this executable.
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await?;
        let info = adapter.get_info();
        if info.backend != wgpu::Backend::Metal {
            return Err("Metal adapter required".into());
        }
        eprintln!(
            "gpu={} backend={:?} protocol={}",
            info.name,
            info.backend,
            horizon_protocol_47::VERSION
        );
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Horizon 1.8.9 Metal"),
                // The legacy terrain atlas is one array layer per sprite
                // (361 in 1.8.9); Metal devices allow 2048.
                required_limits: wgpu::Limits {
                    max_texture_array_layers: adapter.limits().max_texture_array_layers,
                    ..Default::default()
                },
                ..Default::default()
            })
            .await?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("Surface has no supported configuration")?;
        if surface
            .get_capabilities(&adapter)
            .usages
            .contains(wgpu::TextureUsages::COPY_SRC)
        {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = 2;
        let registry = assets
            .as_ref()
            .map(|jar| Registry::open(jar).map(Arc::new))
            .transpose()?;
        let scene = registry
            .as_ref()
            .map(|registry| {
                Scene::from_registry(
                    &device,
                    &queue,
                    &config,
                    registry,
                    !world_view,
                    view_distance,
                )
            })
            .transpose()?;
        let mut renderer = Self {
            window,
            instance,
            surface,
            adapter,
            device,
            queue,
            config,
            drawable: false,
            scene,
            registry,
            world_view,
            view_distance,
            pose: PositionLook::default(),
            entity_vertices: Vec::new(),
            cursor: [0.0; 2],
            offscreen: None,
            offscreen_frames: 0,
        };
        renderer.resize(size);
        Ok(renderer)
    }

    /// Upload one rebuilt column from the mesher thread.
    pub fn apply_mesh(&mut self, update: MeshUpdate) -> Result<(), Box<dyn Error>> {
        let scene = self.scene.as_mut().ok_or("World update requires a scene")?;
        let origin_changed = update.origin != scene.origin();
        scene.apply(&self.device, &self.queue, update)?;
        if origin_changed {
            scene.set_camera(&self.queue, &self.config, self.pose);
        }
        Ok(())
    }

    /// Per-frame eye position, interpolated between simulation ticks.
    pub fn set_pose(&mut self, pose: PositionLook) {
        self.pose = pose;
        if let Some(scene) = &mut self.scene {
            scene.set_camera(&self.queue, &self.config, pose);
        }
    }

    pub fn set_cursor(&mut self, position: (f64, f64)) {
        let scale = 300.0 / self.config.height.max(1) as f64;
        self.cursor = [
            ((position.0 - self.config.width as f64 / 2.0) * scale) as f32,
            ((position.1 - self.config.height as f64 / 2.0) * scale) as f32,
        ];
    }

    /// Rebuild moving-entity geometry at this frame's partial tick.
    pub fn set_entities(
        &mut self,
        entities: &[(i32, horizon_world_189::entities::Entity)],
        partial: f32,
        ui: Option<&(Arc<crate::inventory::View>, Option<u8>, bool)>,
    ) -> Result<(), Box<dyn Error>> {
        let Some(scene) = &mut self.scene else {
            return Ok(());
        };
        let mut vertices = std::mem::take(&mut self.entity_vertices);
        vertices.clear();
        crate::models::entities(
            &mut vertices,
            entities,
            partial,
            scene.origin(),
            self.registry.as_deref(),
        );
        if let Some(registry) = &self.registry {
            crate::models::nametags(
                &mut vertices,
                entities,
                partial,
                scene.origin(),
                self.pose,
                &registry.glyphs,
            );
        }
        if let Some((view, swing, using_item)) = ui
            && let Some(registry) = &self.registry
        {
            crate::models::hud(
                &mut vertices,
                view,
                *swing,
                *using_item,
                partial,
                self.pose,
                scene.origin(),
                registry,
                self.config.width as f32 / self.config.height.max(1) as f32,
                self.cursor,
            );
        }
        let result = scene.set_entities(&self.device, &self.queue, &vertices);
        self.entity_vertices = vertices;
        result
    }

    pub fn world_ready(&self) -> bool {
        self.scene.as_ref().is_some_and(Scene::has_geometry)
    }

    pub fn column_count(&self) -> usize {
        self.scene.as_ref().map_or(0, Scene::column_count)
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        let max = self.device.limits().max_texture_dimension_2d;
        self.offscreen = None;
        self.drawable = (1..=max).contains(&size.width) && (1..=max).contains(&size.height);
        if self.drawable {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
            if let Some(scene) = &mut self.scene {
                scene.resize(&self.device, &self.queue, &self.config);
            }
            eprintln!(
                "drawable={}x{} backing_scale={}",
                size.width,
                size.height,
                self.window.scale_factor()
            );
        }
    }

    /// Read the presented frame back and write it as an sRGB PNG.
    fn capture(
        &self,
        texture: &wgpu::Texture,
        path: &std::path::Path,
    ) -> Result<(), Box<dyn Error>> {
        if !self.config.usage.contains(wgpu::TextureUsages::COPY_SRC) {
            return Err("Surface does not allow frame capture".into());
        }
        let (width, height) = (self.config.width, self.config.height);
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frame capture"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Frame capture"),
            });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        let data = slice.get_mapped_range()?;
        let bgra = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for line in data.chunks(row as usize).take(height as usize) {
            for pixel in line[..(width * 4) as usize].as_chunks::<4>().0 {
                let [a, b, c, d] = *pixel;
                rgba.extend_from_slice(&if bgra { [c, b, a, d] } else { [a, b, c, d] });
            }
        }
        drop(data);
        buffer.unmap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut encoder = png::Encoder::new(std::fs::File::create(path)?, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&rgba)?;
        eprintln!(
            "FRAME_CAPTURED path={} size={width}x{height}",
            path.display()
        );
        Ok(())
    }

    pub fn render(
        &mut self,
        capture: Option<&std::path::Path>,
        smoke: bool,
    ) -> Result<bool, Box<dyn Error>> {
        if !self.drawable {
            return Ok(false);
        }
        let (frame, reconfigure) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (Some(frame), false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (Some(frame), true),
            wgpu::CurrentSurfaceTexture::Occluded if smoke => (None, false),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize(self.window.inner_size());
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                let previous_format = self.config.format;
                self.surface = self.instance.create_surface(self.window.clone())?;
                self.config = self
                    .surface
                    .get_default_config(&self.adapter, self.config.width, self.config.height)
                    .ok_or("Recreated surface has no supported configuration")?;
                self.config.present_mode = wgpu::PresentMode::Fifo;
                self.config.desired_maximum_frame_latency = 2;
                if self.config.format != previous_format
                    && let Some(registry) = self.registry.clone()
                {
                    // Pipelines depend on the surface format; column buffers
                    // and the camera carry over to the rebuilt scene.
                    let mut scene = Scene::from_registry(
                        &self.device,
                        &self.queue,
                        &self.config,
                        &registry,
                        !self.world_view,
                        self.view_distance,
                    )?;
                    if let Some(old) = self.scene.take() {
                        scene.adopt(old);
                    }
                    scene.set_camera(&self.queue, &self.config, self.pose);
                    self.scene = Some(scene);
                }
                self.resize(self.window.inner_size());
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("Surface validation failed".into());
            }
        };
        let texture = if let Some(frame) = &frame {
            &frame.texture
        } else {
            if self.offscreen_frames == 0 {
                eprintln!("SMOKE_OFFSCREEN_ACTIVE surface_occluded=true");
            }
            self.offscreen_frames += 1;
            self.offscreen.get_or_insert_with(|| {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Occluded smoke target"),
                    size: wgpu::Extent3d {
                        width: self.config.width,
                        height: self.config.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.config.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
            });
            self.offscreen
                .as_ref()
                .expect("smoke target was initialized")
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Native frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Native surface clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: SKY[0],
                            g: SKY[1],
                            b: SKY[2],
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: self.scene.as_ref().map(|scene| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view: &scene.depth,
                        depth_ops: Some(wgpu::Operations {
                            // Reversed-Z: far is zero.
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }
                }),
                ..Default::default()
            });
            if let Some(scene) = &self.scene {
                scene.draw(&mut pass, &self.config);
            }
        }
        self.queue.submit([encoder.finish()]);
        if let Some(path) = capture {
            self.capture(texture, path)?;
        }
        if let Some(frame) = frame {
            self.window.pre_present_notify();
            self.queue.present(frame);
        }
        if reconfigure {
            self.resize(self.window.inner_size());
        }
        Ok(true)
    }
}
