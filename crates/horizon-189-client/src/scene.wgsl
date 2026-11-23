struct Camera {
    view_projection: mat4x4<f32>,
    // xyz: eye relative to the mesh origin; w: fog end distance.
    eye: vec4<f32>,
    // rgb: linear fog colour; w: fog start distance.
    fog: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var block_texture: texture_2d_array<f32>;
@group(0) @binding(2) var block_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) color: vec3<f32>,
    @location(3) relative: vec3<f32>,
    @location(4) hurt: f32,
}

@vertex
fn vertex_main(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) layer: u32, @location(3) color: vec3<f32>, @location(4) hurt: f32) -> VertexOut {
    var out: VertexOut;
    out.position = camera.view_projection * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.layer = layer;
    out.color = color;
    out.hurt = hurt;
    out.relative = position - camera.eye.xyz;
    return out;
}

struct TerrainOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    // Linear-light multiplier: the gamma vertex colour decoded once here.
    @location(2) color: vec3<f32>,
    @location(3) relative: vec3<f32>,
}

// Three words shared with TerrainVertex: positions in 1/64 blocks,
// tiled UVs in 1/128 sprites, an 11-bit texture layer and three colour bytes.
struct TerrainDecoded {
    position: vec3<f32>,
    uv: vec2<f32>,
    layer: u32,
    color: vec3<f32>,
}
fn decode_terrain(words: vec3<u32>) -> TerrainDecoded {
    var out: TerrainDecoded;
    let xyz = vec3<u32>(words.x & 2047u, words.y & 32767u, (words.x >> 11u) & 2047u);
    out.position = vec3<f32>(xyz) / 64.0 - 8.0;
    let uv = vec2<u32>((words.y >> 16u) & 4095u, (words.y >> 28u) | ((words.z & 255u) << 4u));
    out.uv = vec2<f32>(uv) / 128.0;
    out.layer = (words.x >> 22u) | (((words.y >> 15u) & 1u) << 10u);
    out.color = vec3<f32>(vec3<u32>((words.z >> 8u) & 255u, (words.z >> 16u) & 255u, words.z >> 24u)) / 255.0;
    return out;
}
@vertex
fn terrain_vertex(@location(0) words: vec3<u32>, @location(3) base: vec3<f32>) -> TerrainOut {
    let decoded = decode_terrain(words);
    var out: TerrainOut;
    let position = decoded.position + base;
    out.position = camera.view_projection * vec4<f32>(position, 1.0);
    out.uv = decoded.uv;
    out.layer = decoded.layer;
    out.color = srgb_to_linear(decoded.color);
    out.relative = position - camera.eye.xyz;
    return out;
}

// Terrain has no hurt overlay, so vanilla's gamma-space product reduces to
// the linear texel times the decoded vertex colour.
fn terrain_shade(in: TerrainOut, texel: vec4<f32>) -> vec4<f32> {
    let lit = texel.rgb * in.color;
    let distance = length(in.relative);
    let fog = clamp((distance - camera.fog.w) / max(camera.eye.w - camera.fog.w, 1.0), 0.0, 1.0);
    return vec4<f32>(mix(lit, camera.fog.rgb, fog), texel.a);
}

// Sprites with no texel under the 0.1 alpha test: no discard, so the GPU can
// reject hidden fragments before shading.
@fragment
fn terrain_solid(in: TerrainOut) -> @location(0) vec4<f32> {
    return terrain_shade(in, textureSample(block_texture, block_sampler, in.uv, in.layer));
}

@fragment
fn terrain_cutout(in: TerrainOut) -> @location(0) vec4<f32> {
    let texel = textureSample(block_texture, block_sampler, in.uv, in.layer);
    if texel.a < 0.1 { discard; }
    return terrain_shade(in, texel);
}

// Vanilla multiplies tint, face shade and light in gamma space. The sRGB
// texture view decodes texels to linear, so the multiplier is decoded too.
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment
fn fragment_main(in: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSample(block_texture, block_sampler, in.uv, in.layer);
    if color.a < 0.1 { discard; }
    // Legacy hurt overlay blends in gamma space before lighting.
    let gamma = select(1.055 * pow(color.rgb, vec3<f32>(1.0 / 2.4)) - 0.055, color.rgb * 12.92, color.rgb <= vec3<f32>(0.0031308));
    let tinted = mix(gamma, vec3<f32>(1.0, 0.0, 0.0), in.hurt);
    let lit = srgb_to_linear(tinted) * srgb_to_linear(in.color);
    // Linear distance fog from 3/4 of the view distance, as vanilla's
    // terrain fog, so loading column edges fade instead of popping.
    let distance = length(in.relative);
    let fog = clamp((distance - camera.fog.w) / max(camera.eye.w - camera.fog.w, 1.0), 0.0, 1.0);
    return vec4<f32>(mix(lit, camera.fog.rgb, fog), color.a);
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

// Name labels: font glyphs or an untextured quad (layer 0xffffffff), with
// the vertex `hurt` slot carrying alpha. Drawn to a non-sRGB view so
// translucency blends in gamma space as vanilla does.
@fragment
fn label_fragment(in: VertexOut) -> @location(0) vec4<f32> {
    var texel = vec4<f32>(1.0);
    if in.layer != 0xffffffffu {
        let sampled = textureSampleLevel(block_texture, block_sampler, in.uv, in.layer, 0.0);
        if sampled.a < 0.1 { discard; }
        texel = vec4<f32>(linear_to_srgb(sampled.rgb), sampled.a);
    }
    return vec4<f32>(texel.rgb * in.color, texel.a * in.hurt);
}

// `RenderItem.renderEffect` and `LayerArmorBase.renderGlint`: the glint
// texture tiled (`hurt` is its share of the layer), times the pass colour,
// added where the entity geometry lies (depth equal).
fn glint(in: VertexOut) -> vec3<f32> {
    let uv = fract(in.uv) * in.hurt;
    let sampled = textureSampleLevel(block_texture, block_sampler, uv, in.layer, 0.0);
    return linear_to_srgb(sampled.rgb) * in.color;
}

@fragment
fn glint_gamma(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(glint(in), 1.0);
}

@fragment
fn glint_linear(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(srgb_to_linear(glint(in)), 1.0);
}

// `drawBlockDamageTexture`: the crack texture, alpha-tested at 0.1, in
// gamma space for `GL_DST_COLOR, GL_SRC_COLOR` (twice texel times target).
@fragment
fn crack_fragment(in: VertexOut) -> @location(0) vec4<f32> {
    let sampled = textureSampleLevel(block_texture, block_sampler, in.uv, in.layer, 0.0);
    if sampled.a <= 0.1 {
        discard;
    }
    return vec4<f32>(linear_to_srgb(sampled.rgb), 1.0);
}
