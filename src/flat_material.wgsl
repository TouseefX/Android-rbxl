// Flat-colour material shader with optional texture. No lighting.
// Bypasses Bevy's StandardMaterial (which fails to compile on some Adreno GPUs
// and renders magenta). Uses Bevy's default vertex shader + a tiny fragment
// shader, so it works on any GPU.

#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material_color: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> light_dir: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> has_texture: u32;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var tex_sampler: sampler;

@fragment
fn fragment(
    mesh: VertexOutput,
) -> @location(0) vec4<f32> {
    var out_rgb = material_color.rgb;
    var out_alpha = material_color.a;

    if (has_texture == 1u) {
        var tex_col = textureSample(tex, tex_sampler, mesh.uv);
        out_rgb = out_rgb * tex_col.rgb;
        out_alpha = tex_col.a * material_color.a;
    }

    // Flat/Lambert-style face shading. `light_dir` was declared and populated
    // from Rust (FlatMaterial::light_dir) but never actually read here, so
    // every face of every part rendered at identical brightness no matter
    // which way it faced. That's why boxy geometry (curbs, planters, building
    // masses) read as flat undifferentiated blobs instead of legible 3D
    // shapes — there was nothing to tell a top face from a side face apart
    // except their base color, and most parts only have one base color.
    let n = normalize(mesh.world_normal);
    let l = normalize(light_dir.xyz);
    let ndotl = clamp(dot(n, l), 0.0, 1.0);
    // Ambient floor (0.55) so shaded faces stay readable instead of going
    // black, plus a Lambert term (0.45) for the lit/shaded contrast.
    let lighting = 0.55 + 0.45 * ndotl;
    out_rgb = out_rgb * lighting;

    return vec4<f32>(out_rgb, out_alpha);
}
