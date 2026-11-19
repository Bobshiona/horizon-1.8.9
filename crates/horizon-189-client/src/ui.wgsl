// Interface pass: GUI pixels to clip space, gamma-space colour times texel.
struct Screen {
    // xy: clip units per GUI pixel; zw: atlas and entity layer counts.
    scale: vec4<f32>,
}
@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var atlas: texture_2d_array<f32>;
@group(0) @binding(2) var entity: texture_2d_array<f32>;
@group(0) @binding(3) var pixels: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) color: vec4<f32>,
}

@vertex
fn vertex_main(@location(0) position: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) layer: u32, @location(3) color: vec4<f32>) -> VertexOut {
    var out: VertexOut;
    out.position = vec4<f32>(position.x * screen.scale.x - 1.0, 1.0 - position.y * screen.scale.y, 0.0, 1.0);
    out.uv = uv;
    out.layer = layer;
    out.color = color;
    return out;
}

// The entity array is sRGB: its texels arrive linear and go back to gamma.
fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

@fragment
fn fragment_main(in: VertexOut) -> @location(0) vec4<f32> {
    var color = in.color;
    if in.layer != 0xffffffffu {
        let index = in.layer & 0x7fffffffu;
        let a = textureSampleLevel(atlas, pixels, in.uv, min(index, u32(screen.scale.z) - 1u), 0.0);
        let e = textureSampleLevel(entity, pixels, in.uv, min(index, u32(screen.scale.w) - 1u), 0.0);
        let texel = select(a, vec4<f32>(linear_to_srgb(e.rgb), e.a), (in.layer & 0x80000000u) != 0u);
        color = color * texel;
    }
    // The GUI's alpha test, GL_GREATER 0.1.
    if color.a <= 0.1 {
        discard;
    }
    return color;
}
