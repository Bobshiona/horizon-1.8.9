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
    /// See-through and depth-tested label vertices, reused between frames.
    label_vertices: [Vec<crate::terrain::Vertex>; 5],
    ui: Option<crate::ui::Pass>,
    glass: crate::glass::GlassPass,
    pub layout: crate::ui::Layout,
    gui_scale: u32,
    fov: f32,
    /// Name tags: off while F1 hides the GUI (`Minecraft.isGuiEnabled`).
    pub names: bool,
    /// F5: the local player's model, drawn with the others.
    pub own_body: Option<(i32, horizon_world_189::entities::Entity)>,
    /// F3's `E:`: entities in view this frame.
    pub entities_drawn: usize,
    /// F3+B.
    pub hitboxes: bool,
    /// F2: read this frame back into `shot`.
    pub screenshot: bool,
    pub shot: Option<Result<crate::screenshot::Pixels, String>>,
    offscreen: Option<wgpu::Texture>,
    pub offscreen_frames: u64,
    /// `--bench`: wait for each frame's GPU work and keep its duration.
    pub bench: bool,
    pub gpu_time: std::time::Duration,
    /// Time this frame spent waiting for the display to hand back a
    /// drawable: pacing, not work.
    pub acquire_wait: std::time::Duration,
    /// When the last acquire returned: a vsync, if it blocked.
    pub acquired_at: std::time::Instant,
    skin_loader: crate::skins::Loader,
    skins: std::collections::HashMap<u128, crate::skins::Binding>,
    skin_images: std::collections::HashMap<u128, crate::skins::Loaded>,
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        assets: Option<PathBuf>,
        world_view: bool,
        view_distance: u8,
        gui_scale: u32,
        fov: f32,
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
        let terrain_features = crate::terrain_bindings::requested_features(&adapter);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Horizon 1.8.9 Metal"),
                required_features: terrain_features,
                // The legacy terrain atlas is one array layer per sprite
                // (361 in 1.8.9); Metal devices allow 2048.
                required_limits: wgpu::Limits {
                    max_texture_array_layers: adapter.limits().max_texture_array_layers,
                    max_binding_array_elements_per_shader_stage: if terrain_features.is_empty() {
                        0
                    } else {
                        crate::terrain_bindings::MAX_PAGES
                    },
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
        config.desired_maximum_frame_latency = 1;
        Self::add_interface_view(&mut config);
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
        let ui = Self::interface(
            &device,
            &queue,
            &config,
            registry.as_deref(),
            scene.as_ref(),
        );
        let glass = crate::glass::GlassPass::new(&device, config.format);
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
            label_vertices: Default::default(),
            ui,
            glass,
            layout: crate::ui::Layout::new((size.width, size.height), 1.0, gui_scale),
            gui_scale,
            fov,
            offscreen: None,
            offscreen_frames: 0,
            names: true,
            own_body: None,
            entities_drawn: 0,
            hitboxes: false,
            screenshot: false,
            shot: None,
            bench: false,
            gpu_time: std::time::Duration::ZERO,
            acquire_wait: std::time::Duration::ZERO,
            acquired_at: std::time::Instant::now(),
            skin_loader: crate::skins::Loader::new()?,
            skins: Default::default(),
            skin_images: Default::default(),
        };
        renderer.resize(size);
        Ok(renderer)
    }

    /// `GuiInventory.drawEntityOnScreen`: `entity` (the local player)
    /// at interface point `at`, thirty interface pixels to a block, turned
    /// toward the mouse; `None` hides it.
    pub fn set_inventory_model(
        &mut self,
        entity: Option<&horizon_world_189::entities::Entity>,
        at: (f32, f32),
        mouse: (f32, f32),
    ) -> Result<(), Box<dyn Error>> {
        let Some(scene) = &mut self.scene else {
            return Ok(());
        };
        let mut vertices = Vec::new();
        let mut clip = glam::Mat4::IDENTITY;
        if let Some(entity) = entity {
            let (look_x, look_y) = (at.0 - mouse.0, at.1 - 50.0 - mouse.1);
            let mut posed = entity.clone();
            let turn = (look_x / 40.0).atan();
            posed.body_yaw = turn * 20.0;
            posed.previous_body_yaw = posed.body_yaw;
            posed.yaw = turn * 40.0;
            posed.head_yaw = posed.yaw;
            posed.previous_head_yaw = posed.yaw;
            posed.pitch = -(look_y / 40.0).atan() * 20.0;
            posed.previous_pitch = posed.pitch;
            let mut glint = Vec::new();
            crate::models::entities(
                &mut vertices,
                &[(0, posed)],
                1.0,
                [0; 3],
                self.registry.as_deref(),
                Some(&self.skins),
                PositionLook::default(),
                &mut glint,
                crate::models::millis(),
                None,
                &|_| true,
            );
            clip = crate::models::inventory_model_matrix(
                at,
                look_y,
                (self.layout.width, self.layout.height),
            );
        }
        scene.set_inventory_model(&self.device, &self.queue, &vertices, clip)
    }

    pub fn set_view_distance(&mut self, chunks: u8) {
        self.view_distance = chunks;
        if let Some(scene) = &mut self.scene {
            scene.set_view_distance(&self.queue, &self.config, chunks);
        }
    }

    /// Vanilla `guiScale`; the interface layout follows at once.
    pub fn set_gui_scale(&mut self, scale: u32) {
        if self.gui_scale != scale {
            self.gui_scale = scale;
            self.resize(self.window.inner_size());
        }
    }

    /// The world camera's vertical FOV (the zoom mod narrows it).
    pub fn set_fov(&mut self, fov: f32) {
        if fov.is_finite() && (fov - self.fov).abs() > 1e-4 {
            self.fov = fov.clamp(1.0, 170.0);
            if let Some(scene) = &mut self.scene {
                scene.set_camera(&self.queue, &self.config, self.pose, self.fov);
            }
        }
    }

    /// Vsync on (FIFO), or immediate presentation for uncapped frame rates.
    pub fn set_vsync(&mut self, vsync: bool) {
        let mode = if vsync
            || !self
                .surface
                .get_capabilities(&self.adapter)
                .present_modes
                .contains(&wgpu::PresentMode::Immediate)
        {
            wgpu::PresentMode::Fifo
        } else {
            wgpu::PresentMode::Immediate
        };
        if self.config.present_mode != mode {
            self.config.present_mode = mode;
            if self.drawable {
                self.surface.configure(&self.device, &self.config);
            }
            eprintln!("present_mode={mode:?}");
        }
    }

    /// The interface draws in gamma space through a non-sRGB view.
    fn add_interface_view(config: &mut wgpu::SurfaceConfiguration) {
        let plain = config.format.remove_srgb_suffix();
        config.view_formats = if plain == config.format {
            Vec::new()
        } else {
            vec![plain]
        };
    }

    fn interface(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        registry: Option<&Registry>,
        scene: Option<&Scene>,
    ) -> Option<crate::ui::Pass> {
        Some(crate::ui::Pass::new(
            device,
            queue,
            config.format.remove_srgb_suffix(),
            &registry?.gui,
            scene?.entity_texture(),
        ))
    }

    /// Build and upload this frame's interface.
    pub fn draw_ui(
        &mut self,
        tick: f64,
        draw: impl FnOnce(&mut crate::ui::Canvas<'_>, &Registry, &dyn Fn(u128) -> Option<u32>),
    ) -> Result<(), Box<dyn Error>> {
        let (Some(registry), Some(pass), Some(scene)) =
            (self.registry.clone(), self.ui.as_mut(), self.scene.as_ref())
        else {
            return Ok(());
        };
        let mut canvas =
            crate::ui::Canvas::new(&registry.gui, self.layout, tick as u64, &pass.pages);
        let skins = &self.skins;
        let skin = |uuid: u128| skins.get(&uuid).map(|binding| binding.skin);
        draw(&mut canvas, &registry, &skin);
        let (glass, split) = canvas.take_glass();
        let (vertices, inverted, missing) = canvas.finish();
        pass.upload(
            &self.device,
            &self.queue,
            self.layout,
            scene.entity_layers(),
            &inverted,
            &vertices,
            split,
        )?;
        self.glass.upload(
            &self.device,
            &self.queue,
            self.layout.factor,
            (self.config.width, self.config.height),
            [&glass[0], &glass[1]],
        )?;
        pass.load_pages(&self.queue, &registry.gui, &missing);
        pass.upload_glyphs(&self.queue);
        Ok(())
    }

    /// Upload one rebuilt column from the mesher thread.
    pub fn apply_mesh(&mut self, update: MeshUpdate) -> Result<(), Box<dyn Error>> {
        let scene = self.scene.as_mut().ok_or("World update requires a scene")?;
        let origin_changed = update.origin != scene.origin();
        scene.apply(&self.device, &self.queue, update)?;
        if origin_changed {
            scene.set_camera(&self.queue, &self.config, self.pose, self.fov);
        }
        Ok(())
    }

    /// Per-frame eye position, interpolated between simulation ticks.
    pub fn set_pose(&mut self, pose: PositionLook) {
        self.pose = pose;
        if let Some(scene) = &mut self.scene {
            scene.set_camera(&self.queue, &self.config, pose, self.fov);
        }
    }

    /// F3: the drawable's size, the GPU's name, sections drawn and the
    /// render distance.
    pub fn debug_facts(&self) -> ((u32, u32), String, u32, u8) {
        (
            (self.config.width, self.config.height),
            self.adapter.get_info().name,
            self.scene.as_ref().map_or(0, Scene::drawn_sections),
            self.view_distance,
        )
    }

    /// Rebuild moving-entity geometry at this frame's partial tick.
    pub fn set_entities(
        &mut self,
        entities: &[(i32, horizon_world_189::entities::Entity)],
        partial: f32,
        board: &crate::scoreboard::Scoreboard,
        own_name: &str,
        own: Option<&(u128, Arc<horizon_protocol_47::player_list::Profile>)>,
        angler: Option<&crate::models::Angler>,
        cracks: &[([i32; 3], horizon_world_189::BlockState, u8)],
        extra: &[crate::terrain::Vertex],
        outline: Option<([f64; 3], [f64; 3], crate::outline::Style)>,
    ) -> Result<(), Box<dyn Error>> {
        let Some(scene) = &mut self.scene else {
            return Ok(());
        };
        let own_uuid = own.map(|(uuid, _)| *uuid);
        self.skins.retain(|uuid, _| {
            Some(*uuid) == own_uuid || entities.iter().any(|(_, e)| e.uuid == *uuid)
        });
        self.skin_images
            .retain(|uuid, _| self.skins.contains_key(uuid));
        self.skin_loader
            .retain(entities.iter().map(|(_, e)| e.uuid).chain(own_uuid));
        for (_, entity) in entities {
            if let Some(profile) = &entity.profile {
                self.skin_loader.request(entity.uuid, profile);
            }
        }
        if let Some((uuid, profile)) = own {
            self.skin_loader.request(*uuid, profile);
        }
        while let Ok(result) = self.skin_loader.results.try_recv() {
            match result {
                Ok(loaded) => {
                    if Some(loaded.uuid) != own_uuid
                        && !entities.iter().any(|(_, e)| e.uuid == loaded.uuid)
                    {
                        continue;
                    }
                    let Some(layer) = self.skins.get(&loaded.uuid).map(|b| b.skin).or_else(|| {
                        (scene.skin_base..scene.skin_base + 256)
                            .step_by(2)
                            .find(|layer| self.skins.values().all(|b| b.skin != *layer))
                    }) else {
                        continue;
                    };
                    if scene.upload_skin(&self.queue, layer, &loaded) {
                        self.skins.insert(
                            loaded.uuid,
                            crate::skins::Binding {
                                skin: layer,
                                cape: loaded.cape.as_ref().map(|_| layer + 1),
                                slim: loaded.slim,
                            },
                        );
                        eprintln!(
                            "SKIN_LOADED cape={} slim={}",
                            loaded.cape.is_some(),
                            loaded.slim
                        );
                        self.skin_images.insert(loaded.uuid, loaded);
                    }
                }
                Err((uuid, error)) => {
                    self.skin_loader.failed(uuid);
                    eprintln!("SKIN_LOAD_FAILED {error}");
                }
            }
        }
        let mut vertices = std::mem::take(&mut self.entity_vertices);
        vertices.clear();
        let [mut crack, mut glint, mut through, mut tested, mut lines] =
            std::mem::take(&mut self.label_vertices);
        crack.clear();
        glint.clear();
        if let Some(registry) = &self.registry {
            let origin = scene.origin();
            for &(at, state, stage) in cracks {
                let Some(layer) = registry.sprite(crate::terrain::DESTROY_STAGES[stage as usize])
                else {
                    continue;
                };
                let centre = glam::Vec3::new(
                    (i64::from(at[0]) - origin[0]) as f32 + 0.5,
                    (i64::from(at[1]) - origin[1]) as f32 + 0.5,
                    (i64::from(at[2]) - origin[2]) as f32 + 0.5,
                );
                let start = crack.len();
                registry.block_model(&mut crack, state, glam::Mat4::from_translation(centre), 1.0);
                for vertex in &mut crack[start..] {
                    vertex.layer = layer;
                    vertex.color = [1.0; 3];
                }
            }
        }
        let planes = scene.planes(&self.config);
        let origin = scene.origin();
        let in_view = |entity: &horizon_world_189::entities::Entity| {
            crate::scene::entity_in_view(&planes, entity, partial, origin)
        };
        // F5's own model draws with the others but never gets a name tag.
        let with_own;
        let drawn = match &self.own_body {
            Some(own) => {
                with_own = entities
                    .iter()
                    .cloned()
                    .chain(std::iter::once(own.clone()))
                    .collect::<Vec<_>>();
                &with_own[..]
            }
            None => entities,
        };
        self.entities_drawn = drawn.iter().filter(|(_, e)| in_view(e)).count();
        crate::models::entities(
            &mut vertices,
            drawn,
            partial,
            scene.origin(),
            self.registry.as_deref(),
            Some(&self.skins),
            self.pose,
            &mut glint,
            crate::models::millis(),
            angler,
            &in_view,
        );
        vertices.extend_from_slice(extra);
        through.clear();
        tested.clear();
        lines.clear();
        crate::models::fishing_lines(&mut lines, drawn, partial, scene.origin(), angler);
        if self.names
            && let Some(registry) = &self.registry
        {
            crate::models::nametags(
                &mut through,
                &mut tested,
                entities,
                partial,
                scene.origin(),
                self.pose,
                &registry.glyphs,
                board,
                own_name,
                &in_view,
            );
        }
        let origin = scene.origin();
        let local = |p: [f64; 3]| {
            glam::Vec3::from_array(std::array::from_fn(|axis| {
                (p[axis] - origin[axis] as f64) as f32
            }))
        };
        let eye = local(self.pose.position);
        // One point's world size a block away, at the vertical FOV.
        let points = self.config.height as f32 / self.window.scale_factor() as f32;
        let pixel = 2.0 * (self.fov.to_radians() * 0.5).tan() / points.max(1.0);
        if self.hitboxes {
            for (_, entity) in drawn.iter().filter(|(_, e)| !e.invisible() && in_view(e)) {
                let Some(size) = entity.dimensions() else {
                    continue;
                };
                let feet: [f64; 3] = std::array::from_fn(|axis| {
                    entity.previous[axis]
                        + (entity.position[axis] - entity.previous[axis]) * f64::from(partial)
                });
                crate::outline::hitbox(
                    &mut tested,
                    local(feet),
                    size,
                    debug_eye_height(entity, size.1),
                    debug_look(entity, partial),
                    eye,
                    pixel,
                );
            }
        }
        if let Some((min, max, style)) = outline {
            crate::outline::outline(&mut tested, local(min), local(max), eye, pixel, style);
        }
        let result = scene
            .set_entities(&self.device, &self.queue, &vertices)
            .and_then(|()| {
                scene.set_labels(
                    &self.device,
                    &self.queue,
                    [&crack, &glint, &through, &tested, &lines],
                )
            });
        self.entity_vertices = vertices;
        self.label_vertices = [crack, glint, through, tested, lines];
        result
    }

    /// The mesh origin entity positions are drawn relative to.
    pub fn origin(&self) -> Option<[i64; 3]> {
        self.scene.as_ref().map(Scene::origin)
    }

    /// A loaded skin's layer and model.
    pub fn skin_of(&self, uuid: u128) -> Option<(u32, bool)> {
        self.skins
            .get(&uuid)
            .map(|binding| (binding.skin, binding.slim))
    }

    /// `hurtCameraEffect` and view bobbing for the world camera.
    pub fn set_camera_effects(&mut self, effects: glam::Mat4) {
        if let Some(scene) = &mut self.scene {
            scene.set_effects(effects);
        }
    }

    /// Rebuild the first-person hand.
    pub fn set_hand(
        &mut self,
        frame: Option<&crate::hand::Frame<'_>>,
    ) -> Result<(), Box<dyn Error>> {
        let (Some(scene), Some(registry)) = (&mut self.scene, &self.registry) else {
            return Ok(());
        };
        let (mut vertices, mut glint) = (Vec::new(), Vec::new());
        if let Some(frame) = frame {
            crate::hand::draw(&mut vertices, &mut glint, frame, registry);
        }
        scene.set_hand(&self.device, &self.queue, &self.config, &vertices, &glint)
    }

    pub fn clear_world(&mut self) {
        self.entity_vertices.clear();
        for list in &mut self.label_vertices {
            list.clear();
        }
        self.pose = PositionLook::default();
        self.skins.clear();
        self.skin_images.clear();
        self.skin_loader.retain(std::iter::empty());
        if let Some(scene) = &mut self.scene {
            scene.clear_world();
            scene.set_camera(&self.queue, &self.config, self.pose, self.fov);
        }
    }

    /// World draw calls since the last call.
    pub fn take_draws(&self) -> u32 {
        self.scene.as_ref().map_or(0, Scene::take_draws)
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
            self.layout = crate::ui::Layout::new(
                (size.width, size.height),
                self.window.scale_factor(),
                self.gui_scale,
            );
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

    /// Read the presented frame back as tightly packed RGBA.
    fn read_frame(
        &self,
        texture: &wgpu::Texture,
    ) -> Result<crate::screenshot::Pixels, Box<dyn Error>> {
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
        Ok(crate::screenshot::Pixels {
            width,
            height,
            rgba,
        })
    }

    /// Read the presented frame back and write it as an sRGB PNG.
    fn capture(
        &self,
        texture: &wgpu::Texture,
        path: &std::path::Path,
    ) -> Result<(), Box<dyn Error>> {
        let pixels = self.read_frame(texture)?;
        crate::screenshot::write_png(path, &pixels, false)?;
        eprintln!(
            "FRAME_CAPTURED path={} size={}x{}",
            path.display(),
            pixels.width,
            pixels.height
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
        // `--bench` renders offscreen so the compositor's pacing never
        // enters the GPU timing.
        let waited = std::time::Instant::now();
        let acquired = if self.bench {
            wgpu::CurrentSurfaceTexture::Occluded
        } else {
            self.surface.get_current_texture()
        };
        self.acquire_wait = waited.elapsed();
        self.acquired_at = std::time::Instant::now();
        let (frame, reconfigure) = match acquired {
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
                self.config.desired_maximum_frame_latency = 1;
                Self::add_interface_view(&mut self.config);
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
                    scene.set_camera(&self.queue, &self.config, self.pose, self.fov);
                    for (uuid, image) in &self.skin_images {
                        if let Some(binding) = self.skins.get(uuid) {
                            scene.upload_skin(&self.queue, binding.skin, image);
                        }
                    }
                    self.scene = Some(scene);
                    self.ui = Self::interface(
                        &self.device,
                        &self.queue,
                        &self.config,
                        self.registry.as_deref(),
                        self.scene.as_ref(),
                    );
                    self.glass = crate::glass::GlassPass::new(&self.device, self.config.format);
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
                eprintln!("SMOKE_OFFSCREEN_ACTIVE surface_occluded={}", !self.bench);
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
                    view_formats: &self.config.view_formats,
                })
            });
            self.offscreen
                .as_ref()
                .expect("smoke target was initialized")
        };
        if let Some(scene) = &mut self.scene {
            scene.prepare(&self.device, &self.queue, &self.config);
        }
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
                scene.draw(&mut pass);
            }
        }
        let plain = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.format.remove_srgb_suffix()),
            ..Default::default()
        });
        if let Some(scene) = self.scene.as_ref().filter(|scene| scene.has_labels()) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Name labels"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &plain,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &scene.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            scene.draw_labels(&mut pass);
        }
        // Each pass loads and stores the whole frame: skip empty ones.
        if let Some(scene) = self.scene.as_ref().filter(|s| s.has_hand()) {
            // renderHand: depth cleared so the hand never enters walls.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("First-person hand"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &scene.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            scene.draw_hand(&mut pass);
        }
        // Glass under the interface, the interface, then glass under the
        // overlay (which frosts the interface too) and the overlay.
        let copyable = texture.usage().contains(wgpu::TextureUsages::COPY_SRC);
        for part in 0..2 {
            if part == 1
                && let Some(scene) = self.scene.as_ref().filter(|s| s.has_inventory_model())
            {
                // Between the inventory's background and its items.
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Inventory player model"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &scene.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                scene.draw_inventory_model(&mut pass);
            }
            self.glass
                .encode(&mut encoder, part, texture, copyable, &view);
            if let Some(ui) = self.ui.as_ref().filter(|ui| ui.has_part(part)) {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Legacy interface"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &plain,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                ui.draw_part(&mut pass, part);
            }
        }
        let upload = self.scene.as_mut().and_then(Scene::take_upload);
        self.queue
            .submit(upload.into_iter().chain(std::iter::once(encoder.finish())));
        if let Some(scene) = &mut self.scene {
            scene.recall();
        }
        if !self.bench {
            // Completes the staging belt's chunk returns without waiting.
            let _ = self.device.poll(wgpu::PollType::Poll);
        }
        if self.bench {
            // Submit-to-completion: an upper bound on this frame's GPU time.
            let waited = std::time::Instant::now();
            self.device.poll(wgpu::PollType::wait_indefinitely())?;
            self.gpu_time = waited.elapsed();
        }
        if let Some(path) = capture {
            self.capture(texture, path)?;
        }
        if std::mem::take(&mut self.screenshot) {
            self.shot = Some(self.read_frame(texture).map_err(|e| e.to_string()));
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

/// `getEyeHeight` for F3+B's red box, from each class's override in the
/// jar (`Entity`'s default is 0.85 of the height). `None` for entities that
/// aren't living.
fn debug_eye_height(entity: &horizon_world_189::entities::Entity, height: f32) -> Option<f32> {
    use horizon_world_189::entities::Kind;
    // EntityZombie and EntityVillager subtract 0.81 in double precision.
    let child = |eyes: f32| {
        if entity.child {
            (f64::from(eyes) - 0.81) as f32
        } else {
            eyes
        }
    };
    Some(match entity.kind {
        Kind::Player { .. } => {
            if entity.sneaking() {
                1.62 - 0.08
            } else {
                1.62
            }
        }
        // Skeletons; wither skeletons fall back to the default.
        Kind::Mob(51) if entity.variant != 1 => 1.74,
        Kind::Mob(52) => 0.65,
        Kind::Mob(53) => 10.440001,
        Kind::Mob(54 | 57) => child(1.74),
        Kind::Mob(55 | 62) => 0.625 * height,
        Kind::Mob(56) => 2.6,
        Kind::Mob(58) => 2.55,
        Kind::Mob(59) => 0.45,
        Kind::Mob(60 | 67) => 0.1,
        Kind::Mob(65) => height / 2.0,
        Kind::Mob(66) => 1.62,
        Kind::Mob(68 | 94) => height * 0.5,
        Kind::Mob(91) => 0.95 * height,
        Kind::Mob(92 | 93 | 96 | 100) => height,
        Kind::Mob(95) => height * 0.8,
        Kind::Mob(97) => 1.7,
        Kind::Mob(120) => child(1.62),
        Kind::Mob(_) => height * 0.85,
        // EntityArmorStand: half the height when small.
        Kind::Object(78) if entity.stand_flags & 0x01 != 0 => height * 0.5,
        Kind::Object(78) => height * 0.9,
        _ => return None,
    })
}

/// `Entity.getLook(partial)`: living entities look along their head yaw.
fn debug_look(entity: &horizon_world_189::entities::Entity, partial: f32) -> glam::Vec3 {
    use horizon_world_189::entities::Kind;
    let (yaw, pitch) = if matches!(entity.kind, Kind::Player { .. } | Kind::Mob(_)) {
        let lerp = |a: f32, b: f32| a + (b - a) * partial;
        (
            lerp(entity.previous_head_yaw, entity.head_yaw),
            lerp(entity.previous_pitch, entity.pitch),
        )
    } else {
        (entity.yaw, entity.pitch)
    };
    glam::Vec3::from_array(horizon_physics_189::raycast::look(yaw, pitch).map(|v| v as f32))
}

#[cfg(test)]
mod eye_tests {
    use super::debug_eye_height;
    use horizon_protocol_47::{entities::EntityPacket, nbt::Meta};
    use horizon_world_189::entities::Entities;

    #[test]
    fn hitbox_eye_heights_follow_each_get_eye_height() {
        let mut entities = Entities::default();
        let mob = |entities: &mut Entities, id, kind, metadata| {
            entities.apply(EntityPacket::SpawnMob {
                id,
                kind,
                position: [0; 3],
                yaw: 0,
                pitch: 0,
                head_yaw: 0,
                metadata,
            });
            let entity = entities.get(id).unwrap().clone();
            let height = entity.dimensions().map_or(0.0, |d| d.1);
            debug_eye_height(&entity, height)
        };
        assert_eq!(mob(&mut entities, 1, 54, vec![]), Some(1.74));
        assert_eq!(
            mob(&mut entities, 2, 54, vec![(12, Meta::Byte(1))]),
            Some(0.93)
        );
        assert_eq!(mob(&mut entities, 3, 58, vec![]), Some(2.55));
        assert_eq!(mob(&mut entities, 4, 52, vec![]), Some(0.65));
        assert_eq!(
            mob(&mut entities, 5, 51, vec![(13, Meta::Byte(1))]),
            Some(2.34 * 0.85)
        );
        assert_eq!(mob(&mut entities, 6, 50, vec![]), Some(1.7 * 0.85));
        assert_eq!(
            mob(&mut entities, 7, 120, vec![(12, Meta::Byte(-1))]),
            Some(0.81)
        );
        assert_eq!(mob(&mut entities, 8, 92, vec![]), Some(1.3));
        entities.apply(EntityPacket::SpawnObject {
            id: 9,
            kind: 78,
            position: [0; 3],
            pitch: 0,
            yaw: 0,
            data: 0,
            velocity: None,
        });
        let stand = entities.get(9).unwrap();
        assert_eq!(debug_eye_height(stand, 1.975), Some(1.975 * 0.9));
    }
}
