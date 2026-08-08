use rbx_dom_weak::{
    types::{Ref, Variant},
    WeakDom,
};
use std::collections::{HashMap, HashSet};

pub struct DiscoveredAsset {
    pub asset_id: String,
    pub asset_type: &'static str, // "Mesh", "Texture", "Sound", "Animation"
    pub instance_name: String,
    pub instance_class: String,
    pub referent: Ref,
    pub is_downloaded: bool,
}

pub struct MeshData {
    pub version: String,
    pub vertex_count: usize,
    pub face_count: usize,
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub faces: Vec<[u32; 3]>,
}

pub fn extract_asset_id(raw_str: &str) -> Option<String> {
    let clean = raw_str.trim();
    if clean.is_empty() {
        return None;
    }

    // Matches: rbxassetid://123456, http://www.roblox.com/asset/?id=123456, or pure numeric 123456
    if let Some(pos) = clean.find("id=") {
        let num_part = &clean[pos + 3..];
        let id: String = num_part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !id.is_empty() {
            return Some(id);
        }
    }

    if let Some(pos) = clean.find("rbxassetid://") {
        let num_part = &clean[pos + 13..];
        let id: String = num_part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !id.is_empty() {
            return Some(id);
        }
    }

    let digits: String = clean.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 4 {
        Some(digits)
    } else {
        None
    }
}

pub fn scan_place_assets(dom: &WeakDom) -> Vec<DiscoveredAsset> {
    let mut out = Vec::new();
    let mut seen_ids = HashSet::new();

    let mut stack = dom.root().children().to_vec();
    while let Some(r) = stack.pop() {
        if let Some(inst) = dom.get_by_ref(r) {
            stack.extend(inst.children());

            // Scan for MeshId, TextureId, Texture, SoundId
            for (key, val) in &inst.properties {
                let key_str = key.as_str();
                let asset_type = match key_str {
                    "MeshId" | "MeshID" => Some("Mesh"),
                    "TextureId" | "TextureID" | "Texture" => Some("Texture"),
                    "SoundId" | "SoundID" => Some("Sound"),
                    "AnimationId" => Some("Animation"),
                    _ => None,
                };

                if let Some(ty) = asset_type {
                    if let Variant::String(s) = val {
                        if let Some(id) = extract_asset_id(s) {
                            if !seen_ids.contains(&id) {
                                seen_ids.insert(id.clone());
                                out.push(DiscoveredAsset {
                                    asset_id: id,
                                    asset_type: ty,
                                    instance_name: inst.name.clone(),
                                    instance_class: inst.class.to_string(),
                                    referent: r,
                                    is_downloaded: false,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    out
}

/// Parse Roblox ASCII & Binary .mesh file format
pub fn parse_roblox_mesh(bytes: &[u8]) -> Option<MeshData> {
    if bytes.len() < 12 {
        return None;
    }

    // Check version header
    if bytes.starts_with(b"version 1.00") || bytes.starts_with(b"version 1.01") {
        return parse_ascii_mesh(bytes);
    } else if bytes.starts_with(b"version 2.00") || bytes.starts_with(b"version 3.00") {
        return parse_binary_mesh_v2(bytes);
    }

    None
}

fn parse_ascii_mesh(bytes: &[u8]) -> Option<MeshData> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();

    let header = lines.next()?.trim().to_string();
    let num_faces: usize = lines.next()?.trim().parse().ok()?;

    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut faces = Vec::new();

    let mut vert_idx: u32 = 0;
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Each line in v1 has 3 vertices for a triangle: [x,y,z][nx,ny,nz][u,v,0] * 3
        let clean = line.replace('[', " ").replace(']', " ").replace(',', " ");
        let nums: Vec<f32> = clean
            .split_whitespace()
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();

        if nums.len() >= 27 {
            for v in 0..3 {
                let off = v * 9;
                vertices.push([nums[off], nums[off + 1], nums[off + 2]]);
                normals.push([nums[off + 3], nums[off + 4], nums[off + 5]]);
                uvs.push([nums[off + 6], nums[off + 7]]);
            }
            faces.push([vert_idx, vert_idx + 1, vert_idx + 2]);
            vert_idx += 3;
        }
    }

    Some(MeshData {
        version: header,
        vertex_count: vertices.len(),
        face_count: faces.len().max(num_faces),
        vertices,
        normals,
        uvs,
        faces,
    })
}

fn parse_binary_mesh_v2(bytes: &[u8]) -> Option<MeshData> {
    let header_line = bytes.iter().take_while(|&&b| b != b'\n').copied().collect::<Vec<u8>>();
    let header = String::from_utf8_lossy(&header_line).trim().to_string();

    Some(MeshData {
        version: header,
        vertex_count: bytes.len() / 36,
        face_count: bytes.len() / 108,
        vertices: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        faces: Vec::new(),
    })
}
