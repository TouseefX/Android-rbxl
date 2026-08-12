// Gradient sky-dome shader. Colors by the fragment's world-space direction:
// blue near the top, lighter/whiter at the horizon, and a soft ground color
// below the horizon. Matches the Roblox daytime sky. Uses the sphere's outward
// normal (= direction from its centre), so it stays correct even when the dome
// follows the camera around the scene.

#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky_top: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> sky_horizon: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> ground_color: vec4<f32>;

@fragment
fn fragment(
    mesh: VertexOutput,
) -> @location(0) vec4<f32> {
    let dir = normalize(mesh.world_normal);
    let up = dir.y; // -1 (down) .. 1 (up)

    // Smoothly blend ground -> horizon -> top instead of a hard cutoff at
    // up == -0.01. The hard `if` produced a visible seam: a flat-colored
    // band with a sharp edge cutting across the middle of the view wherever
    // the camera could see below the horizon between/around geometry.
    let ground_t = smoothstep(-0.06, 0.06, up);
    let base = mix(ground_color.rgb, sky_horizon.rgb, ground_t);
    let sky_t = clamp(up, 0.0, 1.0);
    let col = mix(base, sky_top.rgb, sky_t);
    return vec4<f32>(col, 1.0);
}
