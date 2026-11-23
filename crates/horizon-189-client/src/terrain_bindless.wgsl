// One argument-buffer resource per 128 sprites. Keep the same 11-bit material
// index as the array path; high bits select the page, low bits its array layer.
@group(1) @binding(0) var terrain_pages: binding_array<texture_2d_array<f32>>;
fn sample_terrain(uv: vec2<f32>, layer: u32) -> vec4<f32> {
    return textureSample(terrain_pages[layer >> 7u], block_sampler, uv, i32(layer & 127u));
}
