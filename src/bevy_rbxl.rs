// ============================================================================
// bevy_rbxl.rs  —  Bevy 3D renderer for Android-rbxl
// ----------------------------------------------------------------------------
// A port of OpenRBLX's 3D viewport renderer into the Android-rbxl app, but
// GPU-accelerated by the Bevy engine instead of OpenRBLX's raylib and instead
// of the app's CPU software rasterizer in `viewport3d.rs`.
//
// Design goal: keep the geometry, shape tessellation, coordinate conventions,
// brick-colour and decal/material logic *identical* to the working software
// rasterizer (which is proven to render real .rbxl places), but emit Bevy
// meshes/materials so rendering happens on the GPU.
//
// Coordinate convention
// ---------------------
// The software rasterizer renders in a left-handed "S space" where +Y is up,
// +Z faces the viewer and +X is right.  Bevy is right-handed (+Y up, camera
// looks down -Z).  To avoid mirroring/inside-out geometry we apply a global
// Z-flip to every world vertex and normal: (x, y, z) -> (x, y, -z), and do the
// same to the camera eye/target/up.  Combined with two-sided materials
// (`cull_mode: None`) this removes every winding/backface ambiguity and makes
// the Bevy output geometrically identical to the software rasterizer.
//
// Bevy version: 0.17.  Public API churns between minor versions, so every
// Bevy call is commented with what to change if you bump versions.
// ============================================================================

use crate::asset_downloader::{self, DecodedImage};
use crate::viewport3d::{brick_color_to_rgb, CFrame3D, Vec3};
use bevy::asset::RenderAssetUsages;
use bevy::camera::{Camera, Camera3d, ClearColorConfig, PerspectiveProjection, Projection};
use bevy::color::Color;
use bevy::image::Image;
use bevy::light::{AmbientLight, DirectionalLight};
use bevy::math::Vec3 as GVec3;
use bevy::mesh::{Indices, Mesh};
use bevy::pbr::StandardMaterial;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat};
use rbx_dom_weak::types::{Ref, Variant};
use rbx_dom_weak::WeakDom;
use std::collections::HashMap;

// ----------------------------------------------------------------------------
// Public settings & state
// ----------------------------------------------------------------------------

/// Mirror of the old `Viewport3D` camera/display settings so `app.rs` can keep
/// its existing UI (presets, skybox/grid/wireframe toggles, move speed, etc.).
#[derive(Debug, Clone, Resource)]
pub struct RbxViewportSettings {
    pub show_grid: bool,
    pub show_wireframe: bool,
    pub show_skybox: bool,
    pub move_speed: f32,
    pub clear_color: [f32; 3], // skybox/backdrop colour (SRGB 0..1)
    pub grid_color: [f32; 4],
    pub ground_color: [f32; 4],
}

impl Default for RbxViewportSettings {
    fn default() -> Self {
        Self {
            show_grid: false,
            show_wireframe: false,
            show_skybox: true,
            move_speed: 4.0,
            clear_color: [0.45, 0.66, 0.95], // Roblox daytime sky blue
            grid_color: [1.0, 1.0, 1.0, 0.25],
            ground_color: [0.27, 0.33, 0.24, 1.0],
        }
    }
}

/// Orbit camera state, kept in the rasterizer's "S space" so the existing
/// `set_preset` / `focus_on` / `move_*` math in `app.rs` keeps working.
#[derive(Debug, Clone, Copy, Resource)]
pub struct OrbitCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            yaw: 0.785,
            pitch: 0.45,
            distance: 65.0,
            target: Vec3::new(0.0, 6.0, 0.0),
        }
    }
}

// ----------------------------------------------------------------------------
// Geometry extraction (identical logic to viewport3d::generate_instance_triangles)
// ----------------------------------------------------------------------------

/// A triangle in *Bevy world* coordinates (already Z-flipped).
#[derive(Debug, Clone)]
pub struct Btri {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Texture key: a real asset id, a "__material" pseudo key, or None = plain colour.
    pub tex: Option<String>,
}

/// The extracted geometry + paint for a single BasePart-like instance.
#[derive(Debug, Clone)]
pub struct PartGeo {
    pub referent: Ref,
    pub name: String,
    pub class: String,
    pub color: [f32; 3],
    pub alpha: f32,
    pub tris: Vec<Btri>,
}

#[inline(always)]
fn tp(cf: &CFrame3D, p: Vec3) -> [f32; 3] {
    let w = cf.transform_point(p);
    [w.x, w.y, -w.z]
}

#[inline(always)]
fn tn(cf: &CFrame3D, n: Vec3) -> [f32; 3] {
    let w = cf.transform_normal(n);
    [w.x, w.y, -w.z]
}

fn s_to_b(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[1], -p[2]]
}

fn part_alpha(transparency: f32) -> f32 {
    (1.0 - transparency).clamp(0.0, 1.0)
}

/// Roblox BrickColor code -> linear-ish SRGB [r,g,b] in 0..1.
fn brick_code_rgb(code: u32) -> [f32; 3] {
    let c = brick_color_to_rgb(code);
    [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0]
}

/// Shape of a primitive part (mirrors `RenderShape` in OpenRBLX's renderer.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Block,
    Ball,
    Cylinder,
    Wedge,
    CornerWedge,
    Truss,
    Mesh,
}

fn extract_part_shape(inst: &rbx_dom_weak::Instance, mesh_shape_type: Option<&str>) -> Shape {
    if let Some(mt) = mesh_shape_type {
        if mt == "Sphere" || mt == "Head" {
            return Shape::Ball;
        }
        if mt == "Cylinder" {
            return Shape::Cylinder;
        }
        if mt == "Wedge" {
            return Shape::Wedge;
        }
        if mt == "FileMesh" || mt == "Brick" {
            return Shape::Mesh;
        }
    }
    if inst.class == "WedgePart" {
        return Shape::Wedge;
    }
    if inst.class == "CornerWedgePart" {
        return Shape::CornerWedge;
    }
    if inst.class == "TrussPart" {
        return Shape::Truss;
    }
    if inst.name.to_lowercase().contains("sphere")
        || inst.name.to_lowercase().contains("ball")
        || inst.name.to_lowercase().contains("wheel")
    {
        return Shape::Ball;
    }
    if let Some(v) = inst.properties.get(&rbx_dom_weak::ustr("Shape")) {
        match v {
            Variant::String(s) => match s.as_str() {
                "Ball" => Shape::Ball,
                "Cylinder" => Shape::Cylinder,
                "Block" => Shape::Block,
                "Wedge" => Shape::Wedge,
                _ => Shape::Block,
            },
            Variant::Int32(0) | Variant::Int64(0) => Shape::Ball,
            Variant::Int32(1) | Variant::Int64(1) => Shape::Block,
            Variant::Int32(2) | Variant::Int64(2) => Shape::Cylinder,
            Variant::Int32(3) | Variant::Int64(3) => Shape::Wedge,
            Variant::Int32(4) | Variant::Int64(4) => Shape::CornerWedge,
            _ => Shape::Block,
        }
    } else {
        Shape::Block
    }
}

fn decal_face_name(inst: &rbx_dom_weak::Instance) -> &'static str {
    if let Some(v) = inst.properties.get(&rbx_dom_weak::ustr("Face")) {
        match v {
            Variant::String(s) => match s.as_str() {
                "Top" => "Top",
                "Bottom" => "Bottom",
                "Front" => "Front",
                "Back" => "Back",
                "Left" => "Left",
                "Right" => "Right",
                _ => "Front",
            },
            Variant::Int32(0) | Variant::Int64(0) => "Right",
            Variant::Int32(1) | Variant::Int64(1) => "Top",
            Variant::Int32(2) | Variant::Int64(2) => "Back",
            Variant::Int32(3) | Variant::Int64(3) => "Left",
            Variant::Int32(4) | Variant::Int64(4) => "Bottom",
            Variant::Int32(5) | Variant::Int64(5) => "Front",
            _ => "Front",
        }
    } else {
        "Front"
    }
}

fn is_studs(inst: &rbx_dom_weak::Instance, prop: &str) -> bool {
    if let Some(v) = inst.properties.get(&rbx_dom_weak::ustr(prop)) {
        match v {
            Variant::String(s) => s == "Studs",
            Variant::Int32(3) | Variant::Int64(3) => true,
            _ => false,
        }
    } else {
        false
    }
}

fn is_inlet(inst: &rbx_dom_weak::Instance, prop: &str) -> bool {
    if let Some(v) = inst.properties.get(&rbx_dom_weak::ustr(prop)) {
        match v {
            Variant::String(s) => s == "Inlet" || s == "Inlets",
            Variant::Int32(4) | Variant::Int64(4) => true,
            _ => false,
        }
    } else {
        false
    }
}

/// Walk the place and extract every renderable BasePart as Bevy triangles.
pub fn extract_geometry(dom: &WeakDom, selected: Option<Ref>) -> Vec<PartGeo> {
    let mut out = Vec::new();
    let mut stack = dom.root().children().to_vec();
    while let Some(r) = stack.pop() {
        let Some(inst) = dom.get_by_ref(r) else { continue };
        stack.extend(inst.children());

        let is_3d = matches!(
            inst.class.as_str(),
            "Part"
                | "WedgePart"
                | "CornerWedgePart"
                | "TrussPart"
                | "SpawnLocation"
                | "MeshPart"
                | "UnionOperation"
                | "Seat"
                | "VehicleSeat"
                | "FlagStand"
                | "Terrain"
        );
        if is_3d {
            if let Some(geo) = extract_part(dom, r, inst, selected) {
                out.push(geo);
            }
        }
    }
    out
}

fn extract_part(
    dom: &WeakDom,
    referent: Ref,
    inst: &rbx_dom_weak::Instance,
    _selected: Option<Ref>,
) -> Option<PartGeo> {
    let cframe = crate::viewport3d::extract_instance_cframe(inst);
    let size = match inst.properties.get(&rbx_dom_weak::ustr("Size")) {
        Some(Variant::Vector3(v)) => Vec3::new(v.x.max(0.1), v.y.max(0.1), v.z.max(0.1)),
        _ => Vec3::new(4.0, 1.2, 2.0),
    };

    // Colour: Color3, Color3uint8, or BrickColor.
    let color = match inst.properties.get(&rbx_dom_weak::ustr("Color")) {
        Some(Variant::Color3(c)) => [c.r as f32, c.g as f32, c.b as f32],
        Some(Variant::Color3uint8(c)) => [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0],
        _ => match inst.properties.get(&rbx_dom_weak::ustr("BrickColor")) {
            Some(Variant::BrickColor(bc)) => brick_code_rgb(*bc as u32),
            Some(Variant::Int32(bc)) => brick_code_rgb(*bc as u32),
            Some(Variant::Int64(bc)) => brick_code_rgb(*bc as u32),
            _ => [163.0 / 255.0, 162.0 / 255.0, 165.0 / 255.0],
        },
    };
    let color = [color[0].min(1.0), color[1].min(1.0), color[2].min(1.0)];

    let transparency = match inst.properties.get(&rbx_dom_weak::ustr("Transparency")) {
        Some(Variant::Float32(f)) => *f,
        Some(Variant::Float64(f)) => *f as f32,
        _ => 0.0,
    };
    let alpha = part_alpha(transparency);

    let material_str = match inst.properties.get(&rbx_dom_weak::ustr("Material")) {
        Some(Variant::String(s)) => s.clone(),
        _ => "Plastic".to_string(),
    };
    let is_neon = material_str == "Neon";
    let is_spawn = inst.class == "SpawnLocation";
    let is_baseplate = size.x >= 40.0 || size.z >= 40.0 || inst.name.to_lowercase().contains("base");

    // MeshPart / SpecialMesh / BlockMesh children.
    let mut mesh_id: Option<String> = None;
    let mut mesh_tex: Option<String> = None;
    let mut mesh_shape_type: Option<String> = None;
    let mut scale = Vec3::new(1.0, 1.0, 1.0);
    let mut offset = Vec3::ZERO;

    if inst.class == "MeshPart" {
        if let Some(Variant::String(mid)) = inst.properties.get(&rbx_dom_weak::ustr("MeshId")) {
            mesh_id = Some(mid.clone());
        }
        if let Some(Variant::String(t)) = inst
            .properties
            .get(&rbx_dom_weak::ustr("TextureID"))
            .or_else(|| inst.properties.get(&rbx_dom_weak::ustr("TextureId")))
        {
            mesh_tex = Some(t.clone());
        }
    }

    for child_ref in inst.children() {
        let Some(child) = dom.get_by_ref(*child_ref) else { continue };
        if child.class == "SpecialMesh" || child.class == "BlockMesh" {
            if let Some(Variant::String(m)) = child.properties.get(&rbx_dom_weak::ustr("MeshId")) {
                mesh_id = Some(m.clone());
            }
            if let Some(Variant::String(t)) = child.properties.get(&rbx_dom_weak::ustr("TextureId")) {
                mesh_tex = Some(t.clone());
            }
            if let Some(Variant::String(mt)) = child.properties.get(&rbx_dom_weak::ustr("MeshType")) {
                mesh_shape_type = Some(mt.clone());
            }
            if let Some(Variant::Vector3(sc)) = child.properties.get(&rbx_dom_weak::ustr("Scale")) {
                scale = Vec3::new(sc.x, sc.y, sc.z);
            }
            if let Some(Variant::Vector3(off)) = child.properties.get(&rbx_dom_weak::ustr("Offset")) {
                offset = Vec3::new(off.x, off.y, off.z);
            }
        }
    }

    // Decals / Textures on part faces.
    let mut decals: HashMap<&'static str, String> = HashMap::new();
    for child_ref in inst.children() {
        let Some(child) = dom.get_by_ref(*child_ref) else { continue };
        if child.class == "Decal" || child.class == "Texture" {
            let face = decal_face_name(child);
            if let Some(Variant::String(t)) = child.properties.get(&rbx_dom_weak::ustr("Texture")) {
                if !t.is_empty() {
                    decals.insert(face, t.clone());
                }
            }
        }
    }

    // Kick off mesh download in the background if needed.
    if let Some(ref mid) = mesh_id {
        if asset_downloader::get_cached_mesh(mid).is_none() {
            crate::roblox_api::fetch_and_cache_mesh_async(mid.clone(), None);
        }
    }

    let half = Vec3::new(size.x * 0.5 * scale.x, size.y * 0.5 * scale.y, size.z * 0.5 * scale.z);
    // Apply the SpecialMesh Offset as a rotation-only translation (matches the
    // software rasterizer, which applies `offset` through the part's CFrame).
    let mut part_cf = cframe;
    let rot_offset = cframe.transform_point(offset).sub(&cframe.pos);
    part_cf.pos = part_cf.pos.add(&rot_offset);

    let mut tris: Vec<Btri> = Vec::new();

    // 1) Real .mesh file
    if let Some(ref mid) = mesh_id {
        if let Some(md) = asset_downloader::get_cached_mesh(mid) {
            let bx = (md.aabb_max[0] - md.aabb_min[0]).max(0.01);
            let by = (md.aabb_max[1] - md.aabb_min[1]).max(0.01);
            let bz = (md.aabb_max[2] - md.aabb_min[2]).max(0.01);
            let sx = (size.x * scale.x) / bx;
            let sy = (size.y * scale.y) / by;
            let sz = (size.z * scale.z) / bz;
            let tex_key = mesh_tex.clone().or_else(|| asset_downloader::extract_asset_id(mid));

            for f in &md.faces {
                if let (Some(&va), Some(&vb), Some(&vc)) = (
                    md.vertices.get(f[0] as usize),
                    md.vertices.get(f[1] as usize),
                    md.vertices.get(f[2] as usize),
                ) {
                    let pa = tp(&part_cf, Vec3::new(va[0] * sx, va[1] * sy, va[2] * sz));
                    let pb = tp(&part_cf, Vec3::new(vb[0] * sx, vb[1] * sy, vb[2] * sz));
                    let pc = tp(&part_cf, Vec3::new(vc[0] * sx, vc[1] * sy, vc[2] * sz));
                    let ab = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
                    let ac = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
                    let n = cross(ab, ac);
                    let uva = md.uvs.get(f[0] as usize).copied().unwrap_or([0.0, 0.0]);
                    let uvb = md.uvs.get(f[1] as usize).copied().unwrap_or([1.0, 0.0]);
                    let uvc = md.uvs.get(f[2] as usize).copied().unwrap_or([0.0, 1.0]);
                    let key = tex_key.clone();
                    tris.push(Btri { pos: pa, normal: n, uv: uva, tex: key.clone() });
                    tris.push(Btri { pos: pb, normal: n, uv: uvb, tex: key.clone() });
                    tris.push(Btri { pos: pc, normal: n, uv: uvc, tex: key });
                }
            }

            if tris.is_empty() {
                return None;
            }
            return Some(PartGeo {
                referent,
                name: inst.name.clone(),
                class: inst.class.to_string(),
                color,
                alpha,
                tris,
            });
        }
    }

    // 2) Primitive shapes
    let shape = extract_part_shape(inst, mesh_shape_type.as_deref());
    match shape {
        Shape::Ball => build_ball(&mut tris, &part_cf, half),
        Shape::Cylinder => build_cylinder(&mut tris, &part_cf, half),
        Shape::Wedge => build_wedge(&mut tris, &part_cf, half, inst, is_spawn),
        Shape::CornerWedge => build_corner_wedge(&mut tris, &part_cf, half, inst, is_spawn),
        Shape::Truss => build_truss(&mut tris, &part_cf, half),
        _ => build_block(
            &mut tris,
            &part_cf,
            half,
            inst,
            material_str.as_str(),
            is_neon,
            is_spawn,
            is_baseplate,
            &decals,
        ),
    }

    if tris.is_empty() {
        return None;
    }
    Some(PartGeo {
        referent,
        name: inst.name.clone(),
        class: inst.class.to_string(),
        color,
        alpha,
        tris,
    })
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let n = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 1e-8 {
        [n[0] / len, n[1] / len, n[2] / len]
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn push_quad(tris: &mut Vec<Btri>, cf: &CFrame3D, p: [Vec3; 4], n: Vec3, uv_quad: [[f32; 2]; 4], tex: Option<String>) {
    let pa = tp(cf, p[0]);
    let pb = tp(cf, p[1]);
    let pc = tp(cf, p[2]);
    let pd = tp(cf, p[3]);
    let nrm = tn(cf, n);
    tris.push(Btri { pos: pa, normal: nrm, uv: uv_quad[0], tex: tex.clone() });
    tris.push(Btri { pos: pb, normal: nrm, uv: uv_quad[1], tex: tex.clone() });
    tris.push(Btri { pos: pc, normal: nrm, uv: uv_quad[2], tex: tex.clone() });
    // second triangle
    tris.push(Btri { pos: pa, normal: nrm, uv: uv_quad[0], tex: tex.clone() });
    tris.push(Btri { pos: pc, normal: nrm, uv: uv_quad[2], tex: tex.clone() });
    tris.push(Btri { pos: pd, normal: nrm, uv: uv_quad[3], tex });
}

fn push_tri(tris: &mut Vec<Btri>, cf: &CFrame3D, p: [Vec3; 3], n: Vec3, uv: [[f32; 2]; 3], tex: Option<String>) {
    let nrm = tn(cf, n);
    tris.push(Btri { pos: tp(cf, p[0]), normal: nrm, uv: uv[0], tex: tex.clone() });
    tris.push(Btri { pos: tp(cf, p[1]), normal: nrm, uv: uv[1], tex: tex.clone() });
    tris.push(Btri { pos: tp(cf, p[2]), normal: nrm, uv: uv[2], tex });
}

fn build_block(
    tris: &mut Vec<Btri>,
    cf: &CFrame3D,
    half: Vec3,
    inst: &rbx_dom_weak::Instance,
    material_str: &str,
    _is_neon: bool,
    is_spawn: bool,
    is_baseplate: bool,
    decals: &HashMap<&'static str, String>,
) {
    let h = half;
    let v = [
        Vec3::new(-h.x, -h.y, -h.z),
        Vec3::new(h.x, -h.y, -h.z),
        Vec3::new(h.x, -h.y, h.z),
        Vec3::new(-h.x, -h.y, h.z),
        Vec3::new(-h.x, h.y, -h.z),
        Vec3::new(h.x, h.y, -h.z),
        Vec3::new(h.x, h.y, h.z),
        Vec3::new(-h.x, h.y, h.z),
    ];

    let mat_tex = match material_str {
        "Brick" => Some("__brick".to_string()),
        "DiamondPlate" | "CorrodedMetal" => Some("__diamond_plate".to_string()),
        "Wood" | "WoodPlanks" => Some("__wood_planks".to_string()),
        "Cobblestone" => Some("__cobblestone".to_string()),
        "Grass" => Some("__grass".to_string()),
        "Concrete" | "Slate" => Some("__concrete".to_string()),
        _ => None,
    };

    let faces: [(&str, [usize; 4], Vec3, bool); 6] = [
        ("Top", [4, 5, 6, 7], Vec3::new(0.0, 1.0, 0.0), true),
        ("Bottom", [0, 3, 2, 1], Vec3::new(0.0, -1.0, 0.0), false),
        ("Front", [3, 2, 6, 7], Vec3::new(0.0, 0.0, 1.0), false),
        ("Back", [1, 0, 4, 5], Vec3::new(0.0, 0.0, -1.0), false),
        ("Right", [2, 1, 5, 6], Vec3::new(1.0, 0.0, 0.0), false),
        ("Left", [0, 3, 7, 4], Vec3::new(-1.0, 0.0, 0.0), false),
    ];

    for (face, idx, local_norm, is_top) in faces {
        let face_tex: Option<String> = if let Some(d) = decals.get(face) {
            Some(d.clone())
        } else if is_top {
            if is_spawn {
                Some("SpawnLocation.png".to_string())
            } else if is_studs(inst, "TopSurface") && material_str == "Plastic" && !is_baseplate {
                Some("__studs".to_string())
            } else {
                mat_tex.clone()
            }
        } else if face == "Bottom" {
            if is_inlet(inst, "BottomSurface") && material_str == "Plastic" && !is_baseplate {
                Some("__inlets".to_string())
            } else {
                mat_tex.clone()
            }
        } else {
            mat_tex.clone()
        };
        let uvq = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        push_quad(tris, cf, [v[idx[0]], v[idx[1]], v[idx[2]], v[idx[3]]], local_norm, uvq, face_tex);
    }
}

fn build_ball(tris: &mut Vec<Btri>, cf: &CFrame3D, half: Vec3) {
    let lats = 8;
    let lons = 12;
    let radius = half.x.min(half.y).min(half.z);
    for i in 0..lats {
        let lat0 = std::f32::consts::PI * (-0.5 + i as f32 / lats as f32);
        let lat1 = std::f32::consts::PI * (-0.5 + (i + 1) as f32 / lats as f32);
        let z0 = lat0.sin() * radius;
        let z1 = lat1.sin() * radius;
        let r0 = lat0.cos() * radius;
        let r1 = lat1.cos() * radius;
        let v0 = i as f32 / lats as f32;
        let v1 = (i + 1) as f32 / lats as f32;
        for j in 0..lons {
            let a0 = 2.0 * std::f32::consts::PI * j as f32 / lons as f32;
            let a1 = 2.0 * std::f32::consts::PI * (j + 1) as f32 / lons as f32;
            let x0 = a0.cos();
            let y0 = a0.sin();
            let x1 = a1.cos();
            let y1 = a1.sin();
            let u0 = j as f32 / lons as f32;
            let u1 = (j + 1) as f32 / lons as f32;
            let nrm = Vec3::new((x0 + x1) * 0.5 * (r0 + r1) * 0.5, (y0 + y1) * 0.5 * (r0 + r1) * 0.5, (z0 + z1) * 0.5).normalize();
            push_tri(
                tris,
                cf,
                [
                    Vec3::new(x0 * r0, y0 * r0, z0),
                    Vec3::new(x1 * r0, y1 * r0, z0),
                    Vec3::new(x1 * r1, y1 * r1, z1),
                ],
                nrm,
                [[u0, v0], [u1, v0], [u1, v1]],
                None,
            );
            push_tri(
                tris,
                cf,
                [
                    Vec3::new(x0 * r0, y0 * r0, z0),
                    Vec3::new(x1 * r1, y1 * r1, z1),
                    Vec3::new(x0 * r1, y0 * r1, z1),
                ],
                nrm,
                [[u0, v0], [u1, v1], [u0, v1]],
                None,
            );
        }
    }
}

fn build_cylinder(tris: &mut Vec<Btri>, cf: &CFrame3D, half: Vec3) {
    let segments = 12;
    let radius = half.y.min(half.z);
    let mut front: Vec<Vec3> = Vec::new();
    let mut back: Vec<Vec3> = Vec::new();
    for i in 0..segments {
        let t = i as f32 / segments as f32 * 2.0 * std::f32::consts::PI;
        let n2 = (i + 1) as f32 / segments as f32 * 2.0 * std::f32::consts::PI;
        let (sy1, sz1) = t.sin_cos();
        let (sy2, sz2) = n2.sin_cos();
        let p0 = Vec3::new(-half.x, sy1 * radius, sz1 * radius);
        let p1 = Vec3::new(half.x, sy1 * radius, sz1 * radius);
        let p2 = Vec3::new(half.x, sy2 * radius, sz2 * radius);
        let p3 = Vec3::new(-half.x, sy2 * radius, sz2 * radius);
        let mid_n = Vec3::new(0.0, (sy1 + sy2) * 0.5, (sz1 + sz2) * 0.5).normalize();
        push_quad(tris, cf, [p0, p1, p2, p3], mid_n, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], None);
        front.push(Vec3::new(half.x, sy1 * radius, sz1 * radius));
        back.push(Vec3::new(-half.x, sy1 * radius, sz1 * radius));
    }
    let fc = Vec3::new(half.x, 0.0, 0.0);
    let bc = Vec3::new(-half.x, 0.0, 0.0);
    for i in 0..segments {
        let n2 = (i + 1) % segments;
        push_tri(tris, cf, [fc, front[i], front[n2]], Vec3::new(1.0, 0.0, 0.0), [[0.5, 0.5], [0.0, 0.0], [1.0, 0.0]], None);
        push_tri(tris, cf, [bc, back[n2], back[i]], Vec3::new(-1.0, 0.0, 0.0), [[0.5, 0.5], [1.0, 0.0], [0.0, 0.0]], None);
    }
}

fn build_wedge(tris: &mut Vec<Btri>, cf: &CFrame3D, half: Vec3, inst: &rbx_dom_weak::Instance, is_spawn: bool) {
    let h = half;
    let v0 = Vec3::new(-h.x, -h.y, -h.z);
    let v1 = Vec3::new(h.x, -h.y, -h.z);
    let v2 = Vec3::new(h.x, -h.y, h.z);
    let v3 = Vec3::new(-h.x, -h.y, h.z);
    let v4 = Vec3::new(-h.x, h.y, -h.z);
    let v5 = Vec3::new(h.x, h.y, -h.z);
    let bot_tex = if is_inlet(inst, "BottomSurface") { Some("__inlets".to_string()) } else { None };
    let top_tex = if is_spawn { Some("SpawnLocation.png".to_string()) } else if is_studs(inst, "TopSurface") { Some("__studs".to_string()) } else { None };

    push_quad(tris, cf, [v0, v1, v2, v3], Vec3::new(0.0, -1.0, 0.0), [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], bot_tex);
    push_quad(tris, cf, [v1, v0, v4, v5], Vec3::new(0.0, 0.0, -1.0), [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], None);
    let ramp_norm = Vec3::new(0.0, h.z, h.y).normalize();
    push_quad(tris, cf, [v3, v2, v5, v4], ramp_norm, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], top_tex);
    push_tri(tris, cf, [v0, v3, v4], Vec3::new(-1.0, 0.0, 0.0), [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], None);
    push_tri(tris, cf, [v2, v1, v5], Vec3::new(1.0, 0.0, 0.0), [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], None);
}

fn build_corner_wedge(tris: &mut Vec<Btri>, cf: &CFrame3D, half: Vec3, inst: &rbx_dom_weak::Instance, is_spawn: bool) {
    let h = half;
    let b0 = Vec3::new(-h.x, -h.y, -h.z);
    let b1 = Vec3::new(h.x, -h.y, -h.z);
    let b2 = Vec3::new(h.x, -h.y, h.z);
    let b3 = Vec3::new(-h.x, -h.y, h.z);
    let top = Vec3::new(-h.x, h.y, -h.z);
    let bot_tex = if is_inlet(inst, "BottomSurface") { Some("__inlets".to_string()) } else { None };
    let top_tex = if is_spawn { Some("SpawnLocation.png".to_string()) } else { None };

    push_quad(tris, cf, [b0, b1, b2, b3], Vec3::new(0.0, -1.0, 0.0), [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], bot_tex);
    push_tri(tris, cf, [b1, b0, top], Vec3::new(0.0, 0.0, -1.0), [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]], None);
    push_tri(tris, cf, [b0, b3, top], Vec3::new(-1.0, 0.0, 0.0), [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]], None);
    let sloped = Vec3::new(h.y, h.x, h.z).normalize();
    push_tri(tris, cf, [b3, b2, top], sloped, [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]], top_tex.clone());
    push_tri(tris, cf, [b2, b1, top], sloped, [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]], top_tex);
}

fn build_truss(tris: &mut Vec<Btri>, cf: &CFrame3D, half: Vec3) {
    // Simple truss: approximate as a cross-section box lattice (a plain box is
    // a good-enough placeholder; upgrade with real truss struts later).
    let h = half;
    let v = [
        Vec3::new(-h.x, -h.y, -h.z),
        Vec3::new(h.x, -h.y, -h.z),
        Vec3::new(h.x, -h.y, h.z),
        Vec3::new(-h.x, -h.y, h.z),
        Vec3::new(-h.x, h.y, -h.z),
        Vec3::new(h.x, h.y, -h.z),
        Vec3::new(h.x, h.y, h.z),
        Vec3::new(-h.x, h.y, h.z),
    ];
    let faces: [([usize; 4], Vec3); 6] = [
        ([4, 5, 6, 7], Vec3::new(0.0, 1.0, 0.0)),
        ([0, 3, 2, 1], Vec3::new(0.0, -1.0, 0.0)),
        ([3, 2, 6, 7], Vec3::new(0.0, 0.0, 1.0)),
        ([1, 0, 4, 5], Vec3::new(0.0, 0.0, -1.0)),
        ([2, 1, 5, 6], Vec3::new(1.0, 0.0, 0.0)),
        ([0, 3, 7, 4], Vec3::new(-1.0, 0.0, 0.0)),
    ];
    for (idx, n) in faces {
        push_quad(tris, cf, [v[idx[0]], v[idx[1]], v[idx[2]], v[idx[3]]], n, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], None);
    }
}

// ----------------------------------------------------------------------------
// Bevy material & image helpers
// ----------------------------------------------------------------------------

/// Resolve a texture key into RGBA image data (from the app's downloader cache
/// or procedural generators).
pub fn resolve_texture_rgba(key: &str) -> Option<DecodedImage> {
    // Procedural / builtin pseudo-textures.
    let pseudo: Option<DecodedImage> = match key {
        "__studs" => Some(asset_downloader::generate_studs_texture()),
        "__inlets" => Some(asset_downloader::generate_inlets_texture()),
        "__brick" => Some(asset_downloader::generate_brick_texture()),
        "__diamond_plate" => Some(asset_downloader::generate_diamond_plate_texture()),
        "__wood_planks" => Some(asset_downloader::generate_wood_planks_texture()),
        "__cobblestone" => Some(asset_downloader::generate_cobblestone_texture()),
        "__grass" => Some(asset_downloader::generate_grass_texture()),
        "__concrete" => Some(asset_downloader::generate_concrete_texture()),
        _ => None,
    };
    if let Some(img) = pseudo {
        return Some(img);
    }
    let id = asset_downloader::extract_asset_id(key).unwrap_or_else(|| key.to_string());
    asset_downloader::get_cached_image(&id)
        .map(|a| (*a).clone())
        .or_else(|| {
            // Fall back to a bundled asset, decoded on the fly.
            asset_downloader::get_builtin_asset(&id).and_then(|bytes| asset_downloader::decode_image_bytes(bytes))
        })
}

/// Convert a `DecodedImage` into a Bevy `Image` (RGBA8 sRGB).
pub fn decoded_to_bevy_image(img: &DecodedImage) -> Image {
    Image::new(
        Extent3d {
            width: img.width as u32,
            height: img.height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        img.rgba.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Simple procedural clear-sky gradient texture used as the background when
/// `show_skybox` is off (avoids depending on the Sky plugin).
pub fn solid_color_image(rgb: [f32; 3]) -> Image {
    let (r, g, b) = ((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8);
    let mut rgba = Vec::with_capacity(4);
    rgba.extend_from_slice(&[r, g, b, 255]);
    Image::new(
        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

// ----------------------------------------------------------------------------
// Spawning the Bevy scene
// ----------------------------------------------------------------------------

/// Marker component so the scene can be rebuilt (despawned) when the dom changes.
#[derive(Component)]
pub struct RbxSceneRoot;

/// Cache of Bevy image handles keyed by texture key, so textures are loaded once.
#[derive(Default, Resource)]
pub struct TextureRegistry {
    pub map: HashMap<String, Handle<Image>>,
}

/// Spawn the entire 3D scene from extracted part geometry and return the root
/// entity. Callers are responsible for despawning the previous root with
/// `entity(root).despawn_recursive()` before calling again (see `scene_root`
/// usage in the offscreen bridge and example).
pub fn spawn_scene(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    tex_reg: &mut TextureRegistry,
    parts: &[PartGeo],
    settings: &RbxViewportSettings,
) -> Entity {
    let root = commands.spawn(RbxSceneRoot).id();

    // Ground + grid helper plane at y = 0.
    let grid = if settings.show_grid {
        Some(build_grid_mesh(meshes, materials))
    } else {
        None
    };

    // Bucket triangles by (texture, colour, alpha) so every mesh shares one
    // material (a mesh can only have one StandardMaterial). Without colour in
    // the key, differently-coloured parts would all render one colour.
    type BucketKey = (Option<String>, [u8; 3], u8);
    let mut buckets: HashMap<BucketKey, Vec<Btri>> = HashMap::new();
    for p in parts {
        let ckey: [u8; 3] = [
            (p.color[0] * 255.0).round() as u8,
            (p.color[1] * 255.0).round() as u8,
            (p.color[2] * 255.0).round() as u8,
        ];
        let akey = (p.alpha * 255.0).round() as u8;
        for t in &p.tris {
            buckets.entry((t.tex.clone(), ckey, akey)).or_default().push(t.clone());
        }
    }

    for ((tex_key, ckey, akey), tris) in buckets {
        if tris.is_empty() {
            continue;
        }
        let color = [ckey[0] as f32 / 255.0, ckey[1] as f32 / 255.0, ckey[2] as f32 / 255.0];
        let alpha = akey as f32 / 255.0;
        // Build mesh.
        let mut positions = Vec::with_capacity(tris.len() * 3);
        let mut normals = Vec::with_capacity(tris.len() * 3);
        let mut uvs = Vec::with_capacity(tris.len() * 3);
        let mut indices: Vec<u32> = Vec::with_capacity(tris.len() * 3);
        let mut base = 0u32;
        for t in &tris {
            positions.push([t.pos[0], t.pos[1], t.pos[2]]);
            normals.push([t.normal[0], t.normal[1], t.normal[2]]);
            uvs.push([t.uv[0], t.uv[1]]);
            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
            base += 3;
        }

        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(Indices::U32(indices));
        let mesh_handle = meshes.add(mesh);

        // Build material.
        let mut material = StandardMaterial {
            base_color: Color::srgba(color[0], color[1], color[2], alpha),
            // Two-sided: avoids winding/backface issues from the coordinate flip.
            cull_mode: None,
            ..default()
        };
        if let Some(key) = &tex_key {
            if let Some(img) = resolve_texture_rgba(key) {
                let handle = match tex_reg.map.get(key) {
                    Some(h) => h.clone(),
                    None => {
                        let h = images.add(decoded_to_bevy_image(&img));
                        tex_reg.map.insert(key.clone(), h.clone());
                        h
                    }
                };
                material.base_color_texture = Some(handle);
                // Reset tint to white so the texture shows at full strength.
                material.base_color = Color::WHITE.with_alpha(alpha);
            }
        }
        let mat_handle = materials.add(material);

        commands.entity(root).with_children(|parent| {
            parent.spawn((
                Mesh3d(mesh_handle),
                MeshMaterial3d(mat_handle),
                Transform::IDENTITY,
                Visibility::default(),
            ));
        });
    }

    // Spawn the grid plane as a child of the root.
    if let Some((mh, mat)) = grid {
        commands.entity(root).with_children(|parent| {
            parent.spawn((Mesh3d(mh), MeshMaterial3d(mat), Transform::IDENTITY, Visibility::default()));
        });
    }

    root
}

fn build_grid_mesh(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> (Handle<Mesh>, Handle<StandardMaterial>) {
    let size = 100.0;
    let step = 4.0;
    let n = (size / step) as i32;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut k = 0u32;
    for i in -n..=n {
        let x = i as f32 * step;
        positions.push([x, 0.01, -size]);
        positions.push([x, 0.01, size]);
        indices.push(k);
        indices.push(k + 1);
        k += 2;
        positions.push([-size, 0.01, x]);
        positions.push([size, 0.01, x]);
        indices.push(k);
        indices.push(k + 1);
        k += 2;
    }
    let mut mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    let mh = meshes.add(mesh);
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.9, 0.9, 0.9, 0.4),
        unlit: true,
        ..default()
    });
    (mh, mat)
}

// ----------------------------------------------------------------------------
// Camera
// ----------------------------------------------------------------------------

/// Marker for the scene camera.
#[derive(Component)]
pub struct RbxCamera;

/// A Bevy plugin that owns the orbit camera and lights.  Use this for a
/// standalone windowed app; the offscreen bridge calls the systems directly.
pub struct RbxCameraPlugin {
    pub camera: OrbitCamera,
    pub settings: RbxViewportSettings,
}

impl Plugin for RbxCameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.settings.clone());
        app.insert_resource(self.camera);
        let cam = self.camera;
        app.add_systems(Startup, move |mut commands: Commands| {
            spawn_camera_and_lights(&mut commands, cam);
        });
        app.add_systems(Update, update_orbit_camera);
    }
}

/// Spawn camera + lights. `cam` is in S space; we Z-flip for Bevy.
pub fn spawn_camera_and_lights(commands: &mut Commands, cam: OrbitCamera) {
    let (eye_b, target_b) = orbit_eye_target_b(&cam);
    commands.spawn((
        Camera3d::default(),
        Camera {
            // skybox backdrop via clear colour
            clear_color: ClearColorConfig::Custom(Color::srgb(0.45, 0.66, 0.95)),
            ..default()
        },
        Transform::from_translation(eye_b).looking_at(target_b, GVec3::Y),
        Projection::Perspective(PerspectiveProjection {
            fov: 60f32.to_radians(),
            ..default()
        }),
        RbxCamera,
        Visibility::default(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 30_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(50.0, 80.0, -40.0).looking_at(GVec3::ZERO, GVec3::Y),
    ));
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
        ..default()
    });
}

/// Orbit state is stored in S space; convert eye/target to Bevy world (Z-flip).
pub fn orbit_eye_target_b(cam: &OrbitCamera) -> (GVec3, GVec3) {
    let (sp, cp) = cam.pitch.sin_cos();
    let (sy, cy) = cam.yaw.sin_cos();
    let eye_s = Vec3::new(
        cam.target.x + cam.distance * cp * sy,
        cam.target.y + cam.distance * sp,
        cam.target.z + cam.distance * cp * cy,
    );
    let eye_b = GVec3::new(eye_s.x, eye_s.y, -eye_s.z);
    let target_b = GVec3::new(cam.target.x, cam.target.y, -cam.target.z);
    (eye_b, target_b)
}

/// System that keeps the Bevy camera in sync with the `OrbitCamera` resource.
pub fn update_orbit_camera(
    mut cam_q: Query<&mut Transform, (With<RbxCamera>, Without<RbxSceneRoot>)>,
    orbit: Res<OrbitCamera>,
) {
    let (eye_b, target_b) = orbit_eye_target_b(&orbit);
    for mut t in &mut cam_q {
        *t = Transform::from_translation(eye_b).looking_at(target_b, GVec3::Y);
    }
}

