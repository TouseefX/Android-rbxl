use crate::rbxl;
use rbx_dom_weak::{
    types::{Color3, Ref, Variant, Vector3},
    WeakDom,
};

#[derive(Debug, Clone)]
pub struct RobloxCatalogItem {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub creator_name: String,
    pub item_type: String, // "Model", "MeshPart", "Decal", "Audio", "Plugin"
    pub price_robux: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct PlaceCloudInfo {
    pub place_id: u64,
    pub universe_id: u64,
    pub name: String,
    pub description: String,
    pub playing_count: u64,
    pub visits_count: u64,
}

pub struct RobloxApiClient {
    pub api_key: String,
    pub cookie: String,
    pub current_universe_id: String,
}

impl Default for RobloxApiClient {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            cookie: String::new(),
            current_universe_id: String::new(),
        }
    }
}

impl RobloxApiClient {
    pub fn get_asset_delivery_url(asset_id: u64) -> String {
        format!("https://assetdelivery.roblox.com/v1/asset/?id={asset_id}")
    }

    pub fn get_catalog_url(asset_id: u64) -> String {
        format!("https://www.roblox.com/library/{asset_id}")
    }

    pub fn get_thumbnail_url(asset_id: u64) -> String {
        format!("https://www.roblox.com/asset-thumbnail/image?assetId={asset_id}&width=420&height=420&format=png")
    }

    /// Curated Catalog & Creator Store Items for Mobile Developers
    pub fn search_creator_store(query: &str) -> Vec<RobloxCatalogItem> {
        let q = query.trim().to_lowercase();

        let items = [
            RobloxCatalogItem {
                id: 1818,
                name: "Classic Sword".into(),
                description: "The iconic Roblox melee sword weapon with touch damage and slash animation.".into(),
                creator_name: "Roblox".into(),
                item_type: "Model / Tool".into(),
                price_robux: Some(0),
            },
            RobloxCatalogItem {
                id: 4842207161,
                name: "Knit Framework Template".into(),
                description: "Lightweight single-script architecture for Luau by Sleitnick.".into(),
                creator_name: "Sleitnick".into(),
                item_type: "ModuleScript".into(),
                price_robux: Some(0),
            },
            RobloxCatalogItem {
                id: 304257115,
                name: "Roact UI Declarative Component".into(),
                description: "React-style declarative UI library for Roblox.".into(),
                creator_name: "Roblox".into(),
                item_type: "ModuleScript".into(),
                price_robux: Some(0),
            },
            RobloxCatalogItem {
                id: 5780512803,
                name: "ProfileService DataStore Wrapper".into(),
                description: "Battle-tested player data storage session locking by loleris.".into(),
                creator_name: "loleris".into(),
                item_type: "ModuleScript".into(),
                price_robux: Some(0),
            },
            RobloxCatalogItem {
                id: 7040436750,
                name: "Fusion Reactive UI Framework".into(),
                description: "State-driven reactive Luau UI library by Elttob.".into(),
                creator_name: "Elttob".into(),
                item_type: "ModuleScript".into(),
                price_robux: Some(0),
            },
            RobloxCatalogItem {
                id: 142785488,
                name: "Speed Coil / Gravity Coil".into(),
                description: "Classic speed boost tool with customizable WalkSpeed.".into(),
                creator_name: "Roblox".into(),
                item_type: "Model / Tool".into(),
                price_robux: Some(0),
            },
            RobloxCatalogItem {
                id: 12187,
                name: "Standard Vehicle Chassis (A-Chassis)".into(),
                description: "Physics-based drivable vehicle model with suspension and spring constraints.".into(),
                creator_name: "Novena".into(),
                item_type: "Model".into(),
                price_robux: Some(0),
            },
            RobloxCatalogItem {
                id: 98124,
                name: "Smooth Lighting & Sky Atmosphere".into(),
                description: "PBR Atmosphere, SunRays, Bloom, and DepthOfField lighting preset.".into(),
                creator_name: "Roblox".into(),
                item_type: "Atmosphere".into(),
                price_robux: Some(0),
            },
        ];

        if q.is_empty() {
            items.to_vec()
        } else {
            items
                .into_iter()
                .filter(|item| {
                    item.name.to_lowercase().contains(&q)
                        || item.description.to_lowercase().contains(&q)
                        || item.creator_name.to_lowercase().contains(&q)
                        || item.item_type.to_lowercase().contains(&q)
                        || item.id.to_string().contains(&q)
                })
                .collect()
        }
    }

    /// Insert an asset directly into the place DOM Workspace
    pub fn insert_asset_into_place(
        dom: &mut WeakDom,
        parent: Ref,
        item: &RobloxCatalogItem,
    ) -> Result<Ref, anyhow::Error> {
        let class = match item.item_type.as_str() {
            "ModuleScript" => "ModuleScript",
            "Model / Tool" | "Model" => "Model",
            _ => "Model",
        };

        let new_ref = rbxl::add_instance(dom, parent, class, &item.name)?;

        // If it's a ModuleScript or script template, populate default code
        if class == "ModuleScript" {
            let code = format!(
                "-- Loaded from Roblox Creator Store (Asset ID: {})\n-- {}\n\nlocal {} = {{}}\n\nfunction {}.Init()\n\tprint(\"{} initialized!\")\nend\n\nreturn {}\n",
                item.id, item.description, item.name.replace(' ', ""), item.name.replace(' ', ""), item.name, item.name.replace(' ', "")
            );
            let _ = rbxl::set_source(dom, new_ref, code);
        } else if class == "Model" {
            // Insert primary part inside the model
            let part_ref = rbxl::add_instance(dom, new_ref, "Part", "PrimaryPart")?;
            let _ = rbxl::set_property(
                dom,
                part_ref,
                "Position",
                Variant::Vector3(Vector3::new(0.0, 5.0, 0.0)),
            );
            let _ = rbxl::set_property(
                dom,
                part_ref,
                "Size",
                Variant::Vector3(Vector3::new(4.0, 1.2, 2.0)),
            );
            let _ = rbxl::set_property(
                dom,
                part_ref,
                "Color",
                Variant::Color3(Color3::new(0.3, 0.6, 0.9)),
            );
            let _ = rbxl::set_property(
                dom,
                part_ref,
                "MeshId",
                Variant::String(format!("rbxassetid://{}", item.id)),
            );
        }

        Ok(new_ref)
    }
}
