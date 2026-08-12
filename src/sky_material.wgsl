// Gradient sky-dome shader. Colors by the fragment's world-space direction:
// blue near the top, lighter/whiter at the horizon, and a soft ground color
// below the horizon. This matches the Roblox daytime sky look and, like the
// FlatMaterial shader, is trivial WGSL that compiles on any GPU (no StandardMaterial).

#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky_top: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> sky_horizon: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> ground_color: vec4<f32>;

@fragment
fn fragment(
    mesh: VertexOutput,
) -> @location(0) vec4<f32> {
    let dir = normalize(mesh.world_position - vec3<f32>(0.0, 0.0, 0.0));
    let up = dir.y; // -1 (down) .. 1 (up)

    var col: vec3<f32>;
    if (up < -0.01) {
        col = ground_color.rgb;
    } else {
        let t = clamp(up, 0.0, 1.0);
        // mix horizon -> top as we look up
        col = mix(sky_horizon.rgb, sky_top.rgb, t);
    }
    return vec4<f32>(col, 1.0);
}
