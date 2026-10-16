use rbx_dom_weak::{
    types::{Ref, Variant},
    InstanceBuilder, WeakDom,
};
use rbx_reflection::{ClassDescriptor, DataType, PropertyDescriptor, PropertyKind};
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

pub fn class_exists(class_name: &str) -> bool {
    database().classes.contains_key(class_name)
}

pub fn class_is_service(class_name: &str) -> bool {
    database().classes.get(class_name)
        .is_some_and(|desc| desc.tags.contains(&rbx_reflection::ClassTag::Service))
}

/// Test Roblox API-dump inheritance, including classes not covered by the
/// command VM's small set of hand-written fallback aliases. The chain crosses a
/// class the reflection database omits, so `Part` is a `BasePart` even though
/// nothing describes `FormFactorPart`.
pub fn class_is_subclass_of(class_name: &str, ancestor_name: &str) -> bool {
    // Case-sensitive, like `Instance:IsA` itself.
    class_chain(class_name).iter().any(|name| *name == ancestor_name)
}

pub fn class_is_creatable(class_name: &str) -> bool {
    database().classes.get(class_name).is_some_and(|desc| {
        !desc.tags.contains(&rbx_reflection::ClassTag::NotCreatable)
            && !desc.tags.contains(&rbx_reflection::ClassTag::Service)
    })
}

/// Return the runtime default for a reflected property. Prefer the class dump's
/// explicit default, then synthesize a safe default from its reflected type.
pub fn default_property_value(class_name: &str, property_name: &str) -> Option<Variant> {
    let db = database();
    // The same chain as the type lookup: a default declared on `BasePart` has to
    // survive the `FormFactorPart` gap, or `Part.Transparency` synthesizes a value
    // the engine never uses.
    for name in class_chain(class_name) {
        let Some(class) = db.classes.get(name) else {
            continue;
        };
        if let Some(value) = class.default_properties.get(property_name) {
            return Some(value.clone());
        }
    }
    let data_type = resolve_property_type(class_name, property_name)?;
    default_variant_for(&data_type)
}

pub fn enum_item_exists(enum_name: &str, item_name: &str) -> Option<bool> {
    database().enums.get(enum_name)
        .map(|desc| desc.items.contains_key(item_name))
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

/// Return reflected enum item names and their engine-defined numeric values.
pub fn get_enum_items_with_values(enum_name: &str) -> Vec<(String, u32)> {
    let db = database();
    let mut items: Vec<(String, u32)> = db.enums.get(enum_name).map_or_else(Vec::new, |desc| {
        desc.items
            .iter()
            .map(|(name, value)| (name.to_string(), *value))
            .collect()
    });
    items.sort_by_key(|(name, value)| (*value, name.clone()));
    items
}

/// One class up, preferring the reflection database's link and falling back to
/// the built-in API dump. The database omits abstract classes - `Part` reaches
/// `BasePart` through `FormFactorPart`, which it does not describe - and a walk
/// that stops at that gap loses every property inherited above it.
fn superclass_of(class: &str) -> Option<&'static str> {
    if let Some(desc) = database().classes.get(class) {
        if let Some(parent) = desc.superclass {
            return Some(parent);
        }
    }
    crate::roblox_api_data::class_superclass(class)
}

/// `class` and each of its ancestors, deepest first. A missing entry does not end
/// the chain (see [`superclass_of`]), and the depth limit is a cycle guard rather
/// than a real bound: no Roblox class is more than a handful of hops from Instance.
fn class_chain(class_name: &str) -> Vec<&str> {
    let mut chain: Vec<&str> = Vec::with_capacity(16);
    let mut current = Some(class_name);
    for _ in 0..64 {
        let Some(name) = current else { break };
        if chain.contains(&name) {
            break;
        }
        chain.push(name);
        current = superclass_of(name);
    }
    chain
}

/// Resolve the official type of a property on a class, walking the superclass
/// chain. Returns the reflection `DataType` (either a concrete `VariantType` or
/// a named Roblox `Enum`), so the properties editor can render the right
/// widget (e.g. a dropdown for enums instead of a raw integer).
pub fn resolve_property_type(class_name: &str, prop_name: &str) -> Option<DataType<'static>> {
    let db = database();
    for cls in class_chain(class_name) {
        if let Some(desc) = db.classes.get(cls) {
            if let Some(pd) = desc.properties.get(prop_name) {
                return Some(pd.data_type.clone());
            }
            // The database carries an entry per spelling, with the legacy one
            // (`size`) marked as an alias of the canonical property. A lookup by
            // either name therefore has to land on the same descriptor, which the
            // case-insensitive retry below arranges; following the alias keeps the
            // type of an alias identical to its target even if the database ever
            // records them differently.
            if let Some(pd) = desc
                .properties
                .iter()
                .find(|(key, property)| {
                    let key: &str = *key;
                    key.eq_ignore_ascii_case(prop_name)
                        || property.name.eq_ignore_ascii_case(prop_name)
                })
                .map(|(_, property)| property)
            {
                if let PropertyKind::Alias { alias_for } = &pd.kind {
                    if let Some(target) = desc.properties.get(*alias_for) {
                        return Some(target.data_type.clone());
                    }
                }
                return Some(pd.data_type.clone());
            }
        }
    }
    None
}

/// Check common Luau-to-DOM value types against reflected property metadata.
/// `None` means the property has no reflected descriptor; unsupported Roblox
/// types remain permissive so their specialized host conversions can evolve.
pub fn property_value_matches_type(
    class_name: &str,
    property_name: &str,
    value: &Variant,
) -> Option<bool> {
    use rbx_dom_weak::types::VariantType;
    let data_type = resolve_property_type(class_name, property_name)?;
    Some(match data_type {
        DataType::Enum(_) => matches!(value, Variant::Enum(_)),
        DataType::Value(VariantType::Bool) => matches!(value, Variant::Bool(_)),
        DataType::Value(VariantType::String) => matches!(value, Variant::String(_)),
        DataType::Value(VariantType::Float32) => matches!(value, Variant::Float32(_)),
        DataType::Value(VariantType::Float64) => matches!(value, Variant::Float64(_)),
        DataType::Value(VariantType::Int32) => matches!(value, Variant::Int32(_)),
        DataType::Value(VariantType::Int64) => matches!(value, Variant::Int64(_)),
        DataType::Value(VariantType::CFrame) => matches!(value, Variant::CFrame(_)),
        DataType::Value(VariantType::Vector2) => matches!(value, Variant::Vector2(_)),
        DataType::Value(VariantType::Vector3) => matches!(value, Variant::Vector3(_)),
        DataType::Value(VariantType::Color3) => matches!(value, Variant::Color3(_)),
        DataType::Value(VariantType::UDim) => matches!(value, Variant::UDim(_)),
        DataType::Value(VariantType::UDim2) => matches!(value, Variant::UDim2(_)),
        DataType::Value(VariantType::NumberRange) => matches!(value, Variant::NumberRange(_)),
        _ => true,
    })
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

/// Build a sensible default `Variant` for a reflection-known property type,
/// so the Properties editor can add a class property that isn't present in the
/// file yet (Studio shows the full property list for a class — we mirror that).
/// Returns `None` for types we don't know how to default (rare ones like Tags,
/// Attributes, PhysicalProperties); callers fall back to a string.
pub fn default_variant_for(dt: &DataType<'_>) -> Option<Variant> {
    use rbx_dom_weak::types::{
        CFrame, Color3, Color3uint8, Enum, NumberRange, UDim, UDim2, VariantType, Vector2, Vector3,
    };
    match dt {
        DataType::Value(VariantType::Bool) => Some(Variant::Bool(false)),
        DataType::Value(VariantType::String) => Some(Variant::String(String::new())),
        DataType::Value(VariantType::Float32) => Some(Variant::Float32(0.0)),
        DataType::Value(VariantType::Float64) => Some(Variant::Float64(0.0)),
        DataType::Value(VariantType::Int32) => Some(Variant::Int32(0)),
        DataType::Value(VariantType::Int64) => Some(Variant::Int64(0)),
        DataType::Value(VariantType::Vector3) => {
            Some(Variant::Vector3(Vector3::new(0.0, 0.0, 0.0)))
        }
        DataType::Value(VariantType::Vector2) => Some(Variant::Vector2(Vector2::new(0.0, 0.0))),
        DataType::Value(VariantType::Color3) => Some(Variant::Color3(Color3::new(0.0, 0.0, 0.0))),
        DataType::Value(VariantType::Color3uint8) => {
            Some(Variant::Color3uint8(Color3uint8::new(0, 0, 0)))
        }
        DataType::Value(VariantType::UDim) => Some(Variant::UDim(UDim::new(0.0, 0))),
        DataType::Value(VariantType::UDim2) => Some(Variant::UDim2(UDim2::new(
            UDim::new(0.0, 0),
            UDim::new(0.0, 0),
        ))),
        DataType::Value(VariantType::NumberRange) => {
            Some(Variant::NumberRange(NumberRange::new(0.0, 0.0)))
        }
        DataType::Value(VariantType::CFrame) => Some(Variant::CFrame(CFrame::identity())),
        DataType::Enum(_) => Some(Variant::Enum(Enum::from_u32(0))),
        _ => None,
    }
}

/// Resolve a replication wire name to the class's canonical property name,
/// walking the superclass chain exactly like [`resolve_property_type`] does.
///
/// Snapshot records and delta property updates carry the *wire* spelling of a
/// property (`size`, `cframe`, …), while the DOM and both place writers key on the
/// canonical reflection name (`Size`, `CFrame`). `rbx_reflection` keeps an entry
/// per spelling and marks the legacy one `PropertyKind::Alias`, so this walk
/// resolves either name to the canonical descriptor's name — which is what makes
/// an applied snapshot round-trip through `rbx_xml`/`rbx_binary`. A name the class
/// does not declare is reported as absent instead of being written as an invented
/// property.
pub fn canonical_property_name(class_name: &str, wire_name: &str) -> Option<String> {
    if wire_name.is_empty() {
        return None;
    }
    let db = database();
    for cls in class_chain(class_name) {
        let Some(desc) = db.classes.get(cls) else {
            continue; // an abstract class the database does not describe
        };
        // An exact hit first, because a capture's wire spelling is a key of this
        // map for every legacy property and a snapshot repeats the lookup per
        // instance: walking the map is the fallback, not the norm.
        if let Some(property) = desc.properties.get(wire_name) {
            return Some(canonical_name_for(desc, property));
        }
        for (key, property) in &desc.properties {
            // Bound through a `&str`: the map key is a borrowed `str` already, and
            // reading it through a local keeps the borrow of `desc` in one place.
            let key: &str = key;
            if key.eq_ignore_ascii_case(wire_name) || property.name.eq_ignore_ascii_case(wire_name)
            {
                return Some(canonical_name_for(desc, property));
            }
        }
    }
    None
}

/// The name a property value should be stored under: an alias entry names the
/// descriptor it stands for, and everything else names itself.
fn canonical_name_for(desc: &ClassDescriptor<'_>, property: &PropertyDescriptor<'_>) -> String {
    if let PropertyKind::Alias { alias_for } = &property.kind {
        // Prefer the target's own spelling so a differently-cased alias cannot
        // leak into the DOM; fall back to the name the alias points at.
        if let Some(target) = desc.properties.get(*alias_for) {
            return target.name.to_string();
        }
        return (*alias_for).to_string();
    }
    property.name.to_string()
}

#[cfg(test)]
mod replication_helpers {
    use super::*;

    /// One line describing what the reflection database holds for a class and the
    /// chain its inheritance walk crosses, so a failure names the disagreement
    /// instead of leaving it to be inferred from one pair of values.
    fn facts(class: &str) -> String {
        let db = database();
        let mut parts = vec![format!("chain [{})", class_chain(class).join(", "))];
        for step in class_chain(class) {
            parts.push(match db.classes.get(step) {
                None => format!("{step}: undescribed"),
                Some(desc) => {
                    let mut names: Vec<&str> = desc.properties.keys().copied().collect();
                    names.sort_unstable();
                    let shown = names.len().min(6);
                    format!(
                        "{step}: {} propert(y/ies) [{}]",
                        names.len(),
                        names[..shown].join(", ")
                    )
                }
            });
        }
        parts.join(" | ")
    }

    #[test]
    fn wire_names_resolve_to_canonical_properties() {
        // The database keeps one entry per spelling and marks the legacy one as an
        // alias of the canonical property, so a snapshot's `size` has to come back
        // as `Size` - the name the DOM and both place writers use.
        assert_eq!(canonical_property_name("Part", "size").as_deref(), Some("Size"));
        assert_eq!(canonical_property_name("Part", "Size").as_deref(), Some("Size"));
        // Case-insensitivity is asserted on a name this database is known to hold,
        // which tests the walk rather than the dump's coverage.
        assert_eq!(
            canonical_property_name("Part", "SiZe").as_deref(),
            Some("Size"),
            "{}",
            facts("Part")
        );
        assert!(matches!(
            resolve_property_type("Part", "Size"),
            Some(DataType::Value(rbx_dom_weak::types::VariantType::Vector3))
        ));
        assert!(matches!(
            resolve_property_type("Part", "size"),
            Some(DataType::Value(rbx_dom_weak::types::VariantType::Vector3))
        ));
        // `Part` is described, its abstract `FormFactorPart` parent is not, and the
        // walk still has to reach `BasePart`: without that step every inherited
        // property of every part in an applied snapshot would be reported undeclared.
        assert!(class_is_subclass_of("Part", "BasePart"));
        assert!(class_is_subclass_of("Part", "Instance"));
        assert!(!class_is_subclass_of("Part", "Model"));
        // Whatever the database covers, the two lookups have to agree: a name that
        // resolves to a canonical spelling must also have a type, or the applier
        // would store a value the editor cannot interpret.
        for name in [
            "size",
            "Size",
            "SiZe",
            "shape",
            "anchored",
            "Anchored",
            "transparency",
            "name",
            "parent",
        ] {
            if let Some(canonical) = canonical_property_name("Part", name) {
                assert!(
                    resolve_property_type("Part", &canonical).is_some(),
                    "{name} -> {canonical} resolves with no type: {}",
                    facts("Part")
                );
            }
        }
        assert_eq!(canonical_property_name("Part", "definitely_not_a_property"), None);
        assert_eq!(canonical_property_name("NoSuchClassAtAll", "size"), None);
        assert_eq!(canonical_property_name("Part", ""), None);
    }
}
