//! Offline Roblox **plugin** library manager.
//!
//! A Roblox Studio plugin is just a `.rbxm`/`.rbxmx` model whose root is a
//! `Script`/`ModuleScript` (the entry point Studio runs). Plugins do NOT live
//! inside a place's DataModel under some `PluginServices`; instead Studio loads
//! them from its `Plugins` folder and, at runtime, their dock-widget UIs are
//! parented to **`PluginGuiService`** and any injected UI to **`CoreGui`**,
//! while toolbars/buttons come from the global `plugin:CreateToolbar(...)`.
//!
//! This app has no Roblox engine to *execute* a plugin, so it cannot render a
//! live dock widget. What it CAN do — and what this module provides — is
//! everything else:
//!
//! * store a persistent library of plugins in the app's internal files dir;
//! * import a local `.rbxm`/`.rbxmx`, or download one from the Creator Store by
//!   asset ID (both reuse `rbxl::decode_model_bytes`, so binary/XML/gzip work);
//! * enable/disable/delete plugins;
//! * inspect a plugin's instance hierarchy, locate any GUI objects a plugin
//!   ships (ScreenGui / DockWidgetPluginGui / frames / buttons …) and list the
//!   scripts;
//! * open any script's source in the editor, and insert a plugin's contents
//!   into the currently-open place.

use crate::rbxl;
use anyhow::{anyhow, Context, Result};
use rbx_dom_weak::{types::Ref, InstanceBuilder, WeakDom};
use serde::{Deserialize, Serialize};

use std::path::PathBuf;

/// Metadata for one installed plugin, persisted in `plugins_index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRecord {
    /// Stable id (a slugified name, or `asset_<id>` for store plugins).
    pub id: String,
    /// Display name (root instance name, or asset title).
    pub name: String,
    /// Where the plugin came from.
    pub source: PluginSource,
    /// Creator Store asset id, if it came from there.
    pub asset_id: Option<u64>,
    /// Filename inside the plugins dir (e.g. `my_plugin.rbxm`).
    pub file_name: String,
    /// Whether the user has it enabled (mirrors Studio's "Manage Plugins").
    pub enabled: bool,
    /// Number of top-level instances in the model.
    pub roots: usize,
    /// Total instance count.
    pub instances: usize,
    /// Scripts found anywhere in the plugin (name + class).
    pub scripts: Vec<(String, String)>,
    /// GUI objects found anywhere in the plugin — these are what Studio would
    /// show under PluginGuiService / CoreGui once the plugin runs.
    pub guis: Vec<PluginGuiInfo>,
    /// Roblox asset id for the plugin's icon (Creator Store plugins usually
    /// have one; local plugins default to the toolbox/generic icon).
    pub icon_asset_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginSource {
    Local,
    CreatorStore,
}

/// A summary of a GUI element inside a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginGuiInfo {
    pub name: String,
    pub class: String,
    /// How many descendant GUI elements it contains (rough widget complexity).
    pub descendants: usize,
}

/// The on-disk index of installed plugins.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginIndex {
    pub plugins: Vec<PluginRecord>,
}

impl PluginIndex {
    pub fn get(&self, id: &str) -> Option<&PluginRecord> {
        self.plugins.iter().find(|p| p.id == id)
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut PluginRecord> {
        self.plugins.iter_mut().find(|p| p.id == id)
    }
}

impl PluginRecord {
    /// Best-effort class name for display. For a typical Studio plugin the
    /// root is a single Script/ModuleScript; otherwise list the first class.
    pub fn class(&self) -> &str {
        self.scripts
            .first()
            .map(|(_, c)| c.as_str())
            .unwrap_or("Plugin")
    }
}

/// Root directory for plugin storage. Desktop uses ~/.rbxl_editor/plugins;
/// Android uses the app's internal files dir (no permissions needed).
pub fn plugins_dir() -> Result<PathBuf> {
    let base = plugins_base_dir()?;
    let dir = base.join("plugins");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {:?}", dir))?;
    Ok(dir)
}

#[cfg(target_os = "android")]
fn plugins_base_dir() -> Result<PathBuf> {
    let dir = crate::jni_bridge::files_dir()
        .ok_or_else(|| anyhow!("Android internal files dir is unavailable"))?;
    Ok(PathBuf::from(dir))
}

#[cfg(not(target_os = "android"))]
fn plugins_base_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Ok(PathBuf::from(home).join(".rbxl_editor"))
}

fn index_path() -> Result<PathBuf> {
    Ok(plugins_dir()?.join("plugins_index.json"))
}

pub fn load_index() -> PluginIndex {
    match index_path() {
        Ok(path) if path.exists() => std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        _ => PluginIndex::default(),
    }
}

pub fn save_index(index: &PluginIndex) -> Result<()> {
    let path = index_path()?;
    let json = serde_json::to_string_pretty(index)?;
    std::fs::write(&path, json).with_context(|| format!("writing {:?}", path))?;
    Ok(())
}

/// Classes that represent a plugin/user-interface container or widget.
const GUI_CLASSES: &[&str] = &[
    "ScreenGui",
    "DockWidgetPluginGui",
    "PluginGui",
    "Frame",
    "CanvasGroup",
    "ScrollingFrame",
    "TextLabel",
    "TextButton",
    "TextBox",
    "ImageLabel",
    "ImageButton",
    "ViewportFrame",
    "BillboardGui",
    "SurfaceGui",
];

fn is_gui_class(class: &str) -> bool {
    GUI_CLASSES.contains(&class)
}

fn is_script_class(class: &str) -> bool {
    matches!(class, "Script" | "LocalScript" | "ModuleScript")
}

/// Walk a decoded plugin DOM and build a summary record (scripts + GUIs).
fn analyze(name: &str, source: PluginSource, asset_id: Option<u64>, file_name: String, dom: &WeakDom) -> PluginRecord {
    let roots = dom.root().children().len();
    let mut instances = 0usize;
    let mut scripts = Vec::new();
    let mut guis = Vec::new();

    let mut stack: Vec<Ref> = dom.root().children().to_vec();
    while let Some(r) = stack.pop() {
        if let Some(inst) = dom.get_by_ref(r) {
            instances += 1;
            if is_script_class(inst.class.as_str()) {
                scripts.push((inst.name.clone(), inst.class.to_string()));
            }
            if is_gui_class(inst.class.as_str()) {
                let descendants = count_descendants(dom, r);
                guis.push(PluginGuiInfo {
                    name: inst.name.clone(),
                    class: inst.class.to_string(),
                    descendants,
                });
            }
            stack.extend(inst.children());
        }
    }

    // Stable id: prefer asset id, else slugify the name.
    let id = match asset_id {
        Some(id) => format!("asset_{id}"),
        None => slugify(name),
    };

    PluginRecord {
        id,
        name: name.to_string(),
        source,
        asset_id,
        file_name,
        enabled: true,
        roots,
        instances,
        scripts,
        guis,
        icon_asset_id: asset_id,
    }
}

fn count_descendants(dom: &WeakDom, root: Ref) -> usize {
    let mut count = 0;
    let mut stack = vec![root];
    while let Some(r) = stack.pop() {
        if let Some(inst) = dom.get_by_ref(r) {
            for &c in inst.children() {
                count += 1;
                stack.push(c);
            }
        }
    }
    count
}

fn slugify(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_').to_string()
}

/// Choose a unique file path in the plugins dir for a given id/extension.
fn unique_plugin_file(id: &str, binary: bool) -> Result<(PathBuf, String)> {
    let dir = plugins_dir()?;
    let ext = if binary { "rbxm" } else { "rbxmx" };
    let mut candidate = format!("{id}.{ext}");
    let mut n = 2;
    while dir.join(&candidate).exists() {
        candidate = format!("{id}_{n}.{ext}");
        n += 1;
    }
    Ok((dir.join(&candidate), candidate))
}

/// Add a plugin from raw model bytes (used by both local import and store
/// download). The bytes are decoded to validate + summarize, then re-serialized
/// to binary so everything on disk is a uniform `.rbxm`.
pub fn add_plugin_from_bytes(
    index: &mut PluginIndex,
    name_hint: &str,
    source: PluginSource,
    asset_id: Option<u64>,
    bytes: &[u8],
) -> Result<PluginRecord> {
    let dom = rbxl::decode_model_bytes(bytes).context("decoding plugin model")?;

    // Prefer the first top-level instance's name as the plugin name.
    let root_name = dom
        .root()
        .children()
        .iter()
        .find_map(|r| dom.get_by_ref(*r).map(|i| i.name.clone()))
        .unwrap_or_else(|| name_hint.to_string());

    let (path, file_name) = unique_plugin_file(
        &asset_id.map(|id| format!("asset_{id}")).unwrap_or_else(|| slugify(&root_name)),
        true,
    )?;

    // Re-serialize to a canonical binary .rbxm for uniform storage.
    let saved = rbxl::save_place(&dom).context("re-serializing plugin to .rbxm")?;
    std::fs::write(&path, &saved).with_context(|| format!("writing {:?}", path))?;

    let record = analyze(&root_name, source, asset_id, file_name, &dom);

    // Replace any existing record with the same id (re-import / update).
    index.plugins.retain(|p| p.id != record.id);
    let copy = record.clone();
    index.plugins.push(record);
    save_index(index)?;
    Ok(copy)
}

/// Load and decode a stored plugin's `.rbxm` back into a `WeakDom`.
pub fn load_plugin_dom(record: &PluginRecord) -> Result<WeakDom> {
    let path = plugins_dir()?.join(&record.file_name);
    let bytes = std::fs::read(&path).with_context(|| format!("reading {:?}", path))?;
    rbxl::decode_model_bytes(&bytes).context("parsing stored plugin")
}

/// Toggle a plugin's enabled flag and persist the index.
pub fn set_enabled(index: &mut PluginIndex, id: &str, enabled: bool) -> Result<()> {
    if let Some(rec) = index.get_mut(id) {
        rec.enabled = enabled;
        save_index(index)?;
    }
    Ok(())
}

/// Delete a plugin's file and index entry.
pub fn delete_plugin(index: &mut PluginIndex, id: &str) -> Result<()> {
    if let Some(rec) = index.get(id) {
        let path = plugins_dir()?.join(&rec.file_name);
        let _ = std::fs::remove_file(&path);
    }
    index.plugins.retain(|p| p.id != id);
    save_index(index)?;
    Ok(())
}

/// Insert a stored plugin's entire hierarchy into an open place `dom` under
/// `parent`. Returns the first inserted ref and total instance count.
pub fn insert_into_place(
    record: &PluginRecord,
    target_dom: &mut WeakDom,
    parent: Ref,
) -> Result<(Ref, usize)> {
    let source_dom = load_plugin_dom(record)?;
    let (first, count) = rbxl::insert_all_root_children(target_dom, parent, &source_dom);
    let first = first.ok_or_else(|| anyhow!("plugin contained no instances"))?;
    Ok((first, count))
}

/// Build a standalone WeakDom from a script's source (handy for "export script").
pub fn dom_from_script_source(name: &str, class: &str, source: &str) -> WeakDom {
    let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
    let root = dom.root_ref();
    dom.insert(
        root,
        InstanceBuilder::new(class)
            .with_name(name)
            .with_property("Source", rbx_dom_weak::types::Variant::String(source.to_string())),
    );
    dom
}

/// Best-effort: where would this plugin's UI live at runtime?
/// Returns a human-readable location for the status/help text.
pub fn gui_runtime_location(class: &str) -> &'static str {
    match class {
        "DockWidgetPluginGui" | "PluginGui" => "PluginGuiService (dock widget)",
        "BillboardGui" | "SurfaceGui" => "parented to a Part/Adornee at runtime",
        _ => "CoreGui (injected at runtime) — ScreenGui/Frame widgets",
    }
}

/// Helper used by the UI: sanitize a free-text plugin name into a filename-ish.
pub fn sanitize_name(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == ' ' || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

/// Re-export Path so callers don't need a std::path import.
pub fn plugins_path() -> Option<PathBuf> {
    plugins_dir().ok()
}
