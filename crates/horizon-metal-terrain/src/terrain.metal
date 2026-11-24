#include <metal_stdlib>
#include <metal_command_buffer>
using namespace metal;
struct Camera { float4x4 matrix; float4 eye; float4 fog; };
struct Cull { float4 planes[5]; uint4 count; };
struct Draw { float4 low; float4 high; uint4 args; uint4 page; };
struct Geometry { array<device const packed_uint3 *, 1024> pages [[id(0)]]; };
struct Icb { command_buffer commands [[id(0)]]; };
struct Material { texture2d_array<float> atlas [[id(0)]]; sampler filtering [[id(1)]]; };
struct Out { float4 position [[position]]; float2 uv; uint layer [[flat]]; float3 color; float3 relative; };
float3 linear_color(float3 value) {
    return select(pow((value + 0.055) / 1.055, float3(2.4)), value / 12.92, value <= 0.04045);
}
struct TerrainIn { uint3 words [[attribute(0)]]; float3 base [[attribute(1)]]; };
vertex Out terrain(TerrainIn in [[stage_in]], constant Camera &camera [[buffer(2)]]) {
    uint3 words = in.words;
    uint3 xyz = uint3(words.x & 2047u, words.y & 32767u, (words.x >> 11u) & 2047u);
    float3 position = float3(xyz) / 64.0 - 8.0 + in.base;
    Out out;
    out.position = camera.matrix * float4(position, 1.0);
    out.uv = float2(uint2((words.y >> 16u) & 4095u, (words.y >> 28u) | ((words.z & 255u) << 4u))) / 128.0;
    out.layer = (words.x >> 22u) | (((words.y >> 15u) & 1u) << 10u);
    out.color = linear_color(float3(uint3((words.z >> 8u) & 255u, (words.z >> 16u) & 255u, words.z >> 24u)) / 255.0);
    out.relative = position - camera.eye.xyz;
    return out;
}
float4 shade(Out in, float4 texel, constant Camera &camera) {
    float fog = clamp((length(in.relative) - camera.fog.w) / max(camera.eye.w - camera.fog.w, 1.0), 0.0, 1.0);
    return float4(mix(texel.rgb * in.color, camera.fog.rgb, fog), texel.a);
}
fragment float4 solid(Out in [[stage_in]], constant Material &material [[buffer(0)]], constant Camera &camera [[buffer(1)]]) {
    return shade(in, material.atlas.sample(material.filtering, in.uv, in.layer), camera);
}
fragment float4 cutout(Out in [[stage_in]], constant Material &material [[buffer(0)]], constant Camera &camera [[buffer(1)]]) {
    float4 texel = material.atlas.sample(material.filtering, in.uv, in.layer);
    if (texel.a < 0.1) discard_fragment();
    return shade(in, texel, camera);
}
kernel void cull_and_encode(uint index [[thread_position_in_grid]], constant Cull &cull [[buffer(0)]], device const Draw *draws [[buffer(1)]], device const uint *indices [[buffer(2)]], device Icb &output [[buffer(3)]], device const Geometry &geometry [[buffer(4)]], device const Camera *camera [[buffer(5)]], device const packed_float3 *bases [[buffer(6)]], device const Material *material [[buffer(8)]]) {
    if (index >= cull.count.x) return;
    Draw draw = draws[index];
    bool visible = true;
    for (uint p = 0; p < 5; ++p) {
        float3 corner = select(draw.low.xyz - 0.015625, draw.high.xyz + 0.015625, cull.planes[p].xyz >= 0.0);
        visible = visible && dot(cull.planes[p].xyz, corner) + cull.planes[p].w >= 0.0;
    }
    // Every slot is written each frame, so no separate reset pass is needed.
    render_command command(output.commands, index);
    if (!visible) {
        command.reset();
    } else {
        command.set_vertex_buffer(geometry.pages[draw.page.x], 0);
        command.set_vertex_buffer(bases, 1);
        command.set_vertex_buffer(camera, 2);
        command.set_fragment_buffer(material, 0);
        command.set_fragment_buffer(camera, 1);
        command.draw_indexed_primitives(primitive_type::triangle, draw.args.y, indices + draw.args.x, 1, as_type<int>(draw.args.z), draw.args.w);
    }
}
// Occlusion, run after the nearby solid terrain (the occluders) has been drawn.
// The depth pyramid holds, per texel, the farthest depth of every pixel under it
// (reversed Z: the smallest value); pixels outside the viewport count as far.
kernel void hiz_seed(uint2 gid [[thread_position_in_grid]], depth2d<float, access::read> depth [[texture(0)]], texture2d<float, access::write> level [[texture(1)]], constant uint4 &size [[buffer(0)]]) {
    if (gid.x >= size.z || gid.y >= size.w) return;
    float value = (gid.x < size.x && gid.y < size.y) ? depth.read(gid) : 0.0;
    level.write(float4(value), gid);
}
kernel void hiz_reduce(uint2 gid [[thread_position_in_grid]], texture2d<float, access::read> source [[texture(0)]], texture2d<float, access::write> target [[texture(1)]]) {
    if (gid.x >= target.get_width() || gid.y >= target.get_height()) return;
    uint2 edge = uint2(source.get_width(), source.get_height()) - 1u;
    uint2 at = min(gid * 2u, edge);
    // Rectangular mip chains reach one texel on one axis first. Duplicate
    // that edge instead of reading outside the source texture.
    float value = min(min(source.read(at).x, source.read(min(at + uint2(1u, 0u), edge)).x),
                      min(source.read(min(at + uint2(0u, 1u), edge)).x, source.read(min(at + uint2(1u, 1u), edge)).x));
    target.write(float4(value), gid);
}
struct Occlusion { uint4 range; float4 viewport; };
// A command stays unless its whole box, as projected with this frame's camera,
// lies behind the farthest occluder depth of every pyramid texel it touches. A box
// reaching the eye plane always stays. Commands already culled are reset again,
// which changes nothing.
kernel void occlude(uint index [[thread_position_in_grid]], constant Occlusion &occlusion [[buffer(0)]], device const Draw *draws [[buffer(1)]], device atomic_uint *culled [[buffer(2)]], device Icb &output [[buffer(3)]], constant Camera &camera [[buffer(4)]], texture2d<float, access::read> hiz [[texture(0)]]) {
    uint slot = occlusion.range.x + index;
    if (slot >= occlusion.range.y) return;
    Draw draw = draws[slot];
    float3 low = draw.low.xyz - 0.015625;
    float3 high = draw.high.xyz + 0.015625;
    float2 first = float2(INFINITY);
    float2 last = float2(-INFINITY);
    float nearest = 0.0;
    for (uint corner = 0; corner < 8u; ++corner) {
        float3 at = select(low, high, bool3((corner & 1u) != 0u, (corner & 2u) != 0u, (corner & 4u) != 0u));
        float4 clip = camera.matrix * float4(at, 1.0);
        if (clip.w <= 1.0e-4) return;
        float3 ndc = clip.xyz / clip.w;
        float2 pixel = float2((ndc.x * 0.5 + 0.5) * occlusion.viewport.x, (0.5 - ndc.y * 0.5) * occlusion.viewport.y);
        first = min(first, pixel);
        last = max(last, pixel);
        nearest = max(nearest, ndc.z);
    }
    // One pixel of slack on every side, clamped to the viewport.
    first = max(floor(first) - 1.0, float2(0.0));
    last = min(ceil(last) + 1.0, occlusion.viewport.xy - 1.0);
    if (any(last < first)) return;
    float extent = max(last.x - first.x, last.y - first.y) + 1.0;
    uint level = min(uint(ceil(log2(max(extent * 0.5, 1.0)))), uint(occlusion.viewport.z) - 1u);
    uint2 from = uint2(first) >> level;
    uint2 to = uint2(last) >> level;
    float farthest = 1.0;
    for (uint y = from.y; y <= to.y; ++y) {
        for (uint x = from.x; x <= to.x; ++x) {
            farthest = min(farthest, hiz.read(uint2(x, y), level).x);
        }
    }
    if (nearest < farthest * 0.9999 - 1.0e-7) {
        render_command command(output.commands, slot);
        command.reset();
        atomic_fetch_add_explicit(culled, 1u, memory_order_relaxed);
    }
}
