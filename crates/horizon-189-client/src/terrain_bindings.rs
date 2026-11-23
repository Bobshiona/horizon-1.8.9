//! Capability-selected terrain texture pages. The Metal HAL encodes binding
//! arrays as persistent argument buffers; older adapters keep the array path.
//! Pages bound residency bookkeeping to a few resources, not one per sprite.
use std::num::NonZeroU32;

pub const PAGE_LAYERS: u32 = 128;
pub const MAX_PAGES: u32 = super::terrain::TerrainVertex::LAYERS / PAGE_LAYERS;

fn array_features() -> wgpu::Features {
    wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
}

pub fn requested_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    let features = array_features();
    if adapter.features().contains(features)
        && adapter.limits().max_binding_array_elements_per_shader_stage >= MAX_PAGES
    {
        features
    } else {
        wgpu::Features::empty()
    }
}

pub fn layout(device: &wgpu::Device, layers: u32) -> Option<wgpu::BindGroupLayout> {
    let count = layers.div_ceil(PAGE_LAYERS);
    if !device.features().contains(array_features())
        || count == 0
        || count > device.limits().max_binding_array_elements_per_shader_stage
    {
        return None;
    }
    Some(
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Persistent terrain argument table"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: NonZeroU32::new(count),
            }],
        }),
    )
}

pub fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
    layers: u32,
) -> wgpu::BindGroup {
    let views: Vec<_> = (0..layers)
        .step_by(PAGE_LAYERS as usize)
        .map(|first| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("Terrain argument page"),
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                base_array_layer: first,
                array_layer_count: Some(PAGE_LAYERS.min(layers - first)),
                ..Default::default()
            })
        })
        .collect();
    let views: Vec<_> = views.iter().collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Persistent terrain argument resources"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureViewArray(&views),
        }],
    })
}

pub fn shader_source(bindless: bool) -> String {
    let sampling = if bindless {
        include_str!("terrain_bindless.wgsl")
    } else {
        include_str!("terrain_array.wgsl")
    };
    let preamble = if bindless {
        "enable wgpu_binding_array;\n"
    } else {
        ""
    };
    [preamble, include_str!("scene.wgsl"), sampling].concat()
}
