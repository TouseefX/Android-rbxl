use bevy_egui::egui;
use bevy_egui::egui::{CollapsingHeader, Color32, Ui};
use rbx_dom_weak::{types::Ref, WeakDom};

pub fn show_tree_filtered(
    ui: &mut Ui,
    dom: &WeakDom,
    root: Ref,
    selected: &mut Option<Ref>,
    filter: &str,
) {
    if dom.get_by_ref(root).is_none() {
        return;
    }

    let filter_lower = filter.trim().to_lowercase();
    if !filter_lower.is_empty() && !matches_filter(dom, root, &filter_lower) {
        return;
    }

    render_node(ui, dom, root, selected, &filter_lower);
}

fn matches_filter(dom: &WeakDom, r: Ref, query: &str) -> bool {
    let Some(inst) = dom.get_by_ref(r) else {
        return false;
    };
    if inst.name.to_lowercase().contains(query) || inst.class.to_lowercase().contains(query) {
        return true;
    }
    for child in inst.children() {
        if matches_filter(dom, *child, query) {
            return true;
        }
    }
    false
}

fn render_node(
    ui: &mut Ui,
    dom: &WeakDom,
    root: Ref,
    selected: &mut Option<Ref>,
    filter: &str,
) {
    let Some(inst) = dom.get_by_ref(root) else {
        return;
    };
    let is_selected = *selected == Some(root);
    let icon = class_icon(&inst.class);
    let label = format!("{} {}", icon, inst.name);

    if inst.children().is_empty() {
        let text = if is_selected {
            egui::RichText::new(label).color(Color32::from_rgb(100, 200, 255)).strong()
        } else {
            egui::RichText::new(label)
        };

        if ui.selectable_label(is_selected, text).clicked() {
            *selected = Some(root);
        }
    } else {
        let is_open_default = !filter.is_empty() || root == dom.root_ref();
        let header_text = if is_selected {
            egui::RichText::new(label).color(Color32::from_rgb(100, 200, 255)).strong()
        } else {
            egui::RichText::new(label)
        };

        let header = CollapsingHeader::new(header_text)
            .id_salt(root)
            .default_open(is_open_default)
            .show(ui, |ui| {
                for child in inst.children() {
                    if filter.is_empty() || matches_filter(dom, *child, filter) {
                        render_node(ui, dom, *child, selected, filter);
                    }
                }
            });

        if header.header_response.clicked() {
            *selected = Some(root);
        }
    }
}

pub fn class_icon(class: &str) -> &'static str {
    match class {
        "Script" => "📜",
        "LocalScript" => "📄",
        "ModuleScript" => "📦",
        "Folder" => "📁",
        "Workspace" => "🌐",
        "Players" => "👥",
        "Lighting" => "💡",
        "ReplicatedStorage" => "🔄",
        "ReplicatedFirst" => "⚡",
        "ServerScriptService" => "🖥️",
        "ServerStorage" => "🗄️",
        "StarterGui" => "📱",
        "StarterPack" => "🎒",
        "StarterPlayer" => "🏃",
        "SoundService" => "🔊",
        "HttpService" => "🌐",
        "Tool" => "⚔️",
        "MeshPart" | "SpecialMesh" => "🗿",
        "Part" | "WedgePart" | "CornerWedgePart" | "TrussPart" | "UnionOperation" => "🧱",
        "SpawnLocation" => "🚩",
        "Model" => "📦",
        "Humanoid" => "👤",
        "Attachment" | "Weld" | "Motor6D" | "WeldConstraint" => "⛓️",
        "Animation" | "AnimationTrack" => "🎬",
        "Configuration" => "⚙️",
        "PackageLink" => "📦",
        "Decal" | "Texture" => "🎨",
        "ProximityPrompt" | "ClickDetector" => "👆",
        "Fire" | "Smoke" | "Sparkles" => "🔥",
        "RemoteEvent" => "📡",
        "RemoteFunction" => "📞",
        "BindableEvent" | "BindableFunction" => "🔗",
        "ScreenGui" | "BillboardGui" | "SurfaceGui" => "🖼️",
        "Frame" | "ScrollingFrame" => "🔲",
        "TextLabel" => "🏷️",
        "TextButton" => "🔘",
        "TextBox" => "✍️",
        "ImageLabel" | "ImageButton" => "🖼️",
        "Sound" => "🎵",
        "PointLight" | "SpotLight" | "SurfaceLight" => "✨",
        "ParticleEmitter" => "🎆",
        "Highlight" => "🌟",
        "StringValue" | "IntValue" | "NumberValue" | "BoolValue" | "Color3Value" | "Vector3Value" => "🔢",
        "Camera" => "📷",
        "Terrain" => "🏔️",
        _ => "🔹",
    }
}
