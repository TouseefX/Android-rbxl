use anyhow::{bail, Context, Result};
use rbx_dom_weak::{
    types::{BinaryString, Color3, Color3uint8, Ref, SharedString, Variant, Vector3},
    InstanceBuilder, Ustr, WeakDom,
};
use std::io::Cursor;

pub struct ScriptEntry {
    pub referent: Ref,
    pub name: String,
    pub class: String, // "Script" | "LocalScript" | "ModuleScript"
    pub source: String,
}

pub struct PlaceStats {
    pub total_instances: usize,
    pub scripts_count: usize,
    pub parts_count: usize,
    pub models_count: usize,
    pub gui_count: usize,
}

/// Parse a place file already held in memory (bytes come from the JNI bridge,
/// not a filesystem path — see jni_bridge.rs).
pub fn load_place(bytes: Vec<u8>) -> Result<WeakDom> {
    rbx_binary::from_reader(Cursor::new(bytes)).context("parsing rbxl")
}

/// Retrieve the script source text from an instance, supporting String,
/// SharedString, and BinaryString formats used by Roblox place files.
pub fn get_source(dom: &WeakDom, referent: Ref) -> Option<String> {
    let inst = dom.get_by_ref(referent)?;
    let prop = inst.properties.get(&Ustr::from("Source"))?;
    match prop {
        Variant::String(s) => Some(s.clone()),
        Variant::SharedString(shared) => {
            Some(String::from_utf8_lossy(shared.data()).into_owned())
        }
        Variant::BinaryString(bin) => {
            Some(String::from_utf8_lossy(bin.as_ref()).into_owned())
        }
        _ => None,
    }
}

pub fn set_source(
    dom: &mut WeakDom,
    referent: Ref,
    new_source: String,
) -> Result<()> {
    let inst = dom
        .get_by_ref_mut(referent)
        .context("instance no longer exists")?;
    let ustr_source = Ustr::from("Source");
    let new_variant = match inst.properties.get(&ustr_source) {
        Some(Variant::SharedString(_)) => {
            Variant::SharedString(SharedString::new(new_source.into_bytes()))
        }
        Some(Variant::BinaryString(_)) => {
            Variant::BinaryString(BinaryString::from(new_source.into_bytes()))
        }
        _ => Variant::String(new_source),
    };
    inst.properties.insert(ustr_source, new_variant);
    Ok(())
}

pub fn add_instance(
    dom: &mut WeakDom,
    parent: Ref,
    class: &str,
    name: &str,
) -> Result<Ref> {
    let mut builder = InstanceBuilder::new(class).with_name(name);

    match class {
        "Script" | "LocalScript" | "ModuleScript" => {
            builder = builder.with_property("Source", Variant::String(String::new()));
        }
        "Part" | "WedgePart" | "CornerWedgePart" | "TrussPart" | "SpawnLocation" => {
            builder = builder
                .with_property("Size", Variant::Vector3(Vector3::new(4.0, 1.0, 2.0)))
                .with_property("Position", Variant::Vector3(Vector3::new(0.0, 0.5, 0.0)))
                .with_property("Anchored", Variant::Bool(true))
                .with_property("CanCollide", Variant::Bool(true))
                .with_property("Color", Variant::Color3(Color3::new(0.64, 0.64, 0.64)));
        }
        "StringValue" => {
            builder = builder.with_property("Value", Variant::String(String::new()));
        }
        "IntValue" => {
            builder = builder.with_property("Value", Variant::Int64(0));
        }
        "NumberValue" => {
            builder = builder.with_property("Value", Variant::Float64(0.0));
        }
        "BoolValue" => {
            builder = builder.with_property("Value", Variant::Bool(false));
        }
        "Color3Value" => {
            builder = builder.with_property("Value", Variant::Color3(Color3::new(1.0, 1.0, 1.0)));
        }
        "Vector3Value" => {
            builder = builder.with_property("Value", Variant::Vector3(Vector3::new(0.0, 0.0, 0.0)));
        }
        "TextLabel" | "TextButton" | "TextBox" => {
            builder = builder
                .with_property("Text", Variant::String(name.to_string()))
                .with_property("TextColor3", Variant::Color3uint8(Color3uint8::new(255, 255, 255)))
                .with_property("TextScaled", Variant::Bool(true));
        }
        _ => {}
    }

    Ok(dom.insert(parent, builder))
}

pub fn delete_instance(dom: &mut WeakDom, referent: Ref) -> Result<()> {
    if referent == dom.root_ref() {
        bail!("cannot delete root instance");
    }
    dom.destroy(referent);
    Ok(())
}

pub fn rename_instance(dom: &mut WeakDom, referent: Ref, new_name: &str) -> Result<()> {
    let inst = dom.get_by_ref_mut(referent).context("instance not found")?;
    inst.name = new_name.to_string();
    Ok(())
}

pub fn duplicate_instance(dom: &mut WeakDom, referent: Ref) -> Result<Ref> {
    let (parent, builder) = {
        let inst = dom.get_by_ref(referent).context("instance not found")?;
        let parent = inst.parent();
        let mut b = InstanceBuilder::new(inst.class.as_str())
            .with_name(&format!("{} (Copy)", inst.name));
        for (k, v) in &inst.properties {
            b = b.with_property(k.as_str(), v.clone());
        }
        (parent, b)
    };
    let new_ref = dom.insert(parent, builder);

    fn copy_children(dom: &mut WeakDom, orig: Ref, new_parent: Ref) {
        let children = if let Some(inst) = dom.get_by_ref(orig) {
            inst.children().to_vec()
        } else {
            return;
        };
        for child in children {
            if let Some(child_inst) = dom.get_by_ref(child) {
                let mut b = InstanceBuilder::new(child_inst.class.as_str())
                    .with_name(&child_inst.name);
                for (k, v) in &child_inst.properties {
                    b = b.with_property(k.as_str(), v.clone());
                }
                let new_child = dom.insert(new_parent, b);
                copy_children(dom, child, new_child);
            }
        }
    }
    copy_children(dom, referent, new_ref);
    Ok(new_ref)
}

pub fn set_property(dom: &mut WeakDom, referent: Ref, key: &str, value: Variant) -> Result<()> {
    let inst = dom.get_by_ref_mut(referent).context("instance not found")?;
    inst.properties.insert(Ustr::from(key), value);
    Ok(())
}

pub fn delete_property(dom: &mut WeakDom, referent: Ref, key: &str) -> Result<()> {
    let inst = dom.get_by_ref_mut(referent).context("instance not found")?;
    inst.properties.remove(&Ustr::from(key));
    Ok(())
}

pub fn calculate_stats(dom: &WeakDom) -> PlaceStats {
    let mut total = 0;
    let mut scripts = 0;
    let mut parts = 0;
    let mut models = 0;
    let mut guis = 0;

    let mut stack = dom.root().children().to_vec();
    while let Some(r) = stack.pop() {
        if let Some(inst) = dom.get_by_ref(r) {
            total += 1;
            match inst.class.as_str() {
                "Script" | "LocalScript" | "ModuleScript" => scripts += 1,
                "Part" | "WedgePart" | "CornerWedgePart" | "TrussPart" | "SpawnLocation" => parts += 1,
                "Model" => models += 1,
                "ScreenGui" | "BillboardGui" | "SurfaceGui" | "Frame" | "TextLabel" | "TextButton" => guis += 1,
                _ => {}
            }
            stack.extend(inst.children());
        }
    }

    PlaceStats {
        total_instances: total,
        scripts_count: scripts,
        parts_count: parts,
        models_count: models,
        gui_count: guis,
    }
}

/// Recursively copies an instance from `source_dom` (including all properties and child subtrees)
/// into `target_dom` under `target_parent`.
pub fn insert_dom_subtree(
    target_dom: &mut WeakDom,
    target_parent: Ref,
    source_dom: &WeakDom,
    source_ref: Ref,
) -> Option<Ref> {
    let source_inst = source_dom.get_by_ref(source_ref)?;
    let mut builder = InstanceBuilder::new(source_inst.class.as_str())
        .with_name(&source_inst.name);
    for (k, v) in &source_inst.properties {
        builder = builder.with_property(k.as_str(), v.clone());
    }
    let new_ref = target_dom.insert(target_parent, builder);

    for &child_ref in source_inst.children() {
        insert_dom_subtree(target_dom, new_ref, source_dom, child_ref);
    }
    Some(new_ref)
}

/// Inserts all top-level children from `source_dom` into `target_dom` under `target_parent`.
/// Returns the Ref of the first inserted top-level instance and the total count of all inserted instances.
pub fn insert_all_root_children(
    target_dom: &mut WeakDom,
    target_parent: Ref,
    source_dom: &WeakDom,
) -> (Option<Ref>, usize) {
    let mut first_ref = None;
    let mut total_count = 0;
    for &root_child in source_dom.root().children() {
        if let Some(r) = insert_dom_subtree(target_dom, target_parent, source_dom, root_child) {
            if first_ref.is_none() {
                first_ref = Some(r);
            }
            total_count += count_instances(target_dom, r);
        }
    }
    (first_ref, total_count)
}

pub fn count_instances(dom: &WeakDom, root: Ref) -> usize {
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(r) = stack.pop() {
        if let Some(inst) = dom.get_by_ref(r) {
            count += 1;
            stack.extend(inst.children());
        }
    }
    count
}

/// Decodes any Roblox model payload (.rbxm binary, .rbxmx XML, compressed gzip, or Luau script)
/// into a `WeakDom` hierarchy.
pub fn decode_model_bytes(bytes: &[u8]) -> Result<WeakDom> {
    if bytes.is_empty() {
        bail!("Cannot decode empty model bytes");
    }

    // 1. Decompress if gzipped (magic bytes 0x1f 0x8b)
    let decompressed: Vec<u8>;
    let payload = if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let mut gz = GzDecoder::new(bytes);
        let mut buf = Vec::new();
        if gz.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
            decompressed = buf;
            &decompressed[..]
        } else {
            bytes
        }
    } else {
        bytes
    };

    // 2. Binary RBXM / RBXL (starts with "<roblox!\x89\xff\r\n\x1a\n" or "<roblox")
    if payload.starts_with(b"<roblox!") || (payload.len() >= 8 && &payload[0..7] == b"<roblox" && payload.contains(&0x89)) {
        if let Ok(dom) = rbx_binary::from_reader(Cursor::new(payload)) {
            return Ok(dom);
        }
    }

    // 3. XML RBXMX / RBXLX (starts with "<roblox" or "<?xml")
    if payload.starts_with(b"<roblox") || payload.starts_with(b"<?xml") || payload.windows(12).any(|w| w == b"<Item class=") {
        if let Ok(dom) = rbx_xml::from_reader_default(Cursor::new(payload)) {
            return Ok(dom);
        }
    }

    // Try rbx_binary general fallback
    if let Ok(dom) = rbx_binary::from_reader(Cursor::new(payload)) {
        return Ok(dom);
    }

    // Try rbx_xml general fallback
    if let Ok(dom) = rbx_xml::from_reader_default(Cursor::new(payload)) {
        return Ok(dom);
    }

    // 4. Lua / Luau Source Code fallback
    if let Ok(text) = std::str::from_utf8(payload) {
        let trimmed = text.trim();
        if trimmed.starts_with("--") || trimmed.contains("function") || trimmed.contains("local ") || trimmed.contains("return ") || trimmed.contains("game:") {
            let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
            let root = dom.root_ref();
            let builder = InstanceBuilder::new("ModuleScript")
                .with_name("ScriptAsset")
                .with_property("Source", Variant::String(text.to_string()));
            dom.insert(root, builder);
            return Ok(dom);
        }
    }

    bail!("Unsupported Roblox model format or unrecognized payload structure")
}

/// Serialize the whole DOM to bytes, ready to hand to the JNI bridge for saving.
pub fn save_place(dom: &WeakDom) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    rbx_binary::to_writer(&mut buf, dom, dom.root().children()).context("serializing rbxl")?;
    Ok(buf)
}
