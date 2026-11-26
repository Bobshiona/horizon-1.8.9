// The overworld sky (sky.rs): drawn at the far plane where nothing else is.
struct Sky {
    // Projection times the view's rotation: sky geometry is eye-relative.
    matrix: mat4x4<f32>,
    // The sun, moon and stars' rotation.
    celestial: mat4x4<f32>,
    // rgb: linear sky colour; w: fog end (the render distance in blocks).
    sky: vec4<f32>,
    // rgb: linear fog colour; w: the lower plane's height offset.
    fog: vec4<f32>,
    // rgb: linear colour of the plane below.
    lower: vec4<f32>,
    // x: star brightness.
    stars: vec4<f32>,
}
@group(0) @binding(0) var<uniform> sky: Sky;
@group(0) @binding(1) var sky_texture: texture_2d_array<f32>;
@group(0) @binding(2) var sky_sampler: sampler;

struct SkyOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) layer: u32,
    // xyz: eye-relative position; w: 1 where linear fog applies.
    @location(3) relative: vec4<f32>,
}

@vertex
fn sky_vertex(
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) layer: u32,
    @location(4) kind: u32,
) -> SkyOut {
    var p = position;
    var c = color;
    var fogged = 0.0;
    if kind == 0u {
        c = vec4<f32>(sky.sky.rgb, 1.0);
        fogged = 1.0;
    } else if kind == 1u {
        p.y = p.y + sky.fog.w;
        c = vec4<f32>(sky.lower.rgb, 1.0);
        fogged = 1.0;
    } else if kind == 2u {
        p = (sky.celestial * vec4<f32>(p, 1.0)).xyz;
        c = vec4<f32>(sky.stars.x);
    }
    var out: SkyOut;
    let clip = sky.matrix * vec4<f32>(p, 1.0);
    // Reversed-Z far plane: only pixels nothing else has drawn pass.
    out.position = vec4<f32>(clip.xy, 0.0, clip.w);
    out.color = c;
    out.uv = uv;
    out.layer = layer;
    out.relative = vec4<f32>(p, fogged);
    return out;
}

@fragment
fn sky_fragment(in: SkyOut) -> @location(0) vec4<f32> {
    let texel = textureSample(sky_texture, sky_sampler, in.uv, in.layer);
    var c = in.color * texel;
    if in.relative.w > 0.5 {
        // setupFog(-1): linear from the eye to the render distance.
        let f = clamp(length(in.relative.xyz) / max(sky.sky.w, 1.0), 0.0, 1.0);
        c = vec4<f32>(mix(c.rgb, sky.fog.rgb, f), c.a);
    }
    return c;
}
