// Frosted "liquid glass" panels: a dual-Kawase blur of the frame behind
// them, refracted at the rim, tinted, with a specular edge and a soft shadow.
struct Params {
    // Blur passes: xy is one source texel in uv. Panels: xy is the target
    // in physical pixels and z physical pixels per GUI pixel.
    value: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var linear: sampler;

struct FullOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex_full(@builtin(vertex_index) index: u32) -> FullOut {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: FullOut;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

@fragment
fn fragment_down(in: FullOut) -> @location(0) vec4<f32> {
    let o = params.value.xy * 0.5;
    var sum = textureSample(source, linear, in.uv) * 4.0;
    sum += textureSample(source, linear, in.uv - o);
    sum += textureSample(source, linear, in.uv + o);
    sum += textureSample(source, linear, in.uv + vec2<f32>(o.x, -o.y));
    sum += textureSample(source, linear, in.uv - vec2<f32>(o.x, -o.y));
    return sum / 8.0;
}

@fragment
fn fragment_up(in: FullOut) -> @location(0) vec4<f32> {
    let o = params.value.xy * 0.5;
    var sum = textureSample(source, linear, in.uv + vec2<f32>(-o.x * 2.0, 0.0));
    sum += textureSample(source, linear, in.uv + vec2<f32>(-o.x, o.y)) * 2.0;
    sum += textureSample(source, linear, in.uv + vec2<f32>(0.0, o.y * 2.0));
    sum += textureSample(source, linear, in.uv + vec2<f32>(o.x, o.y)) * 2.0;
    sum += textureSample(source, linear, in.uv + vec2<f32>(o.x * 2.0, 0.0));
    sum += textureSample(source, linear, in.uv + vec2<f32>(o.x, -o.y)) * 2.0;
    sum += textureSample(source, linear, in.uv + vec2<f32>(0.0, -o.y * 2.0));
    sum += textureSample(source, linear, in.uv + vec2<f32>(-o.x, -o.y)) * 2.0;
    return sum / 12.0;
}

struct PanelOut {
    @builtin(position) position: vec4<f32>,
    // GUI pixels, relative to the panel centre.
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) half_size: vec2<f32>,
    // x: corner radius, y: rim highlight, z: shadow, w: panel opacity.
    @location(2) @interpolate(flat) params: vec4<f32>,
    @location(3) @interpolate(flat) tint: vec4<f32>,
}

@vertex
fn vertex_panel(
    @location(0) position: vec2<f32>,
    @location(1) center: vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) shape: vec4<f32>,
    @location(4) tint: vec4<f32>,
) -> PanelOut {
    let clip = position * params.value.z * 2.0 / params.value.xy;
    var out: PanelOut;
    out.position = vec4<f32>(clip.x - 1.0, 1.0 - clip.y, 0.0, 1.0);
    out.local = position - center;
    out.half_size = half_size;
    out.params = shape;
    out.tint = tint;
    return out;
}

fn rounded_box(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half_size.x, half_size.y));
    let q = abs(p) - half_size + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment
fn fragment_panel(in: PanelOut) -> @location(0) vec4<f32> {
    let pixel = 1.0 / params.value.z;
    let d = rounded_box(in.local, in.half_size, in.params.x);
    if d > 0.0 {
        // Outside: a soft shadow that fades over a few GUI pixels.
        let shadow = in.params.z * exp(-d * 0.45) * (1.0 - smoothstep(0.0, pixel, -d + pixel));
        return vec4<f32>(0.0, 0.0, 0.0, shadow * in.params.w);
    }
    let coverage = clamp(-d / pixel + 0.5, 0.0, 1.0);
    // Light bends at the rim: sample slightly inward, like a thick lens.
    let edge = clamp(1.0 + d / max(in.params.x * 1.5, 1.0), 0.0, 1.0);
    let normal = normalize(in.local / max(in.half_size, vec2<f32>(1.0)) + vec2<f32>(1e-4));
    let bend = normal * edge * edge * 6.0 * params.value.z;
    let uv = (in.position.xy - bend) / params.value.xy;
    var glass = textureSampleLevel(source, linear, uv, 0.0).rgb;
    // Vibrancy: lift saturation and keep the frost bright.
    let luma = dot(glass, vec3<f32>(0.2126, 0.7152, 0.0722));
    glass = mix(vec3<f32>(luma), glass, 1.35) * 1.04 + 0.015;
    let tint = srgb_to_linear(in.tint.rgb);
    var color = mix(glass, tint, in.tint.a);
    // Specular rim: brightest along the top edge, a hairline everywhere.
    let rim = 1.0 - smoothstep(0.0, pixel * 1.6, -d);
    let top = clamp(0.5 - in.local.y / max(in.half_size.y, 1.0) * 0.5, 0.0, 1.0);
    color += vec3<f32>(rim * in.params.y * (0.35 + 0.65 * top));
    // A faint sheen over the upper half.
    color += vec3<f32>(0.04 * in.params.y * smoothstep(0.2, -1.0, in.local.y / max(in.half_size.y, 1.0)));
    let alpha = coverage * in.params.w;
    return vec4<f32>(color * alpha, alpha);
}
