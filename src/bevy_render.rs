// ============================================================================
// bevy_render.rs — the Bevy 3D renderer for the editor
// ----------------------------------------------------------------------------
// A port of OpenRBLX's 3D viewport renderer to the Bevy engine. Bevy owns the
// window (so it gets a GPU context on Android) and renders the place geometry
// with GPU lighting; the egui editor UI is drawn on top via bevy_egui.
//
// Coordinate handling: Roblox is left-handed, Bevy is right-handed, so we flip
// the Z axis on every world vertex/normal and use two-sided materials
// (cull_mode: None) to avoid winding/backface problems.
// ============================================================================

use bevy::asset::RenderAssetUsages;
use bevy::math::Vec3 as BVec3;
use bevy::mesh::{Indices, Mesh};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use rbx_dom_weak::{types::Variant, WeakDom};
use std::collections::HashMap;
use std::path::Path;

// ----------------------------------------------------------------------------
// Math (S space, matches the Android software rasterizer)
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}
impl Vec3 {
    fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    fn add(&self, o: &Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    fn sub(&self, o: &Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    fn mul(&self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
    fn length(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    fn normalize(&self) -> Self {
        let l = self.length();
        if l > 1e-6 {
            self.mul(1.0 / l)
        } else {
            *self
        }
    }
    fn cross(&self, o: &Self) -> Self {
        Self::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct CFrame {
    pos: Vec3,
    // rotation rows = basis vectors (right, up, back)
    r00: f32, r01: f32, r02: f32,
    r10: f32, r11: f32, r12: f32,
    r20: f32, r21: f32, r22: f32,
}
impl CFrame {
    fn transform_point(&self, p: Vec3) -> Vec3 {
        Vec3::new(
            self.pos.x + self.r00 * p.x + self.r01 * p.y + self.r02 * p.z,
            self.pos.y + self.r10 * p.x + self.r11 * p.y + self.r12 * p.z,
            self.pos.z + self.r20 * p.x + self.r21 * p.y + self.r22 * p.z,
        )
    }
    fn transform_normal(&self, n: Vec3) -> Vec3 {
        Vec3::new(
            self.r00 * n.x + self.r01 * n.y + self.r02 * n.z,
            self.r10 * n.x + self.r11 * n.y + self.r12 * n.z,
            self.r20 * n.x + self.r21 * n.y + self.r22 * n.z,
        )
        .normalize()
    }
}

fn extract_cframe(inst: &rbx_dom_weak::Instance) -> CFrame {
    if let Some(Variant::CFrame(cf)) = inst
        .properties
        .get(&rbx_dom_weak::ustr("CFrame"))
        .or_else(|| inst.properties.get(&rbx_dom_weak::ustr("CoordinateFrame")))
    {
        return CFrame {
            pos: Vec3::new(cf.position.x, cf.position.y, cf.position.z),
            r00: cf.orientation.x.x, r01: cf.orientation.x.y, r02: cf.orientation.x.z,
            r10: cf.orientation.y.x, r11: cf.orientation.y.y, r12: cf.orientation.y.z,
            r20: cf.orientation.z.x, r21: cf.orientation.z.y, r22: cf.orientation.z.z,
        };
    }
    CFrame {
        pos: match inst.properties.get(&rbx_dom_weak::ustr("Position")) {
            Some(Variant::Vector3(v)) => Vec3::new(v.x, v.y, v.z),
            _ => Vec3::new(0.0, 0.0, 0.0),
        },
        r00: 1.0, r01: 0.0, r02: 0.0,
        r10: 0.0, r11: 1.0, r12: 0.0,
        r20: 0.0, r21: 0.0, r22: 1.0,
    }
}

// S -> Bevy world (Z flip).
fn s2b(p: Vec3) -> [f32; 3] {
    [p.x, p.y, -p.z]
}
fn tp(cf: &CFrame, p: Vec3) -> [f32; 3] {
    let w = cf.transform_point(p);
    s2b(w)
}
fn tn(cf: &CFrame, n: Vec3) -> [f32; 3] {
    let w = cf.transform_normal(n);
    s2b(w)
}

// ----------------------------------------------------------------------------
// Geometry triangle types
// ----------------------------------------------------------------------------

#[derive(Clone)]
struct Tri {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    tex: Option<String>,
}

#[derive(Clone)]
struct PartGeo {
    color: [f32; 3],
    alpha: f32,
    tris: Vec<Tri>,
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let n = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 1e-8 {
        [n[0] / l, n[1] / l, n[2] / l]
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn push_quad(tris: &mut Vec<Tri>, cf: &CFrame, p: [Vec3; 4], n: Vec3, uv: [[f32; 2]; 4], tex: Option<String>) {
    let a = tp(cf, p[0]);
    let b = tp(cf, p[1]);
    let c = tp(cf, p[2]);
    let d = tp(cf, p[3]);
    let nrm = tn(cf, n);
    tris.push(Tri { pos: a, normal: nrm, uv: uv[0], tex: tex.clone() });
    tris.push(Tri { pos: b, normal: nrm, uv: uv[1], tex: tex.clone() });
    tris.push(Tri { pos: c, normal: nrm, uv: uv[2], tex: tex.clone() });
    tris.push(Tri { pos: a, normal: nrm, uv: uv[0], tex: tex.clone() });
    tris.push(Tri { pos: c, normal: nrm, uv: uv[2], tex: tex.clone() });
    tris.push(Tri { pos: d, normal: nrm, uv: uv[3], tex });
}

fn push_tri(tris: &mut Vec<Tri>, cf: &CFrame, p: [Vec3; 3], n: Vec3, uv: [[f32; 2]; 3], tex: Option<String>) {
    let nrm = tn(cf, n);
    tris.push(Tri { pos: tp(cf, p[0]), normal: nrm, uv: uv[0], tex: tex.clone() });
    tris.push(Tri { pos: tp(cf, p[1]), normal: nrm, uv: uv[1], tex: tex.clone() });
    tris.push(Tri { pos: tp(cf, p[2]), normal: nrm, uv: uv[2], tex });
}

// ----------------------------------------------------------------------------
// Place loading
// ----------------------------------------------------------------------------

// ----------------------------------------------------------------------------
// Geometry extraction
// ----------------------------------------------------------------------------

fn extract_geometry(dom: &WeakDom) -> Vec<PartGeo> {
    let mut out = Vec::new();
    let mut stack = dom.root().children().to_vec();
    while let Some(r) = stack.pop() {
        let Some(inst) = dom.get_by_ref(r) else { continue };
        stack.extend(inst.children());
        let is_3d = matches!(
            inst.class.as_str(),
            "Part" | "WedgePart" | "CornerWedgePart" | "TrussPart" | "SpawnLocation" | "MeshPart" | "Seat" | "VehicleSeat" | "UnionOperation"
        );
        if is_3d {
            if let Some(g) = extract_part(dom, inst) {
                out.push(g);
            }
        }
    }
    out
}

fn extract_part(dom: &WeakDom, inst: &rbx_dom_weak::Instance) -> Option<PartGeo> {
    let cf = extract_cframe(inst);
    let size = match inst.properties.get(&rbx_dom_weak::ustr("Size")) {
        Some(Variant::Vector3(v)) => Vec3::new(v.x.max(0.1), v.y.max(0.1), v.z.max(0.1)),
        _ => Vec3::new(4.0, 1.2, 2.0),
    };
    let color = match inst.properties.get(&rbx_dom_weak::ustr("Color")) {
        Some(Variant::Color3(c)) => [c.r as f32, c.g as f32, c.b as f32],
        Some(Variant::Color3uint8(c)) => [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0],
        _ => [0.7, 0.7, 0.7],
    };
    let transparency = match inst.properties.get(&rbx_dom_weak::ustr("Transparency")) {
        Some(Variant::Float32(f)) => *f,
        Some(Variant::Float64(f)) => *f as f32,
        _ => 0.0,
    };
    let alpha = (1.0 - transparency).clamp(0.0, 1.0);
    let material_str = match inst.properties.get(&rbx_dom_weak::ustr("Material")) {
        Some(Variant::String(s)) => s.clone(),
        _ => "Plastic".into(),
    };

    // MeshPart / SpecialMesh
    let mut mesh_id: Option<String> = None;
    let mut mesh_tex: Option<String> = None;
    let mut mesh_type: Option<String> = None;
    let mut scale = Vec3::new(1.0, 1.0, 1.0);
    let mut offset = Vec3::new(0.0, 0.0, 0.0);
    if inst.class == "MeshPart" {
        if let Some(Variant::String(m)) = inst.properties.get(&rbx_dom_weak::ustr("MeshId")) {
            mesh_id = Some(m.clone());
        }
        if let Some(Variant::String(t)) = inst.properties.get(&rbx_dom_weak::ustr("TextureID")).or_else(|| inst.properties.get(&rbx_dom_weak::ustr("TextureId"))) {
            mesh_tex = Some(t.clone());
        }
    }
    for child_ref in inst.children() {
        let Some(ch) = dom.get_by_ref(*child_ref) else { continue };
        if ch.class == "SpecialMesh" || ch.class == "BlockMesh" {
            if let Some(Variant::String(m)) = ch.properties.get(&rbx_dom_weak::ustr("MeshId")) {
                mesh_id = Some(m.clone());
            }
            if let Some(Variant::String(t)) = ch.properties.get(&rbx_dom_weak::ustr("TextureId")) {
                mesh_tex = Some(t.clone());
            }
            if let Some(Variant::String(mt)) = ch.properties.get(&rbx_dom_weak::ustr("MeshType")) {
                mesh_type = Some(mt.clone());
            }
            if let Some(Variant::Vector3(sc)) = ch.properties.get(&rbx_dom_weak::ustr("Scale")) {
                scale = Vec3::new(sc.x, sc.y, sc.z);
            }
            if let Some(Variant::Vector3(o)) = ch.properties.get(&rbx_dom_weak::ustr("Offset")) {
                offset = Vec3::new(o.x, o.y, o.z);
            }
        }
    }

    // Decals
    let mut decals: HashMap<&'static str, String> = HashMap::new();
    for child_ref in inst.children() {
        let Some(ch) = dom.get_by_ref(*child_ref) else { continue };
        if ch.class == "Decal" || ch.class == "Texture" {
            let face = match ch.properties.get(&rbx_dom_weak::ustr("Face")) {
                Some(Variant::String(s)) => match s.as_str() {
                    "Top" => "Top", "Bottom" => "Bottom", "Front" => "Front", "Back" => "Back", "Left" => "Left", "Right" => "Right", _ => "Front",
                },
                Some(Variant::Int32(1)) | Some(Variant::Int64(1)) => "Top",
                Some(Variant::Int32(5)) | Some(Variant::Int64(5)) => "Front",
                _ => "Front",
            };
            if let Some(Variant::String(t)) = ch.properties.get(&rbx_dom_weak::ustr("Texture")) {
                if !t.is_empty() {
                    decals.insert(face, t.clone());
                }
            }
        }
    }

    let half = Vec3::new(size.x * 0.5 * scale.x, size.y * 0.5 * scale.y, size.z * 0.5 * scale.z);
    let mut cf2 = cf;
    let rot_off = cf.transform_point(offset).sub(&cf.pos);
    cf2.pos = cf2.pos.add(&rot_off);

    let mut tris = Vec::new();

    // Mesh part
    if let Some(ref mid) = mesh_id {
        if let Some(md) = load_mesh_local(mid) {
            let bx = (md.aabb_max[0] - md.aabb_min[0]).max(0.01);
            let by = (md.aabb_max[1] - md.aabb_min[1]).max(0.01);
            let bz = (md.aabb_max[2] - md.aabb_min[2]).max(0.01);
            let sx = (size.x * scale.x) / bx;
            let sy = (size.y * scale.y) / by;
            let sz = (size.z * scale.z) / bz;
            let tex_key = mesh_tex.clone();
            for f in &md.faces {
                if let (Some(&a), Some(&b), Some(&c)) = (
                    md.vertices.get(f[0] as usize),
                    md.vertices.get(f[1] as usize),
                    md.vertices.get(f[2] as usize),
                ) {
                    let pa = tp(&cf2, Vec3::new(a[0] * sx, a[1] * sy, a[2] * sz));
                    let pb = tp(&cf2, Vec3::new(b[0] * sx, b[1] * sy, b[2] * sz));
                    let pc = tp(&cf2, Vec3::new(c[0] * sx, c[1] * sy, c[2] * sz));
                    let n = cross(sub3(pb, pa), sub3(pc, pa));
                    let ua = md.uvs.get(f[0] as usize).copied().unwrap_or([0.0, 0.0]);
                    let ub = md.uvs.get(f[1] as usize).copied().unwrap_or([1.0, 0.0]);
                    let uc = md.uvs.get(f[2] as usize).copied().unwrap_or([0.0, 1.0]);
                    let t = tex_key.clone();
                    tris.push(Tri { pos: pa, normal: n, uv: ua, tex: t.clone() });
                    tris.push(Tri { pos: pb, normal: n, uv: ub, tex: t.clone() });
                    tris.push(Tri { pos: pc, normal: n, uv: uc, tex: t });
                }
            }
            if tris.is_empty() {
                return None;
            }
            return Some(PartGeo { color, alpha, tris });
        }
    }

    let shape = match mesh_type.as_deref() {
        Some("Sphere") | Some("Head") => "Ball",
        Some("Cylinder") => "Cylinder",
        Some("Wedge") => "Wedge",
        _ => match inst.class.as_str() {
            "WedgePart" => "Wedge",
            "CornerWedgePart" => "CornerWedge",
            "TrussPart" => "Truss",
            _ => {
                if inst.name.to_lowercase().contains("ball") || inst.name.to_lowercase().contains("sphere") {
                    "Ball"
                } else if let Some(Variant::String(s)) = inst.properties.get(&rbx_dom_weak::ustr("Shape")) {
                    match s.as_str() {
                        "Ball" => "Ball",
                        "Cylinder" => "Cylinder",
                        "Block" => "Block",
                        "Wedge" => "Wedge",
                        _ => "Block",
                    }
                } else {
                    "Block"
                }
            }
        },
    };

    match shape {
        "Ball" => build_ball(&mut tris, &cf2, half),
        "Cylinder" => build_cylinder(&mut tris, &cf2, half),
        "Wedge" => build_wedge(&mut tris, &cf2, half),
        "CornerWedge" => build_corner_wedge(&mut tris, &cf2, half),
        "Truss" => build_truss(&mut tris, &cf2, half),
        _ => build_block(&mut tris, &cf2, half, material_str.as_str(), &decals),
    }

    if tris.is_empty() {
        return None;
    }
    Some(PartGeo { color, alpha, tris })
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

// --- primitive builders (same shapes as the Android app / OpenRBLX) ---

fn build_block(tris: &mut Vec<Tri>, cf: &CFrame, half: Vec3, material: &str, decals: &HashMap<&'static str, String>) {
    let h = half;
    let v = [
        Vec3::new(-h.x, -h.y, -h.z), Vec3::new(h.x, -h.y, -h.z), Vec3::new(h.x, -h.y, h.z), Vec3::new(-h.x, -h.y, h.z),
        Vec3::new(-h.x, h.y, -h.z), Vec3::new(h.x, h.y, -h.z), Vec3::new(h.x, h.y, h.z), Vec3::new(-h.x, h.y, h.z),
    ];
    let mat_tex = match material {
        "Brick" => Some("__brick".into()),
        "Wood" | "WoodPlanks" => Some("__wood_planks".into()),
        "Concrete" | "Slate" => Some("__concrete".into()),
        "Grass" => Some("__grass".into()),
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
    for (face, idx, n, is_top) in faces {
        let tex = if let Some(d) = decals.get(face) {
            Some(d.clone())
        } else if is_top {
            mat_tex.clone()
        } else {
            mat_tex.clone()
        };
        push_quad(tris, cf, [v[idx[0]], v[idx[1]], v[idx[2]], v[idx[3]]], n, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], tex);
    }
}

fn build_ball(tris: &mut Vec<Tri>, cf: &CFrame, half: Vec3) {
    let lats = 8;
    let lons = 12;
    let r = half.x.min(half.y).min(half.z);
    for i in 0..lats {
        let la0 = std::f32::consts::PI * (-0.5 + i as f32 / lats as f32);
        let la1 = std::f32::consts::PI * (-0.5 + (i + 1) as f32 / lats as f32);
        let z0 = la0.sin() * r;
        let z1 = la1.sin() * r;
        let r0 = la0.cos() * r;
        let r1 = la1.cos() * r;
        for j in 0..lons {
            let a0 = 2.0 * std::f32::consts::PI * j as f32 / lons as f32;
            let a1 = 2.0 * std::f32::consts::PI * (j + 1) as f32 / lons as f32;
            let x0 = a0.cos();
            let y0 = a0.sin();
            let x1 = a1.cos();
            let y1 = a1.sin();
            let n = Vec3::new((x0 + x1) * 0.5 * (r0 + r1) * 0.5, (y0 + y1) * 0.5 * (r0 + r1) * 0.5, (z0 + z1) * 0.5).normalize();
            push_tri(tris, cf, [Vec3::new(x0 * r0, y0 * r0, z0), Vec3::new(x1 * r0, y1 * r0, z0), Vec3::new(x1 * r1, y1 * r1, z1)], n, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], None);
            push_tri(tris, cf, [Vec3::new(x0 * r0, y0 * r0, z0), Vec3::new(x1 * r1, y1 * r1, z1), Vec3::new(x0 * r1, y0 * r1, z1)], n, [[0.0, 0.0], [1.0, 1.0], [0.0, 1.0]], None);
        }
    }
}

fn build_cylinder(tris: &mut Vec<Tri>, cf: &CFrame, half: Vec3) {
    let segments = 12;
    let r = half.y.min(half.z);
    let mut front = Vec::new();
    let mut back = Vec::new();
    for i in 0..segments {
        let t = i as f32 / segments as f32 * 2.0 * std::f32::consts::PI;
        let t2 = (i + 1) as f32 / segments as f32 * 2.0 * std::f32::consts::PI;
        let (sy1, sz1) = t.sin_cos();
        let (sy2, sz2) = t2.sin_cos();
        let n = Vec3::new(0.0, (sy1 + sy2) * 0.5, (sz1 + sz2) * 0.5).normalize();
        push_quad(tris, cf, [Vec3::new(-half.x, sy1 * r, sz1 * r), Vec3::new(half.x, sy1 * r, sz1 * r), Vec3::new(half.x, sy2 * r, sz2 * r), Vec3::new(-half.x, sy2 * r, sz2 * r)], n, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], None);
        front.push(Vec3::new(half.x, sy1 * r, sz1 * r));
        back.push(Vec3::new(-half.x, sy1 * r, sz1 * r));
    }
    let fc = Vec3::new(half.x, 0.0, 0.0);
    let bc = Vec3::new(-half.x, 0.0, 0.0);
    for i in 0..segments {
        let n2 = (i + 1) % segments;
        push_tri(tris, cf, [fc, front[i], front[n2]], Vec3::new(1.0, 0.0, 0.0), [[0.5, 0.5], [0.0, 0.0], [1.0, 0.0]], None);
        push_tri(tris, cf, [bc, back[n2], back[i]], Vec3::new(-1.0, 0.0, 0.0), [[0.5, 0.5], [1.0, 0.0], [0.0, 0.0]], None);
    }
}

fn build_wedge(tris: &mut Vec<Tri>, cf: &CFrame, half: Vec3) {
    let h = half;
    let v0 = Vec3::new(-h.x, -h.y, -h.z);
    let v1 = Vec3::new(h.x, -h.y, -h.z);
    let v2 = Vec3::new(h.x, -h.y, h.z);
    let v3 = Vec3::new(-h.x, -h.y, h.z);
    let v4 = Vec3::new(-h.x, h.y, -h.z);
    let v5 = Vec3::new(h.x, h.y, -h.z);
    push_quad(tris, cf, [v0, v1, v2, v3], Vec3::new(0.0, -1.0, 0.0), [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], None);
    push_quad(tris, cf, [v1, v0, v4, v5], Vec3::new(0.0, 0.0, -1.0), [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], None);
    let ramp = Vec3::new(0.0, h.z, h.y).normalize();
    push_quad(tris, cf, [v3, v2, v5, v4], ramp, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], None);
    push_tri(tris, cf, [v0, v3, v4], Vec3::new(-1.0, 0.0, 0.0), [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]], None);
    push_tri(tris, cf, [v2, v1, v5], Vec3::new(1.0, 0.0, 0.0), [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], None);
}

fn build_corner_wedge(tris: &mut Vec<Tri>, cf: &CFrame, half: Vec3) {
    let h = half;
    let b0 = Vec3::new(-h.x, -h.y, -h.z);
    let b1 = Vec3::new(h.x, -h.y, -h.z);
    let b2 = Vec3::new(h.x, -h.y, h.z);
    let b3 = Vec3::new(-h.x, -h.y, h.z);
    let top = Vec3::new(-h.x, h.y, -h.z);
    push_quad(tris, cf, [b0, b1, b2, b3], Vec3::new(0.0, -1.0, 0.0), [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], None);
    push_tri(tris, cf, [b1, b0, top], Vec3::new(0.0, 0.0, -1.0), [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]], None);
    push_tri(tris, cf, [b0, b3, top], Vec3::new(-1.0, 0.0, 0.0), [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]], None);
    let sloped = Vec3::new(h.y, h.x, h.z).normalize();
    push_tri(tris, cf, [b3, b2, top], sloped, [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]], None);
    push_tri(tris, cf, [b2, b1, top], sloped, [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]], None);
}

fn build_truss(tris: &mut Vec<Tri>, cf: &CFrame, half: Vec3) {
    let h = half;
    let v = [
        Vec3::new(-h.x, -h.y, -h.z), Vec3::new(h.x, -h.y, -h.z), Vec3::new(h.x, -h.y, h.z), Vec3::new(-h.x, -h.y, h.z),
        Vec3::new(-h.x, h.y, -h.z), Vec3::new(h.x, h.y, -h.z), Vec3::new(h.x, h.y, h.z), Vec3::new(-h.x, h.y, h.z),
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
// Local asset loading (reads ../asset/{id}.mesh / .png / .jpg)
// ----------------------------------------------------------------------------

struct MeshData {
    vertices: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    faces: Vec<[u32; 3]>,
    aabb_min: [f32; 3],
    aabb_max: [f32; 3],
}

fn asset_dir() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR = .../repo/renderer; assets live at repo/asset.
    let d = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    Path::new(&d)
        .parent()
        .unwrap_or(Path::new("."))
        .join("asset")
}

fn asset_path(id: &str, exts: &[&str]) -> Option<std::path::PathBuf> {
    let dir = asset_dir();
    for e in exts {
        let p = dir.join(format!("{id}.{e}"));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn extract_asset_id(s: &str) -> Option<String> {
    let mut chars = s.chars().rev().peekable();
    let mut digits = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            digits.push(c);
            chars.next();
        } else {
            break;
        }
    }
    if digits.is_empty() {
        return None;
    }
    Some(digits.chars().rev().collect())
}

fn load_mesh_local(id_or_path: &str) -> Option<MeshData> {
    let id = extract_asset_id(id_or_path)?;
    let path = asset_path(&id, &["mesh"])?;
    let bytes = std::fs::read(path).ok()?;
    // Very small Roblox .mesh binary parser (v1.00/v1.01).
    let version = String::from_utf8_lossy(&bytes[..8.min(bytes.len())]).into_owned();
    let mut sizeof_vertex = 0usize;
    let mut sizeof_face = 0usize;
    if version.starts_with("version 1.") {
        // header size + counts
        let header_start = 8;
        if bytes.len() < header_start + 16 {
            return None;
        }
        if version.starts_with("version 1.01") || version.starts_with("version 1.00") {
            sizeof_vertex = 32;
            sizeof_face = 12;
            let nv = u32::from_le_bytes([bytes[header_start + 4], bytes[header_start + 5], bytes[header_start + 6], bytes[header_start + 7]]) as usize;
            let nf = u32::from_le_bytes([bytes[header_start + 8], bytes[header_start + 9], bytes[header_start + 10], bytes[header_start + 11]]) as usize;
            let v_start = header_start + 16;
            let mut vertices = Vec::new();
            let mut uvs = Vec::new();
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for i in 0..nv {
                let o = v_start + i * sizeof_vertex;
                if o + 32 > bytes.len() {
                    break;
                }
                let px = f32::from_le_bytes(bytes[o..o + 4].try_into().ok()?);
                let py = f32::from_le_bytes(bytes[o + 4..o + 8].try_into().ok()?);
                let pz = f32::from_le_bytes(bytes[o + 8..o + 12].try_into().ok()?);
                let u = f32::from_le_bytes(bytes[o + 24..o + 28].try_into().ok()?);
                let v = f32::from_le_bytes(bytes[o + 28..o + 32].try_into().ok()?);
                min[0] = min[0].min(px);
                min[1] = min[1].min(py);
                min[2] = min[2].min(pz);
                max[0] = max[0].max(px);
                max[1] = max[1].max(py);
                max[2] = max[2].max(pz);
                vertices.push([px, py, pz]);
                uvs.push([u, 1.0 - v]);
            }
            let f_start = v_start + nv * sizeof_vertex;
            let mut faces = Vec::new();
            for i in 0..nf {
                let o = f_start + i * sizeof_face;
                if o + 12 > bytes.len() {
                    break;
                }
                let a = u32::from_le_bytes(bytes[o..o + 4].try_into().ok()?);
                let b = u32::from_le_bytes(bytes[o + 4..o + 8].try_into().ok()?);
                let c = u32::from_le_bytes(bytes[o + 8..o + 12].try_into().ok()?);
                faces.push([a, b, c]);
            }
            return Some(MeshData { vertices, uvs, faces, aabb_min: min, aabb_max: max });
        }
    }
    None
}

// ----------------------------------------------------------------------------
// Editor integration
// ----------------------------------------------------------------------------

/// Marker for the 3D camera (so the orbit update only affects the viewport cam).
#[derive(Component)]
pub struct RbxCamera;

/// Marker for the scene root (so the whole scene can be despawned on reload).
#[derive(Component)]
pub struct RbxSceneRoot;

/// Orbit camera state, driven by egui drags on the 3D viewport tab.
#[derive(Resource, Clone, Copy, Debug)]
pub struct OrbitCam {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: [f32; 3],
}

impl Default for OrbitCam {
    fn default() -> Self {
        Self { yaw: 0.785, pitch: 0.45, dist: 65.0, target: [0.0, 6.0, 0.0] }
    }
}

/// Spawn the viewport camera + sun light. The camera is tagged `RbxCamera`.
pub fn spawn_camera_and_light(mut commands: Commands) {
    let cam = OrbitCam::default();
    let (eye_b, target_b) = orbit_eye_target(&cam);
    commands.spawn((
        Camera3d::default(),
        // CRITICAL for mobile performance: MSAA is on by default (4×). At the
        // phone's native resolution (e.g. 1440×3200) that is a huge fill-rate
        // cost for a trivial scene, which is what caused the ~20 fps. Turning
        // MSAA off is the single biggest perf win on Android.
        Msaa::Off,
        // Explicit near/far planes: a large far plane lets you zoom way out to
        // see the whole place, and a small near plane means you can go right up
        // to / inside a part without the geometry vanishing.
        Projection::Perspective(PerspectiveProjection {
            near: 0.1,
            far: 20000.0,
            fov: 60f32.to_radians(),
            ..default()
        }),
        Transform::from_translation(eye_b).looking_at(target_b, BVec3::Y),
        RbxCamera,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 30_000.0,
            // Shadows OFF: the shadow pass re-renders the whole scene every
            // frame and was a major cause of the low frame rate.
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(60.0, 90.0, -40.0).looking_at(BVec3::ZERO, BVec3::Y),
    ));
}

/// Convert S-space orbit state to Bevy eye/target (Z-flip).
pub fn orbit_eye_target(cam: &OrbitCam) -> (BVec3, BVec3) {
    let (sp, cp) = cam.pitch.sin_cos();
    let (sy, cy) = cam.yaw.sin_cos();
    let eye_s = [
        cam.target[0] + cam.dist * cp * sy,
        cam.target[1] + cam.dist * sp,
        cam.target[2] + cam.dist * cp * cy,
    ];
    let eye = BVec3::new(eye_s[0], eye_s[1], -eye_s[2]);
    let target = BVec3::new(cam.target[0], cam.target[1], -cam.target[2]);
    (eye, target)
}

/// Despawn the old scene and build fresh meshes for `dom`. Called whenever the
/// opened place changes.
pub fn rebuild_scene(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    _images: &mut Assets<Image>,
    dom: &WeakDom,
) {
    let parts = extract_geometry(dom);

    let root = commands.spawn(RbxSceneRoot).id();

    // Bucket by (color, alpha) ONLY — ignore per-face texture ids. Textured
    // materials (studs, decals, mesh textures) are skipped for now because on
    // Android the image assets aren't reliably GPU-ready, which makes Bevy
    // render those materials as magenta/purple. Rendering solid colours both
    // fixes the purple AND matches OpenRBLX's flat-coloured look, and merging
    // by colour collapses many materials into few, cutting draw calls.
    type Key = ([u8; 3], u8);
    let mut buckets: HashMap<Key, Vec<Tri>> = HashMap::new();
    for p in &parts {
        let ck = [
            (p.color[0] * 255.0).round() as u8,
            (p.color[1] * 255.0).round() as u8,
            (p.color[2] * 255.0).round() as u8,
        ];
        let ak = (p.alpha * 255.0).round() as u8;
        for t in &p.tris {
            buckets.entry((ck, ak)).or_default().push(t.clone());
        }
    }
    let draw_call_count = buckets.len();

    for ((ck, ak), tris) in buckets {
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut vertex_colors = Vec::new();
        let mut indices = Vec::new();
        let mut idx = 0u32;
        // Fixed light direction (from origin toward the sun at (60,90,-40)),
        // used to bake simple Lambert shading into per-vertex colors. This
        // gives the lit look of the reference WITHOUT relying on Bevy's PBR
        // shader, which was rendering everything purple on this device.
        let light = BVec3::new(60.0, 90.0, -40.0).normalize();
        let base_rgb = [ck[0] as f32 / 255.0, ck[1] as f32 / 255.0, ck[2] as f32 / 255.0];
        for t in &tris {
            let n = BVec3::new(t.normal[0], t.normal[1], t.normal[2]);
            // Lambert with an ambient floor so back/side faces aren't pure black.
            let lambert = n.dot(light).max(0.0) * 0.7 + 0.3;
            let c = [
                (base_rgb[0] * lambert).min(1.0),
                (base_rgb[1] * lambert).min(1.0),
                (base_rgb[2] * lambert).min(1.0),
            ];
            positions.push(t.pos);
            normals.push(t.normal);
            uvs.push(t.uv);
            vertex_colors.push([c[0], c[1], c[2], 1.0]);
            indices.push(idx);
            indices.push(idx + 1);
            indices.push(idx + 2);
            idx += 3;
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vertex_colors);
        mesh.insert_indices(Indices::U32(indices));
        let mh = meshes.add(mesh);

        let alpha = ak as f32 / 255.0;
        // unlit shader (robust — no PBR purple) + white base color so the baked
        // per-vertex Lambert colors show through. Alpha comes from base_color.
        let mth = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, alpha),
            unlit: true,
            cull_mode: None,
            ..default()
        });

        commands.entity(root).with_children(|parent| {
            parent.spawn((Mesh3d(mh), MeshMaterial3d(mth), Transform::IDENTITY));
        });
    }

    log::info!("Bevy: rendered {} parts, {} draw calls", parts.len(), draw_call_count);
}

/// Keep the viewport camera in sync with the `OrbitCam` resource each frame.
pub fn update_camera(mut q: Query<&mut Transform, With<RbxCamera>>, cam: Res<OrbitCam>) {
    let (eye_b, target_b) = orbit_eye_target(&cam);
    for mut t in &mut q {
        *t = Transform::from_translation(eye_b).looking_at(target_b, BVec3::Y);
    }
}
