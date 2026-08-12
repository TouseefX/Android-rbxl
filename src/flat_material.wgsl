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

    return vec4<f32>(out_rgb, out_alpha);
}
