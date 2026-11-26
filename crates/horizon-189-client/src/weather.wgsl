// Rain and snow (weather.rs): camera-relative quads, blended, depth-tested.
struct Weather {
    matrix: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> weather: Weather;
@group(0) @binding(1) var weather_texture: texture_2d_array<f32>;
@group(0) @binding(2) var weather_sampler: sampler;

struct WeatherOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) color: vec4<f32>,
}

@vertex
fn weather_vertex(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) layer: u32,
    @location(3) color: vec4<f32>,
) -> WeatherOut {
    var out: WeatherOut;
    out.position = weather.matrix * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.layer = layer;
    out.color = color;
    return out;
}

@fragment
fn weather_fragment(in: WeatherOut) -> @location(0) vec4<f32> {
    let c = in.color * textureSample(weather_texture, weather_sampler, in.uv, in.layer);
    // alphaFunc(GL_GREATER, 0.1)
    if c.a <= 0.1 {
        discard;
    }
    return c;
}
