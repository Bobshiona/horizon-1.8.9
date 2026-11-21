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

// `RenderItem.renderEffect` on an icon (layer flag 0x40000000): colour
// carries the pass's scroll, angle and glint layer. The icon's alpha masks
// it, as the depth test does in vanilla, and it adds src * src colour.
fn glint(in: VertexOut) -> vec4<f32> {
    let index = in.layer & 0x3fffffffu;
    let icon = textureSampleLevel(entity, pixels, in.uv, min(index, u32(screen.scale.w) - 1u), 0.0);
    if icon.a <= 0.1 {
        discard;
    }
    let size = f32(textureDimensions(entity).x);
    let pixel = in.uv * size;
    let atlas = vec2<f32>(pixel.x / 1024.0, pixel.y / 512.0);
    let c = cos(in.color.y);
    let s = sin(in.color.y);
    let g = vec2<f32>(atlas.x * c - atlas.y * s + in.color.x, atlas.x * s + atlas.y * c) * 8.0;
    let sampled = textureSampleLevel(entity, pixels, fract(g) * (64.0 / size), u32(in.color.z), 0.0);
    let rgb = linear_to_srgb(sampled.rgb) * vec3<f32>(128.0 / 255.0, 64.0 / 255.0, 204.0 / 255.0);
    // Premultiplied blending with zero alpha: the colour is added.
    return vec4<f32>(rgb * rgb, 0.0);
}

@fragment
fn fragment_main(in: VertexOut) -> @location(0) vec4<f32> {
    if in.layer != 0xffffffffu && (in.layer & 0x40000000u) != 0u {
        return glint(in);
    }
    var color = in.color;
    // 0x20000000: system-font coverage, no alpha test.
    let coverage_text = in.layer != 0xffffffffu && (in.layer & 0xa0000000u) == 0x20000000u;
    if in.layer != 0xffffffffu {
        let index = select(in.layer & 0x7fffffffu, in.layer & 0x1fffffffu, coverage_text);
        let a = textureSampleLevel(atlas, pixels, in.uv, min(index, u32(screen.scale.z) - 1u), 0.0);
        let e = textureSampleLevel(entity, pixels, in.uv, min(index, u32(screen.scale.w) - 1u), 0.0);
        let texel = select(a, vec4<f32>(linear_to_srgb(e.rgb), e.a), (in.layer & 0x80000000u) != 0u);
        color = color * texel;
    }
    // The GUI's alpha test, GL_GREATER 0.1.
    if color.a <= select(0.1, 0.0, coverage_text) {
        discard;
    }
    return vec4<f32>(color.rgb * color.a, color.a);
}
