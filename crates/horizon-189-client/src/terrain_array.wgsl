fn sample_terrain(uv: vec2<f32>, layer: u32) -> vec4<f32> {
    return textureSample(block_texture, block_sampler, uv, i32(layer));
}
