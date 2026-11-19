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
}

@vertex
fn vertex_main(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) layer: u32, @location(3) color: vec3<f32>) -> VertexOut {
    var out: VertexOut;
    out.position = camera.view_projection * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.layer = layer;
    out.color = color;
    out.relative = position - camera.eye.xyz;
    return out;
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
    let lit = color.rgb * srgb_to_linear(in.color);
    // Linear distance fog from 3/4 of the view distance, as vanilla's
    // terrain fog, so loading column edges fade instead of popping.
    let distance = length(in.relative);
    let fog = clamp((distance - camera.fog.w) / max(camera.eye.w - camera.fog.w, 1.0), 0.0, 1.0);
    return vec4<f32>(mix(lit, camera.fog.rgb, fog), color.a);
}
