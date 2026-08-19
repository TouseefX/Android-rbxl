use rbx_dom_weak::{
    types::{Ref, Variant},
    InstanceBuilder, WeakDom,
};
use rbx_reflection::DataType;
use rbx_reflection_database::get as get_reflection_database;
use std::collections::BTreeMap;

/// In newer `rbx_reflection_database`, `get()` returns a `Result` because the
/// embedded database can (theoretically) fail to load. On every shipped build
/// it resolves, so fall back to an empty database on error — callers treat a
/// missing class/enum the same as "not found".
fn database() -> &'static rbx_reflection::ReflectionDatabase<'static> {
    match get_reflection_database() {
        Ok(db) => db,
        Err(_) => {
            // No global const empty database is exposed; use a leaked empty one
            // so the signature stays a simple shared reference.
            static EMPTY: std::sync::OnceLock<rbx_reflection::ReflectionDatabase<'static>> =
                std::sync::OnceLock::new();
            EMPTY.get_or_init(rbx_reflection::ReflectionDatabase::new)
        }
    }
}

pub struct SchemaClassInfo {
    pub name: String,
    pub superclass: Option<String>,
    pub property_count: usize,
    pub is_creatable: bool,
}

/// Search all official Roblox Engine classes from rbx_reflection_database
pub fn search_engine_classes(query: &str) -> Vec<SchemaClassInfo> {
    let db = database();
    let q = query.trim().to_lowercase();

    let mut classes: Vec<SchemaClassInfo> = db
        .classes
        .values()
        .filter(|desc| {
            let is_not_creatable = desc.tags.contains(&rbx_reflection::ClassTag::NotCreatable)
                || desc.tags.contains(&rbx_reflection::ClassTag::Service)
                || desc.tags.contains(&rbx_reflection::ClassTag::Deprecated);

            if q.is_empty() {
                !is_not_creatable
            } else {
                desc.name.to_lowercase().contains(&q)
            }
        })
        .map(|desc| {
            let is_creatable = !desc.tags.contains(&rbx_reflection::ClassTag::NotCreatable)
                && !desc.tags.contains(&rbx_reflection::ClassTag::Service);

            SchemaClassInfo {
                name: desc.name.to_string(),
                superclass: desc.superclass.as_ref().map(|s| s.to_string()),
                property_count: desc.properties.len(),
                is_creatable,
            }
        })
        .collect();

    classes.sort_by(|a, b| a.name.cmp(&b.name));
    classes
}

/// Retrieve all available schema properties and their official types for a class
pub fn get_class_schema_properties(class_name: &str) -> BTreeMap<String, String> {
    let db = database();
    let mut out = BTreeMap::new();

    let mut current_class = Some(class_name);
    while let Some(cls) = current_class {
        if let Some(desc) = db.classes.get(cls) {
            for (prop_name, prop_desc) in &desc.properties {
                let type_name = format!("{:?}", prop_desc.data_type);
                out.entry(prop_name.to_string()).or_insert(type_name);
            }
            current_class = desc.superclass.as_deref();
        } else {
            break;
        }
    }

    out
}

/// Retrieve all valid Roblox Enum items for an enum type
pub fn get_enum_items(enum_name: &str) -> Vec<String> {
    let db = database();
    if let Some(enum_desc) = db.enums.get(enum_name) {
        let mut items: Vec<String> = enum_desc.items.keys().map(|k| k.to_string()).collect();
        items.sort();
        items
    } else {
        Vec::new()
    }
}

/// Resolve the official type of a property on a class, walking the superclass
/// chain. Returns the reflection `DataType` (either a concrete `VariantType` or
/// a named Roblox `Enum`), so the properties editor can render the right
/// widget (e.g. a dropdown for enums instead of a raw integer).
pub fn resolve_property_type(class_name: &str, prop_name: &str) -> Option<DataType<'static>> {
    let db = database();
    let mut current_class = Some(class_name);
    while let Some(cls) = current_class {
        if let Some(desc) = db.classes.get(cls) {
            if let Some(pd) = desc.properties.get(prop_name) {
                return Some(pd.data_type.clone());
            }
            current_class = desc.superclass.as_deref();
        } else {
            break;
        }
    }
    None
}

/// If a property resolves to a Roblox enum, return (enum_name, items sorted).
pub fn resolve_enum(class_name: &str, prop_name: &str) -> Option<(String, Vec<(String, u32)>)> {
    let db = database();
    if let DataType::Enum(enum_name) = resolve_property_type(class_name, prop_name)? {
        if let Some(ed) = db.enums.get(enum_name) {
            let mut items: Vec<(String, u32)> = ed
                .items
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect();
            items.sort_by_key(|(_, v)| *v);
            return Some((enum_name.to_string(), items));
        }
    }
    None
}

/// Instantiates any official Roblox class from the reflection database with default properties
pub fn create_instance_from_schema(
    dom: &mut WeakDom,
    parent: Ref,
    class_name: &str,
    name: &str,
) -> Result<Ref, anyhow::Error> {
    let db = database();
    let mut builder = InstanceBuilder::new(class_name).with_name(name);

    if let Some(desc) = db.classes.get(class_name) {
        for (prop_key, default_val) in &desc.default_properties {
            builder = builder.with_property::<&str, Variant>(prop_key.as_ref(), default_val.clone());
        }
    }

    if matches!(class_name, "Script" | "LocalScript" | "ModuleScript") {
        builder = builder.with_property("Source", Variant::String(String::new()));
    }

    Ok(dom.insert(parent, builder))
}
