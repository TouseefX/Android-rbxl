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
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var<uniform> tint_texture: u32;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<uniform> texture_mode: u32;

@fragment
fn fragment(
    mesh: VertexOutput,
) -> @location(0) vec4<f32> {
    var out_rgb = material_color.rgb;
    var out_alpha = material_color.a;

    if (has_texture == 1u) {
        // Roblox Texture and mesh UVs wrap outside 0..1. `fract` provides
        // repeat sampling even when a mobile backend creates a clamp sampler.
        var tex_col = textureSample(tex, tex_sampler, fract(mesh.uv));
        // Procedural material patterns (studs/brick/grass/etc., generated in
        // asset_downloader.rs) are deliberately greyscale/neutral and MEANT
        // to be multiplied by the part's BrickColor — that's how a red brick
        // gets red studs instead of always-grey studs. Real downloaded
        // Decal/MeshPart textures are different: Roblox doesn't tint those
        // by the part's Color, the image shows as-is. Without this
        // distinction, every real texture got multiplied by whatever
        // (usually non-white, often default grey 194) BrickColor the part
        // had — invisible before the texture finished downloading (nothing
        // to multiply against yet), then a visible muddy/grey tint the
        // moment it landed a few seconds later.
        if (texture_mode == 1u) {
            // SurfaceAppearance Overlay: image alpha reveals the underlying
            // MeshPart color; it does not make the object itself transparent.
            let textured = select(tex_col.rgb, out_rgb * tex_col.rgb, tint_texture == 1u);
            out_rgb = mix(out_rgb, textured, tex_col.a);
            out_alpha = material_color.a;
        } else if (texture_mode == 2u) {
            // TintMask: alpha chooses where MeshPart.Color tints the ColorMap.
            out_rgb = tex_col.rgb * mix(vec3<f32>(1.0), out_rgb, tex_col.a);
            out_alpha = material_color.a;
        } else {
            if (tint_texture == 1u) {
                out_rgb = out_rgb * tex_col.rgb;
            } else {
                out_rgb = tex_col.rgb;
            }
            // Opaque ignores image alpha; mode 0 is regular transparency.
            out_alpha = select(tex_col.a * material_color.a, material_color.a, texture_mode == 3u);
        }
    }

    // Deliberately unlit. This is the viewport's mobile-safe "Flat" mode:
    // Color3 and texture pixels reach the framebuffer without directional
    // lighting darkening them by as much as 45%. That lighting multiplication
    // made correctly decoded Roblox colors look like entirely different
    // BrickColors depending on face direction. Tonemapping is disabled on the
    // camera as well, so this is a predictable sRGB-authored color pipeline.
    return vec4<f32>(out_rgb, out_alpha);
}
