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
///
/// Accepts BOTH binary places (`.rbxl`, header `<roblox!…`) and XML places
/// (`.rbxlx`, starts with `<roblox`/`<?xml`), auto-detecting from the content
/// rather than the extension (the SAF picker gives us no reliable MIME type).
pub fn load_place(bytes: Vec<u8>) -> Result<WeakDom> {
    let trimmed = skip_leading_whitespace_and_bom(&bytes);

    // Binary .rbxl
    if trimmed.starts_with(&BINARY_HEADER) {
        return rbx_binary::from_reader(Cursor::new(trimmed)).context("parsing rbxl (binary)");
    }

    // XML .rbxlx
    if trimmed.starts_with(b"<roblox") || trimmed.starts_with(b"<?xml") {
        return rbx_xml::from_reader_default(Cursor::new(trimmed)).context("parsing rbxlx (xml)");
    }

    // Fallbacks: try both parsers and surface whichever error is most useful.
    match rbx_binary::from_reader(Cursor::new(trimmed)) {
        Ok(dom) => Ok(dom),
        Err(bin_err) => match rbx_xml::from_reader_default(Cursor::new(trimmed)) {
            Ok(dom) => Ok(dom),
            Err(xml_err) => bail!(
                "Could not parse place as .rbxl or .rbxlx.\n  \
                 • Binary: {bin_err}\n  • XML: {xml_err}"
            ),
        },
    }
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

/// The 8-byte magic header of every raw binary Roblox model (.rbxm) and place
/// (.rbxl) file: `<roblox!` followed by `\x89\xFF\x0D\x0A\x1A\x0A`. This is
/// exactly what a local .rbxm saved by Studio starts with.
const BINARY_HEADER: [u8; 8] = *b"<roblox!";

/// Decodes any Roblox model payload into a `WeakDom` hierarchy.
///
/// Accepts, in order:
/// * raw binary `.rbxm` / `.rbxl` (`<roblox!…` magic) — this is what **local**
///   model files use, with no gzip/wrapper around them;
/// * gzip- or zstd-wrapped binary/XML (Creator Store / Asset Delivery payloads);
/// * XML `.rbxmx` / `.rbxlx`;
/// * plain Lua/Luau source (wrapped in a `ModuleScript`).
///
/// Unlike the previous implementation, every parser failure is **recorded** and
/// surfaced in the final error instead of being silently swallowed, so a file
/// that the parser genuinely can't read (e.g. produced by a newer Studio than
/// the bundled reflection database) produces an actionable message rather than
/// a generic "unsupported format".
pub fn decode_model_bytes(bytes: &[u8]) -> Result<WeakDom> {
    if bytes.is_empty() {
        bail!("Cannot decode empty model bytes");
    }

    // Peel off any compression wrapper. gzip (0x1F 0x8B) is used by the classic
    // AssetDelivery v1 endpoint; zstd (0x28 0xB5 0x2F 0xFD) is used by newer
    // CDN responses. Local .rbxm files are NOT compressed, so this is a no-op
    // for them.
    let decompressed: Vec<u8>;
    let payload: &[u8] = if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let mut buf = Vec::new();
        match GzDecoder::new(bytes).read_to_end(&mut buf) {
            Ok(_) if !buf.is_empty() => {
                decompressed = buf;
                &decompressed[..]
            }
            _ => bytes,
        }
    } else if bytes.len() >= 4 && bytes[0] == 0x28 && bytes[1] == 0xb5 && bytes[2] == 0x2f && bytes[3] == 0xfd {
        // zstd magic — rbx_binary itself also understands chunk-level zstd, but
        // a whole-file zstd wrapper needs explicit decompression first.
        match zstd_decode_all(bytes) {
            Some(buf) if !buf.is_empty() => {
                decompressed = buf;
                &decompressed[..]
            }
            _ => bytes,
        }
    } else {
        bytes
    };

    // Some providers/pickers prepend a UTF-8 BOM or stray whitespace; trim it
    // before signature matching (never mutates the actual parsed bytes).
    let trimmed = skip_leading_whitespace_and_bom(payload);

    let mut binary_err: Option<String> = None;
    let mut xml_err: Option<String> = None;

    // 1. Raw binary .rbxm / .rbxl — exact 8-byte signature.
    if trimmed.starts_with(&BINARY_HEADER) {
        match rbx_binary::from_reader(Cursor::new(trimmed)) {
            Ok(dom) => return Ok(dom),
            Err(e) => binary_err = Some(format!("rbx_binary: {e}")),
        }
    }

    // 2. XML .rbxmx / .rbxlx.
    let looks_like_xml = trimmed.starts_with(b"<roblox")
        || trimmed.starts_with(b"<?xml")
        || trimmed.windows(12).any(|w| w == b"<Item class=");
    if looks_like_xml {
        match rbx_xml::from_reader_default(Cursor::new(trimmed)) {
            Ok(dom) => return Ok(dom),
            Err(e) => xml_err = Some(format!("rbx_xml: {e}")),
        }
    }

    // 3. General fallbacks: a file may have a weird/extensionless header yet
    //    still be parseable. Try both parsers and remember their errors.
    if binary_err.is_none() {
        match rbx_binary::from_reader(Cursor::new(trimmed)) {
            Ok(dom) => return Ok(dom),
            Err(e) => binary_err = Some(format!("rbx_binary: {e}")),
        }
    }
    if xml_err.is_none() {
        match rbx_xml::from_reader_default(Cursor::new(trimmed)) {
            Ok(dom) => return Ok(dom),
            Err(e) => xml_err = Some(format!("rbx_xml: {e}")),
        }
    }

    // 4. Lua / Luau source fallback.
    if let Ok(text) = std::str::from_utf8(trimmed) {
        let t = text.trim();
        if t.starts_with("--")
            || t.contains("function")
            || t.contains("local ")
            || t.contains("return ")
            || t.contains("game:")
        {
            let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
            let root = dom.root_ref();
            let builder = InstanceBuilder::new("ModuleScript")
                .with_name("ScriptAsset")
                .with_property("Source", Variant::String(text.to_string()));
            dom.insert(root, builder);
            return Ok(dom);
        }
    }

    // Build a diagnostic that actually tells the user (and us) what happened.
    let head: String = trimmed
        .iter()
        .take(16)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut msg = format!(
        "Could not decode Roblox model/place ({} bytes, first bytes: {head}).",
        trimmed.len()
    );
    if let Some(e) = &binary_err {
        msg.push_str(&format!("\n  • Binary (.rbxm) parser: {e}"));
    }
    if let Some(e) = &xml_err {
        msg.push_str(&format!("\n  • XML (.rbxmx) parser: {e}"));
    }
    msg.push_str(
        "\nIf this is a .rbxm saved by a very recent Roblox Studio, updating the \
         rbx_binary/rbx_reflection_database crates may be required.",
    );
    bail!(msg)
}

/// Skip ASCII whitespace and an optional UTF-8 BOM at the start of `bytes`.
fn skip_leading_whitespace_and_bom(mut bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes = &bytes[3..];
    }
    while let Some((&b, rest)) = bytes.split_first() {
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

/// Decompress a whole-file zstd stream (used by newer AssetDelivery CDN
/// responses). Local `.rbxm` files are never zstd-wrapped, so this only fires
/// for downloaded payloads. Returns `None` if the stream isn't valid zstd, in
/// which case the caller passes the raw bytes through to the parsers.
fn zstd_decode_all(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut dec = zstd::Decoder::new(bytes).ok()?;
    let mut out = Vec::new();
    dec.read_to_end(&mut out).ok()?;
    Some(out)
}

/// Serialize the whole DOM to bytes, ready to hand to the JNI bridge for saving.
pub fn save_place(dom: &WeakDom) -> Result<Vec<u8>> {
    save_place_as(dom, PlaceFormat::Binary)
}

/// The on-disk format of a place/model file. Tracked so "Save" writes back the
/// same format the user opened (e.g. an opened `.rbxlx` stays XML).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceFormat {
    /// Binary `.rbxl` / `.rbxm` (header `<roblox!…`).
    Binary,
    /// XML `.rbxlx` / `.rbxmx`.
    Xml,
}

impl PlaceFormat {
    /// Detect the format from raw file bytes.
    ///
    /// Both formats begin with `<roblox`, so the discriminator is byte 7:
    /// binary files have `!` (`<roblox!…`), while XML files have ` ` or `>`
    /// (`<roblox version="4">` / `<roblox xmlns…`).
    pub fn detect(bytes: &[u8]) -> PlaceFormat {
        let t = skip_leading_whitespace_and_bom(bytes);
        if t.starts_with(&BINARY_HEADER) {
            PlaceFormat::Binary
        } else if t.starts_with(b"<?xml") || t.starts_with(b"<roblox") {
            PlaceFormat::Xml
        } else {
            // Unknown header; binary is the modern default.
            PlaceFormat::Binary
        }
    }

    /// Suggested file extension for "Save As".
    pub fn extension(self) -> &'static str {
        match self {
            PlaceFormat::Binary => "rbxl",
            PlaceFormat::Xml => "rbxlx",
        }
    }

    /// Human-readable label for the status bar.
    pub fn label(self) -> &'static str {
        match self {
            PlaceFormat::Binary => ".rbxl binary",
            PlaceFormat::Xml => ".rbxlx XML",
        }
    }
}

/// Serialize the DOM in the requested format.
pub fn save_place_as(dom: &WeakDom, format: PlaceFormat) -> Result<Vec<u8>> {
    let refs: Vec<Ref> = dom.root().children().to_vec();
    match format {
        PlaceFormat::Binary => {
            let mut buf = Vec::new();
            rbx_binary::to_writer(&mut buf, dom, &refs).context("serializing rbxl (binary)")?;
            Ok(buf)
        }
        PlaceFormat::Xml => {
            let mut buf = Vec::new();
            rbx_xml::to_writer_default(Cursor::new(&mut buf), dom, &refs)
                .context("serializing rbxlx (xml)")?;
            Ok(buf)
        }
    }
}
