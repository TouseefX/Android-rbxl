use anyhow::{Context, Result, bail};
use rbx_dom_weak::{InstanceBuilder, Ustr, WeakDom, types::Variant};
use std::io::Cursor;

pub struct ScriptEntry {
    pub referent: rbx_dom_weak::types::Ref,
    pub name: String,
    pub class: String, // "Script" | "LocalScript" | "ModuleScript"
    pub source: String,
}

/// Parse a place file already held in memory (bytes come from the JNI bridge,
/// not a filesystem path — see jni_bridge.rs).
pub fn load_place(bytes: Vec<u8>) -> Result<WeakDom> {
    rbx_binary::from_reader(Cursor::new(bytes)).context("parsing rbxl")
}

/// Walk the whole tree and collect every script-like instance.
pub fn collect_scripts(dom: &WeakDom) -> Vec<ScriptEntry> {
    let mut out = Vec::new();
    let mut stack: Vec<_> = dom.root().children().to_vec();

    while let Some(referent) = stack.pop() {
        let Some(inst) = dom.get_by_ref(referent) else {
            continue;
        };
        stack.extend(inst.children());

        if matches!(inst.class.as_str(), "Script" | "LocalScript" | "ModuleScript") {
            let source = match inst.properties.get(&Ustr::from("Source")) {
                Some(Variant::String(s)) => s.clone(),
                _ => String::new(),
            };
            out.push(ScriptEntry {
                referent,
                name: inst.name.clone(),
                class: inst.class.to_string(),
                source,
            });
        }
    }
    out
}

pub fn set_source(
    dom: &mut WeakDom,
    referent: rbx_dom_weak::types::Ref,
    new_source: String,
) -> Result<()> {
    let inst = dom
        .get_by_ref_mut(referent)
        .context("instance no longer exists")?;
    inst.properties
        .insert(Ustr::from("Source"), Variant::String(new_source));
    Ok(())
}

pub fn add_script(
    dom: &mut WeakDom,
    parent: rbx_dom_weak::types::Ref,
    class: &str,
    name: &str,
    source: String,
) -> Result<rbx_dom_weak::types::Ref> {
    if !matches!(class, "Script" | "LocalScript" | "ModuleScript") {
        bail!("unsupported class: {class}");
    }
    let builder = InstanceBuilder::new(class)
        .with_name(name)
        .with_property("Source", Variant::String(source));
    Ok(dom.insert(parent, builder))
}

/// Serialize the whole DOM to bytes, ready to hand to the JNI bridge for saving.
pub fn save_place(dom: &WeakDom) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    rbx_binary::to_writer(&mut buf, dom, dom.root().children()).context("serializing rbxl")?;
    Ok(buf)
}
