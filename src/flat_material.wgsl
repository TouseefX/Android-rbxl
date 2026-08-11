// Flat-colour material shader with simple Lambert lighting + optional texture.
// Bypasses Bevy's StandardMaterial (which fails to compile on some Adreno GPUs
// and renders magenta). This uses Bevy's default vertex shader and a small
// fragment shader of our own, so it is guaranteed to work on the device.
//
// Lighting: a fixed directional light in world space, computed with the world
// normal from VertexOutput (Lambert + ambient floor), so parts read as 3D.
// Texture: if a texture is bound, sample it by UV and multiply by the lit color.

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
    // Lambert diffuse with an ambient floor so back/side faces aren't black.
    var lit = material_color.rgb;
    var ndotl = dot(normalize(mesh.world_normal), normalize(light_dir.xyz));
    var shade = ndotl * 0.7 + 0.35;
    lit = lit * shade;

    var out_alpha = material_color.a;
    if (has_texture == 1u) {
        var tex_col = textureSample(tex, tex_sampler, mesh.uv);
        lit = lit * tex_col.rgb;
        out_alpha = tex_col.a * material_color.a;
    }

    return vec4<f32>(lit, out_alpha);
}
