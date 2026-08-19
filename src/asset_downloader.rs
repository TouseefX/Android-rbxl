use rbx_dom_weak::{
    types::{ContentType, Ref, Variant},
    WeakDom,
};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct DiscoveredAsset {
    pub asset_id: String,
    pub asset_type: &'static str,
    pub instance_name: String,
    pub instance_class: String,
    pub referent: Ref,
    pub is_downloaded: bool,
}

#[derive(Debug, Clone)]
pub struct MeshData {
    pub version: String,
    pub vertex_count: usize,
    pub face_count: usize,
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub faces: Vec<[u32; 3]>,
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

static MESH_CACHE: OnceLock<Mutex<HashMap<String, MeshData>>> = OnceLock::new();
static IMAGE_CACHE: OnceLock<Mutex<HashMap<String, Arc<DecodedImage>>>> = OnceLock::new();
/// Raw bytes for audio (ogg/mp3) and any other asset that doesn't have a
/// specialized decoder. Keyed by the same `rbxassetid://<id>` string used by
/// meshes/images so that callers can look up any downloaded asset by id.
static RAW_CACHE: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();

pub fn mesh_cache() -> &'static Mutex<HashMap<String, MeshData>> {
    MESH_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn image_cache() -> &'static Mutex<HashMap<String, Arc<DecodedImage>>> {
    IMAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn raw_cache() -> &'static Mutex<HashMap<String, Vec<u8>>> {
    RAW_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn get_cached_raw(id: &str) -> Option<Vec<u8>> {
    raw_cache().lock().ok()?.get(id).cloned()
}

pub fn store_cached_raw(id: String, bytes: Vec<u8>) {
    if let Ok(mut c) = raw_cache().lock() {
        c.insert(id, bytes);
    }
}

pub fn get_builtin_mesh_bytes(id_or_path: &str) -> Option<&'static [u8]> {
    let clean = id_or_path.trim();
    if clean.contains("sword.mesh") {
        return Some(include_bytes!("../asset/101840172.mesh"));
    }
    let id = extract_asset_id(clean).unwrap_or_else(|| clean.to_string());
    match id.as_str() {
        "101840172" => Some(include_bytes!("../asset/101840172.mesh")),
        "10470609" => Some(include_bytes!("../asset/10470609.mesh")),
        "1091940" => Some(include_bytes!("../asset/1091940.mesh")),
        "1136139" => Some(include_bytes!("../asset/1136139.mesh")),
        "115289510" => Some(include_bytes!("../asset/115289510.mesh")),
        "115296503" => Some(include_bytes!("../asset/115296503.mesh")),
        "115955313" => Some(include_bytes!("../asset/115955313.mesh")),
        "116439976" => Some(include_bytes!("../asset/116439976.mesh")),
        "11821197" => Some(include_bytes!("../asset/11821197.mesh")),
        "119875584" => Some(include_bytes!("../asset/119875584.mesh")),
        "12212520" => Some(include_bytes!("../asset/12212520.mesh")),
        "123115084" => Some(include_bytes!("../asset/123115084.mesh")),
        "1290033" => Some(include_bytes!("../asset/1290033.mesh")),
        "13319240" => Some(include_bytes!("../asset/13319240.mesh")),
        "13425802" => Some(include_bytes!("../asset/13425802.mesh")),
        "14655367" => Some(include_bytes!("../asset/14655367.mesh")),
        "147831825" => Some(include_bytes!("../asset/147831825.mesh")),
        "15952512" => Some(include_bytes!("../asset/15952512.mesh")),
        "16606212" => Some(include_bytes!("../asset/16606212.mesh")),
        "16659363" => Some(include_bytes!("../asset/16659363.mesh")),
        "17230208" => Some(include_bytes!("../asset/17230208.mesh")),
        "19059116" => Some(include_bytes!("../asset/19059116.mesh")),
        "192488915" => Some(include_bytes!("../asset/192488915.mesh")),
        "212302951" => Some(include_bytes!("../asset/212302951.mesh")),
        "25078175" => Some(include_bytes!("../asset/25078175.mesh")),
        "25096532" => Some(include_bytes!("../asset/25096532.mesh")),
        "27039535" => Some(include_bytes!("../asset/27039535.mesh")),
        "27787143" => Some(include_bytes!("../asset/27787143.mesh")),
        "28501599" => Some(include_bytes!("../asset/28501599.mesh")),
        "29515975" => Some(include_bytes!("../asset/29515975.mesh")),
        "31183234" => Some(include_bytes!("../asset/31183234.mesh")),
        "3270017" => Some(include_bytes!("../asset/3270017.mesh")),
        "36365830" => Some(include_bytes!("../asset/36365830.mesh")),
        "41434307" => Some(include_bytes!("../asset/41434307.mesh")),
        "41707557" => Some(include_bytes!("../asset/41707557.mesh")),
        "54031359" => Some(include_bytes!("../asset/54031359.mesh")),
        "57718450" => Some(include_bytes!("../asset/57718450.mesh")),
        "84313478" => Some(include_bytes!("../asset/84313478.mesh")),
        "84313555" => Some(include_bytes!("../asset/84313555.mesh")),
        _ => None,
    }
}

pub fn get_builtin_asset(id_or_path: &str) -> Option<&'static [u8]> {
    get_builtin_mesh_bytes(id_or_path).or_else(|| get_builtin_image_bytes(id_or_path))
}

pub fn get_builtin_image_bytes(id_or_path: &str) -> Option<&'static [u8]> {

    let clean = id_or_path.trim();
    if clean.contains("null_plainsky512_bk") || clean.ends_with("sky512_bk.jpg") || clean.ends_with("null_plainsky512_bk.jpg") { return Some(include_bytes!("../content/sky/null_plainsky512_bk.jpg")); }
    if clean.contains("null_plainsky512_dn") || clean.ends_with("sky512_dn.jpg") || clean.ends_with("null_plainsky512_dn.jpg") { return Some(include_bytes!("../content/sky/null_plainsky512_dn.jpg")); }
    if clean.contains("null_plainsky512_ft") || clean.ends_with("sky512_ft.jpg") || clean.ends_with("null_plainsky512_ft.jpg") { return Some(include_bytes!("../content/sky/null_plainsky512_ft.jpg")); }
    if clean.contains("null_plainsky512_lf") || clean.ends_with("sky512_lf.jpg") || clean.ends_with("null_plainsky512_lf.jpg") { return Some(include_bytes!("../content/sky/null_plainsky512_lf.jpg")); }
    if clean.contains("null_plainsky512_rt") || clean.ends_with("sky512_rt.jpg") || clean.ends_with("null_plainsky512_rt.jpg") { return Some(include_bytes!("../content/sky/null_plainsky512_rt.jpg")); }
    if clean.contains("null_plainsky512_up") || clean.ends_with("sky512_up.jpg") || clean.ends_with("null_plainsky512_up.jpg") { return Some(include_bytes!("../content/sky/null_plainsky512_up.jpg")); }

    if clean.contains("SpawnLocation") || clean.contains("spawnlocation") { return Some(include_bytes!("../content/Textures/SpawnLocation.png")); }
    if clean.contains("ArrowFarCursor") { return Some(include_bytes!("../content/Textures/ArrowFarCursor.png")); }
    if clean.contains("ArrowCursor") { return Some(include_bytes!("../content/Textures/ArrowCursor.png")); }
    if clean.contains("ImageError") { return Some(include_bytes!("../content/Textures/ImageError.png")); }

    let id = extract_asset_id(clean).unwrap_or_else(|| clean.to_string());
    match id.as_str() {
        "10055744" => Some(include_bytes!("../asset/10055744.png")),
        "1008743" => Some(include_bytes!("../asset/1008743.png")),
        "1008744" => Some(include_bytes!("../asset/1008744.png")),
        "1008745" => Some(include_bytes!("../asset/1008745.png")),
        "1008748" => Some(include_bytes!("../asset/1008748.png")),
        "10124534" => Some(include_bytes!("../asset/10124534.png")),
        "1013849" => Some(include_bytes!("../asset/1013849.png")),
        "1013850" => Some(include_bytes!("../asset/1013850.png")),
        "1013851" => Some(include_bytes!("../asset/1013851.png")),
        "1013852" => Some(include_bytes!("../asset/1013852.png")),
        "1013853" => Some(include_bytes!("../asset/1013853.png")),
        "1013854" => Some(include_bytes!("../asset/1013854.png")),
        "101840086" => Some(include_bytes!("../asset/101840086.png")),
        "10470600" => Some(include_bytes!("../asset/10470600.png")),
        "10759411" => Some(include_bytes!("../asset/10759411.png")),
        "107706048" => Some(include_bytes!("../asset/107706048.png")),
        "1091942" => Some(include_bytes!("../asset/1091942.png")),
        "1136146" => Some(include_bytes!("../asset/1136146.png")),
        "1136349" => Some(include_bytes!("../asset/1136349.png")),
        "1136350" => Some(include_bytes!("../asset/1136350.png")),
        "1138386" => Some(include_bytes!("../asset/1138386.png")),
        "1138387" => Some(include_bytes!("../asset/1138387.png")),
        "1139750" => Some(include_bytes!("../asset/1139750.png")),
        "1139751" => Some(include_bytes!("../asset/1139751.png")),
        "1139752" => Some(include_bytes!("../asset/1139752.png")),
        "1140961" => Some(include_bytes!("../asset/1140961.png")),
        "1140964" => Some(include_bytes!("../asset/1140964.png")),
        "1143109" => Some(include_bytes!("../asset/1143109.png")),
        "1143110" => Some(include_bytes!("../asset/1143110.png")),
        "115340918" => Some(include_bytes!("../asset/115340918.png")),
        "115955343" => Some(include_bytes!("../asset/115955343.png")),
        "116440028" => Some(include_bytes!("../asset/116440028.png")),
        "116620938" => Some(include_bytes!("../asset/116620938.png")),
        "116620941" => Some(include_bytes!("../asset/116620941.png")),
        "1181642" => Some(include_bytes!("../asset/1181642.png")),
        "11820196" => Some(include_bytes!("../asset/11820196.png")),
        "118869704" => Some(include_bytes!("../asset/118869704.png")),
        "1193159" => Some(include_bytes!("../asset/1193159.png")),
        "119364093" => Some(include_bytes!("../asset/119364093.png")),
        "119875721" => Some(include_bytes!("../asset/119875721.png")),
        "123115105" => Some(include_bytes!("../asset/123115105.png")),
        "1239456" => Some(include_bytes!("../asset/1239456.png")),
        "13319242" => Some(include_bytes!("../asset/13319242.png")),
        "13425822" => Some(include_bytes!("../asset/13425822.png")),
        "143675742" => Some(include_bytes!("../asset/143675742.png")),
        "14655345" => Some(include_bytes!("../asset/14655345.png")),
        "146872602" => Some(include_bytes!("../asset/146872602.png")),
        "147037195" => Some(include_bytes!("../asset/147037195.png")),
        "147831861" => Some(include_bytes!("../asset/147831861.png")),
        "15952494" => Some(include_bytes!("../asset/15952494.png")),
        "161240005" => Some(include_bytes!("../asset/161240005.png")),
        "165443860" => Some(include_bytes!("../asset/165443860.png")),
        "165853672" => Some(include_bytes!("../asset/165853672.png")),
        "16606141" => Some(include_bytes!("../asset/16606141.png")),
        "16659355" => Some(include_bytes!("../asset/16659355.png")),
        "16922886" => Some(include_bytes!("../asset/16922886.png")),
        "17230185" => Some(include_bytes!("../asset/17230185.png")),
        "173781040" => Some(include_bytes!("../asset/173781040.png")),
        "178434732" => Some(include_bytes!("../asset/178434732.png")),
        "19059111" => Some(include_bytes!("../asset/19059111.png")),
        "192488947" => Some(include_bytes!("../asset/192488947.png")),
        "212303049" => Some(include_bytes!("../asset/212303049.png")),
        "2204142" => Some(include_bytes!("../asset/2204142.png")),
        "243848567" => Some(include_bytes!("../asset/243848567.png")),
        "25077555" => Some(include_bytes!("../asset/25077555.png")),
        "25095762" => Some(include_bytes!("../asset/25095762.png")),
        "258209444" => Some(include_bytes!("../asset/258209444.png")),
        "27039641" => Some(include_bytes!("../asset/27039641.png")),
        "27787168" => Some(include_bytes!("../asset/27787168.png")),
        "28501623" => Some(include_bytes!("../asset/28501623.png")),
        "2861779" => Some(include_bytes!("../asset/2861779.png")),
        "28872862" => Some(include_bytes!("../asset/28872862.png")),
        "29515949" => Some(include_bytes!("../asset/29515949.png")),
        "31183303" => Some(include_bytes!("../asset/31183303.png")),
        "324896283" => Some(include_bytes!("../asset/324896283.png")),
        "324896348" => Some(include_bytes!("../asset/324896348.png")),
        "331852588" => Some(include_bytes!("../asset/331852588.png")),
        "332043631" => Some(include_bytes!("../asset/332043631.png")),
        "336946240" => Some(include_bytes!("../asset/336946240.png")),
        "345936256" => Some(include_bytes!("../asset/345936256.png")),
        "345988271" => Some(include_bytes!("../asset/345988271.png")),
        "345995058" => Some(include_bytes!("../asset/345995058.png")),
        "345995475" => Some(include_bytes!("../asset/345995475.png")),
        "346001941" => Some(include_bytes!("../asset/346001941.png")),
        "36365793" => Some(include_bytes!("../asset/36365793.png")),
        "365603813" => Some(include_bytes!("../asset/365603813.png")),
        "367726197" => Some(include_bytes!("../asset/367726197.png")),
        "37065186" => Some(include_bytes!("../asset/37065186.png")),
        "41385900" => Some(include_bytes!("../asset/41385900.png")),
        "41708405" => Some(include_bytes!("../asset/41708405.png")),
        "4639675" => Some(include_bytes!("../asset/4639675.png")),
        "5009999" => Some(include_bytes!("../asset/5009999.png")),
        "51498309" => Some(include_bytes!("../asset/51498309.png")),
        "54031415" => Some(include_bytes!("../asset/54031415.png")),
        "57718359" => Some(include_bytes!("../asset/57718359.png")),
        "60112675" => Some(include_bytes!("../asset/60112675.png")),
        "60112959" => Some(include_bytes!("../asset/60112959.png")),
        "60502701" => Some(include_bytes!("../asset/60502701.png")),
        "6372755229" => Some(include_bytes!("../asset/6372755229.png")),
        "6412503613" => Some(include_bytes!("../asset/6412503613.png")),
        "6444884337" => Some(include_bytes!("../asset/6444884337.png")),
        "6444884785" => Some(include_bytes!("../asset/6444884785.png")),
        "8057767" => Some(include_bytes!("../asset/8057767.png")),
        "8331089" => Some(include_bytes!("../asset/8331089.png")),
        "84132548" => Some(include_bytes!("../asset/84132548.png")),
        "84313638" => Some(include_bytes!("../asset/84313638.png")),
        "8798201" => Some(include_bytes!("../asset/8798201.png")),
        "89348280" => Some(include_bytes!("../asset/89348280.png")),
        "89627839" => Some(include_bytes!("../asset/89627839.png")),
        "98151581" => Some(include_bytes!("../asset/98151581.png")),
        "99170547" => Some(include_bytes!("../asset/99170547.png")),
        _ => None,
    }
}


pub fn extract_asset_id(raw_str: &str) -> Option<String> {
    let clean = raw_str.trim();
    if clean.is_empty() {
        return None;
    }

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

    if let Some(pos) = clean.find("asset/?id=") {
        let num_part = &clean[pos + 10..];
        let id: String = num_part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !id.is_empty() {
            return Some(id);
        }
    }

    if let Some(pos) = clean.find("assetId/") {
        let num_part = &clean[pos + 8..];
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

pub fn get_cached_mesh(mesh_id: &str) -> Option<MeshData> {
    let key = mesh_id.trim();
    if let Ok(cache) = mesh_cache().lock() {
        if let Some(m) = cache.get(key) {
            return Some(m.clone());
        }
    }

    if let Some(bytes) = get_builtin_mesh_bytes(key) {
        if let Some(mesh) = parse_roblox_mesh(bytes) {
            store_cached_mesh(key.to_string(), mesh.clone());
            return Some(mesh);
        }
    }

    if let Some(id) = extract_asset_id(key) {
        let candidate_paths = [
            format!("/Android/media/com.yourname.rbxleditor/scripts/asset/{id}.mesh"),
            format!("/home/user/Android-rbxl/asset/{id}.mesh"),
            format!("/home/user/asset/{id}.mesh"),
            format!("/sdcard/Download/{id}.mesh"),
        ];

        for cp in &candidate_paths {
            if let Ok(bytes) = std::fs::read(cp) {
                if let Some(mesh) = parse_roblox_mesh(&bytes) {
                    store_cached_mesh(key.to_string(), mesh.clone());
                    return Some(mesh);
                }
            }
        }
    }

    None
}

pub fn store_cached_mesh(mesh_id: String, mesh: MeshData) {
    if let Ok(mut cache) = mesh_cache().lock() {
        cache.insert(mesh_id, mesh);
    }
}

pub fn get_cached_image(image_id: &str) -> Option<Arc<DecodedImage>> {
    let key = image_id.trim();
    if let Ok(cache) = image_cache().lock() {
        if let Some(img) = cache.get(key) {
            return Some(img.clone());
        }
    }

    // Check procedural textures
    match key {
        "__studs" => {
            let img = Arc::new(generate_studs_texture());
            store_cached_image(key.to_string(), img.clone());
            return Some(img);
        }
        "__inlets" => {
            let img = Arc::new(generate_inlets_texture());
            store_cached_image(key.to_string(), img.clone());
            return Some(img);
        }
        "__brick" => {
            let img = Arc::new(generate_brick_texture());
            store_cached_image(key.to_string(), img.clone());
            return Some(img);
        }
        "__diamond_plate" => {
            let img = Arc::new(generate_diamond_plate_texture());
            store_cached_image(key.to_string(), img.clone());
            return Some(img);
        }
        "__wood_planks" => {
            let img = Arc::new(generate_wood_planks_texture());
            store_cached_image(key.to_string(), img.clone());
            return Some(img);
        }
        "__cobblestone" => {
            let img = Arc::new(generate_cobblestone_texture());
            store_cached_image(key.to_string(), img.clone());
            return Some(img);
        }
        "__grass" => {
            let img = Arc::new(generate_grass_texture());
            store_cached_image(key.to_string(), img.clone());
            return Some(img);
        }
        "__concrete" => {
            let img = Arc::new(generate_concrete_texture());
            store_cached_image(key.to_string(), img.clone());
            return Some(img);
        }
        _ => {}
    }

    if let Some(bytes) = get_builtin_image_bytes(key) {
        if let Some(img) = decode_image_bytes(bytes) {
            let arc_img = Arc::new(img);
            store_cached_image(key.to_string(), arc_img.clone());
            return Some(arc_img);
        }
    }

    if let Some(id) = extract_asset_id(key) {
        let candidate_paths = [
            format!("/Android/media/com.yourname.rbxleditor/scripts/asset/{id}.png"),
            format!("/home/user/Android-rbxl/asset/{id}.png"),
            format!("/home/user/asset/{id}.png"),
            format!("/sdcard/Download/{id}.png"),
        ];

        for cp in &candidate_paths {
            if let Ok(bytes) = std::fs::read(cp) {
                if let Some(img) = decode_image_bytes(&bytes) {
                    let arc_img = Arc::new(img);
                    store_cached_image(key.to_string(), arc_img.clone());
                    return Some(arc_img);
                }
            }
        }
    }

    None
}

pub fn store_cached_image(image_id: String, img: Arc<DecodedImage>) {
    if let Ok(mut cache) = image_cache().lock() {
        cache.insert(image_id, img);
    }
}

pub fn decode_image_bytes(bytes: &[u8]) -> Option<DecodedImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let width = rgba.width() as usize;
    let height = rgba.height() as usize;
    Some(DecodedImage {
        width,
        height,
        rgba: rgba.into_raw(),
    })
}

// ----------------------------------------------------------------------------
// Procedural Roblox Material Textures (Studs, Inlets, Brick, DiamondPlate, etc.)
// ----------------------------------------------------------------------------

pub fn generate_studs_texture() -> DecodedImage {
    let w = 64;
    let h = 64;
    let mut rgba = vec![255u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            // 2x2 grid of studs in 64x64 tile (each 32x32)
            let lx = (x % 32) as f32 - 16.0;
            let ly = (y % 32) as f32 - 16.0;
            let r = (lx * lx + ly * ly).sqrt();

            let (base_r, base_g, base_b, base_a) = if r < 9.0 {
                // Top stud face
                let highlight = (-lx - ly) * 2.5;
                let val = (220.0 + highlight).clamp(190.0, 255.0) as u8;
                (val, val, val, 255)
            } else if r < 11.5 {
                // Stud outer bevel edge
                let angle = ly.atan2(lx);
                let light = -(angle.sin() + angle.cos()) * 0.5;
                let val = if light > 0.0 {
                    (220.0 + light * 35.0) as u8
                } else {
                    (170.0 + light * 40.0) as u8
                };
                (val, val, val, 255)
            } else if r < 13.5 {
                // Drop shadow ring around stud
                (140, 140, 140, 255)
            } else {
                // Base flat surface with subtle grid lines
                let grid = (x % 32 == 0 || y % 32 == 0) as u8 * 25;
                let val = 195u8.saturating_sub(grid).clamp(160, 210);
                (val, val, val, 255)
            };

            rgba[idx] = base_r;
            rgba[idx + 1] = base_g;
            rgba[idx + 2] = base_b;
            rgba[idx + 3] = base_a;
        }
    }

    DecodedImage { width: w, height: h, rgba }
}

pub fn generate_inlets_texture() -> DecodedImage {
    let w = 64;
    let h = 64;
    let mut rgba = vec![255u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let lx = (x % 32) as f32 - 16.0;
            let ly = (y % 32) as f32 - 16.0;
            let r = (lx * lx + ly * ly).sqrt();

            let (base_r, base_g, base_b, base_a) = if r < 9.0 {
                // Recessed inlet cavity
                (110, 110, 110, 255)
            } else if r < 11.5 {
                // Inlet inner rim shadow & highlight
                let angle = ly.atan2(lx);
                let light = (angle.sin() + angle.cos()) * 0.5;
                let val = if light > 0.0 {
                    (230.0 + light * 25.0) as u8
                } else {
                    (110.0 + light * 30.0) as u8
                };
                (val, val, val, 255)
            } else {
                // Base surface
                let grid = (x % 32 == 0 || y % 32 == 0) as u8 * 20;
                let val = 195u8.saturating_sub(grid);
                (val, val, val, 255)
            };

            rgba[idx] = base_r;
            rgba[idx + 1] = base_g;
            rgba[idx + 2] = base_b;
            rgba[idx + 3] = base_a;
        }
    }

    DecodedImage { width: w, height: h, rgba }
}

pub fn generate_brick_texture() -> DecodedImage {
    let w = 64;
    let h = 64;
    let mut rgba = vec![255u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let row = y / 16;
            let offset_x = if row % 2 == 1 { x + 16 } else { x };
            let is_mortar_x = (offset_x % 32) < 2;
            let is_mortar_y = (y % 16) < 2;

            let (r, g, b) = if is_mortar_x || is_mortar_y {
                (140, 135, 130)
            } else {
                let grain = (((x * 17 + y * 31) % 23) as i32 - 11) * 2;
                let base = 210 + grain;
                (base.clamp(170, 240) as u8, (base - 10).clamp(160, 230) as u8, (base - 20).clamp(150, 220) as u8)
            };

            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = 255;
        }
    }

    DecodedImage { width: w, height: h, rgba }
}

pub fn generate_diamond_plate_texture() -> DecodedImage {
    let w = 64;
    let h = 64;
    let mut rgba = vec![255u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let lx = (x % 32) as i32 - 16;
            let ly = (y % 32) as i32 - 16;
            let d1 = (lx.abs() * 2 + ly.abs()).abs();
            let d2 = (lx.abs() + ly.abs() * 2).abs();

            let (r, g, b) = if d1 < 14 || d2 < 14 {
                let shine = if lx + ly < 0 { 245 } else { 160 };
                (shine, shine, shine)
            } else {
                (190, 190, 195)
            };

            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = 255;
        }
    }

    DecodedImage { width: w, height: h, rgba }
}

pub fn generate_wood_planks_texture() -> DecodedImage {
    let w = 64;
    let h = 64;
    let mut rgba = vec![255u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let is_seam = (x % 16) < 2;
            let (r, g, b) = if is_seam {
                (90, 60, 40)
            } else {
                let grain = (((y * 13 + x * 3) % 19) as i32 - 9) * 3;
                let val = (190 + grain).clamp(140, 230);
                (val as u8, (val - 25).max(0) as u8, (val - 55).max(0) as u8)
            };

            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = 255;
        }
    }

    DecodedImage { width: w, height: h, rgba }
}

pub fn generate_cobblestone_texture() -> DecodedImage {
    let w = 64;
    let h = 64;
    let mut rgba = vec![255u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let cx = ((x / 16) * 16 + 8) as f32;
            let cy = ((y / 16) * 16 + 8) as f32;
            let dx = (x as f32) - cx;
            let dy = (y as f32) - cy;
            let r = (dx * dx + dy * dy).sqrt();

            let (val_r, val_g, val_b) = if r < 6.5 {
                let shade = 210u8.saturating_sub((r * 8.0) as u8);
                (shade, shade.saturating_sub(5), shade.saturating_sub(10))
            } else {
                (110, 105, 100)
            };

            rgba[idx] = val_r;
            rgba[idx + 1] = val_g;
            rgba[idx + 2] = val_b;
            rgba[idx + 3] = 255;
        }
    }

    DecodedImage { width: w, height: h, rgba }
}

pub fn generate_grass_texture() -> DecodedImage {
    let w = 64;
    let h = 64;
    let mut rgba = vec![255u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let n = (((x * 29 + y * 47) % 31) as i32 - 15) * 3;
            let g = (180 + n).clamp(130, 230) as u8;
            let r = (g / 2).saturating_sub(10);
            let b = (g / 3).saturating_sub(15);

            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = 255;
        }
    }

    DecodedImage { width: w, height: h, rgba }
}

pub fn generate_concrete_texture() -> DecodedImage {
    let w = 64;
    let h = 64;
    let mut rgba = vec![255u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let n = (((x * 37 + y * 73) % 29) as i32 - 14) * 2;
            let val = (185 + n).clamp(150, 220) as u8;

            rgba[idx] = val;
            rgba[idx + 1] = val;
            rgba[idx + 2] = val;
            rgba[idx + 3] = 255;
        }
    }

    DecodedImage { width: w, height: h, rgba }
}

// ----------------------------------------------------------------------------
// Roblox .mesh File Parsers (v1.00 / v1.01 ASCII and v2.00 / v3.00 Binary)
// ----------------------------------------------------------------------------

pub fn parse_roblox_mesh(bytes: &[u8]) -> Option<MeshData> {
    if bytes.len() < 12 {
        return None;
    }

    if bytes.starts_with(b"version 1.00") || bytes.starts_with(b"version 1.01") {
        parse_ascii_mesh(bytes)
    } else if bytes.starts_with(b"version 2.00") || bytes.starts_with(b"version 3.00") {
        parse_binary_mesh_v2_v3(bytes)
    } else {
        None
    }
}

fn parse_ascii_mesh(bytes: &[u8]) -> Option<MeshData> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();

    let header = lines.next()?.trim().to_string();
    let is_v10 = header.starts_with("version 1.00");
    let offset = if is_v10 { 0.5_f32 } else { 1.0_f32 };
    let num_faces: usize = lines.next()?.trim().parse().ok()?;

    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut faces = Vec::new();

    let mut min = [f32::INFINITY, f32::INFINITY, f32::INFINITY];
    let mut max = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];

    let mut vert_idx: u32 = 0;
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let clean = line.replace('[', " ").replace(']', " ").replace(',', " ");
        let nums: Vec<f32> = clean
            .split_whitespace()
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();

        if nums.len() >= 27 {
            for v in 0..3 {
                let off = v * 9;
                let px = nums[off] * offset;
                let py = nums[off + 1] * offset;
                let pz = nums[off + 2] * offset;

                min[0] = min[0].min(px);
                min[1] = min[1].min(py);
                min[2] = min[2].min(pz);
                max[0] = max[0].max(px);
                max[1] = max[1].max(py);
                max[2] = max[2].max(pz);

                vertices.push([px, py, pz]);
                normals.push([nums[off + 3], nums[off + 4], nums[off + 5]]);
                uvs.push([nums[off + 6], 1.0 - nums[off + 7]]);
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
        aabb_min: min,
        aabb_max: max,
    })
}

fn parse_binary_mesh_v2_v3(bytes: &[u8]) -> Option<MeshData> {
    let header_end = bytes.iter().position(|&b| b == b'\n')?;
    let header_line = &bytes[..header_end];
    let version = String::from_utf8_lossy(header_line).trim().to_string();

    let header_start = header_end + 1;
    if bytes.len() < header_start + 12 {
        return None;
    }

    let sizeof_mesh_header = u16::from_le_bytes([bytes[header_start], bytes[header_start + 1]]) as usize;
    let sizeof_vertex = bytes[header_start + 2] as usize;
    let sizeof_face = bytes[header_start + 3] as usize;

    let (num_verts, num_faces) = if sizeof_mesh_header > 12 {
        let num_verts = u32::from_le_bytes([bytes[header_start + 8], bytes[header_start + 9], bytes[header_start + 10], bytes[header_start + 11]]) as usize;
        let num_faces = u32::from_le_bytes([bytes[header_start + 12], bytes[header_start + 13], bytes[header_start + 14], bytes[header_start + 15]]) as usize;
        (num_verts, num_faces)
    } else {
        let num_verts = u32::from_le_bytes([bytes[header_start + 4], bytes[header_start + 5], bytes[header_start + 6], bytes[header_start + 7]]) as usize;
        let num_faces = u32::from_le_bytes([bytes[header_start + 8], bytes[header_start + 9], bytes[header_start + 10], bytes[header_start + 11]]) as usize;
        (num_verts, num_faces)
    };

    let v_start = header_start + sizeof_mesh_header;
    let v_end = v_start + num_verts * sizeof_vertex;

    if bytes.len() < v_end {
        return None;
    }

    let mut vertices = Vec::with_capacity(num_verts);
    let mut normals = Vec::with_capacity(num_verts);
    let mut uvs = Vec::with_capacity(num_verts);

    let mut min = [f32::INFINITY, f32::INFINITY, f32::INFINITY];
    let mut max = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];

    let mut cursor = v_start;
    for _ in 0..num_verts {
        if cursor + 32 > bytes.len() {
            break;
        }

        let px = f32::from_le_bytes([bytes[cursor], bytes[cursor + 1], bytes[cursor + 2], bytes[cursor + 3]]);
        let py = f32::from_le_bytes([bytes[cursor + 4], bytes[cursor + 5], bytes[cursor + 6], bytes[cursor + 7]]);
        let pz = f32::from_le_bytes([bytes[cursor + 8], bytes[cursor + 9], bytes[cursor + 10], bytes[cursor + 11]]);

        let nx = f32::from_le_bytes([bytes[cursor + 12], bytes[cursor + 13], bytes[cursor + 14], bytes[cursor + 15]]);
        let ny = f32::from_le_bytes([bytes[cursor + 16], bytes[cursor + 17], bytes[cursor + 18], bytes[cursor + 19]]);
        let nz = f32::from_le_bytes([bytes[cursor + 20], bytes[cursor + 21], bytes[cursor + 22], bytes[cursor + 23]]);

        let u = f32::from_le_bytes([bytes[cursor + 24], bytes[cursor + 25], bytes[cursor + 26], bytes[cursor + 27]]);
        let v = f32::from_le_bytes([bytes[cursor + 28], bytes[cursor + 29], bytes[cursor + 30], bytes[cursor + 31]]);

        min[0] = min[0].min(px);
        min[1] = min[1].min(py);
        min[2] = min[2].min(pz);
        max[0] = max[0].max(px);
        max[1] = max[1].max(py);
        max[2] = max[2].max(pz);

        vertices.push([px, py, pz]);
        normals.push([nx, ny, nz]);
        uvs.push([u, 1.0 - v]);

        cursor += sizeof_vertex;
    }

    let f_start = v_end;
    let f_end = f_start + num_faces * sizeof_face;
    let mut faces = Vec::with_capacity(num_faces);

    if bytes.len() >= f_end && sizeof_face >= 12 {
        let mut f_cursor = f_start;
        for _ in 0..num_faces {
            if f_cursor + 12 > bytes.len() {
                break;
            }
            let a = u32::from_le_bytes([bytes[f_cursor], bytes[f_cursor + 1], bytes[f_cursor + 2], bytes[f_cursor + 3]]);
            let b = u32::from_le_bytes([bytes[f_cursor + 4], bytes[f_cursor + 5], bytes[f_cursor + 6], bytes[f_cursor + 7]]);
            let c = u32::from_le_bytes([bytes[f_cursor + 8], bytes[f_cursor + 9], bytes[f_cursor + 10], bytes[f_cursor + 11]]);

            faces.push([a, b, c]);
            f_cursor += sizeof_face;
        }
    }

    Some(MeshData {
        version,
        vertex_count: vertices.len(),
        face_count: faces.len(),
        vertices,
        normals,
        uvs,
        faces,
        aabb_min: min,
        aabb_max: max,
    })
}

pub fn scan_place_assets(dom: &WeakDom) -> Vec<DiscoveredAsset> {
    let mut out = Vec::new();
    let mut seen_ids = HashSet::new();

    let mut stack = dom.root().children().to_vec();
    while let Some(r) = stack.pop() {
        if let Some(inst) = dom.get_by_ref(r) {
            stack.extend(inst.children());

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
                    // A compiled .rbxl stores MeshId/TextureID/TextureId as
                    // `Variant::ContentId` and Decal/Texture.Texture as
                    // `Variant::Content` — never as `Variant::String` (that
                    // only ever matched hand-built XML doms). Matching only
                    // String meant this scanner discovered zero assets on
                    // any real compiled place, so "Download & Cache Place
                    // Assets" had nothing to fetch in the first place.
                    let found = match val {
                        Variant::String(s) if !s.is_empty() => Some(s.clone()),
                        Variant::ContentId(c) if !c.as_str().is_empty() => Some(c.as_str().to_string()),
                        Variant::Content(c) => match c.value() {
                            ContentType::Uri(s) if !s.is_empty() => Some(s.clone()),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(s) = found {
                        if let Some(id) = extract_asset_id(&s) {
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
