use bevy_egui::egui;
use bevy_egui::egui::{CollapsingHeader, Color32, Ui};
use rbx_dom_weak::{types::Ref, WeakDom};
use std::collections::HashSet;

/// One user interaction with the Explorer tree. The tree only borrows the DOM
/// immutably while rendering; the caller applies these afterwards (selection
/// changes and drag-and-drop reparenting both mutate app state).
pub enum TreeAction {
    /// Plain click: make this row the single selection.
    Select(Ref),
    /// Ctrl+click (desktop) or any click while multi-select mode is on:
    /// toggle this row in/out of the multi-selection set.
    ToggleMulti(Ref),
    /// A row was dragged and dropped onto another row (drag mode only).
    Reparent { child: Ref, new_parent: Ref },
}

#[allow(clippy::too_many_arguments)]
pub fn show_tree_filtered(
    ui: &mut Ui,
    dom: &WeakDom,
    root: Ref,
    selected: Option<Ref>,
    multi: &[Ref],
    multi_mode: bool,
    drag_mode: bool,
    filter: &str,
) -> Vec<TreeAction> {
    let mut actions = Vec::new();
    if dom.get_by_ref(root).is_none() {
        return actions;
    }

    let filter_lower = filter.trim().to_lowercase();
    // Performance: the previous implementation re-ran a recursive
    // `matches_filter` subtree scan (with fresh `to_lowercase()`
    // allocations for every name/class) for *every node, every frame*.
    // On a large place that is O(n^2) string allocations per frame while
    // a filter is typed. Instead compute the visible set (matches plus
    // their ancestors) once per frame in a single iterative post-order
    // pass: O(n), and O(1) membership tests while rendering. The
    // explicit stack also removes recursion-depth overflow risk.
    let visible: Option<HashSet<Ref>> = if filter_lower.is_empty() {
        None
    } else {
        let mut visible = HashSet::new();
        let mut stack: Vec<(Ref, bool)> = vec![(root, false)];
        while let Some((r, expanded)) = stack.pop() {
            let Some(inst) = dom.get_by_ref(r) else { continue };
            if !expanded {
                stack.push((r, true));
                for child in inst.children() {
                    stack.push((*child, false));
                }
            } else if inst.name.to_lowercase().contains(&filter_lower)
                || inst.class.to_lowercase().contains(&filter_lower)
                || inst.children().iter().any(|c| visible.contains(c))
            {
                visible.insert(r);
            }
        }
        if !visible.contains(&root) {
            return actions;
        }
        Some(visible)
    };
    // `multi` was a slice scanned linearly for every rendered row.
    let multi_set: HashSet<Ref> = multi.iter().copied().collect();

    render_node(
        ui,
        dom,
        root,
        selected,
        &multi_set,
        multi_mode,
        drag_mode,
        visible.as_ref(),
        &mut actions,
    );
    actions
}

#[allow(clippy::too_many_arguments)]
fn render_node(
    ui: &mut Ui,
    dom: &WeakDom,
    root: Ref,
    selected: Option<Ref>,
    multi: &HashSet<Ref>,
    multi_mode: bool,
    drag_mode: bool,
    visible: Option<&HashSet<Ref>>,
    actions: &mut Vec<TreeAction>,
) {
    let Some(inst) = dom.get_by_ref(root) else {
        return;
    };
    let is_primary = selected == Some(root);
    let in_multi = multi.contains(&root);
    let is_selected = is_primary || in_multi;
    let icon = class_icon(&inst.class);
    let label = format!("{} {}", icon, inst.name);

    // Primary selection = blue, extra multi-selected rows = amber.
    let text = if is_primary {
        egui::RichText::new(label).color(Color32::from_rgb(100, 200, 255)).strong()
    } else if in_multi {
        egui::RichText::new(label).color(Color32::from_rgb(255, 210, 120)).strong()
    } else {
        egui::RichText::new(label)
    };

    let row_response = if inst.children().is_empty() {
        let mut resp = ui.selectable_label(is_selected, text);
        if drag_mode {
            resp = resp.interact(egui::Sense::click_and_drag());
        }
        if resp.clicked() {
            push_click(ui, root, multi_mode, actions);
        }
        resp
    } else {
        let is_open_default = visible.is_some() || root == dom.root_ref();

        let header = CollapsingHeader::new(text)
            .id_salt(root)
            .default_open(is_open_default)
            .show(ui, |ui| {
                for child in inst.children() {
                    if visible.map_or(true, |v| v.contains(child)) {
                        render_node(
                            ui, dom, *child, selected, multi, multi_mode, drag_mode,
                            visible, actions,
                        );
                    }
                }
            });

        let mut resp = header.header_response;
        if drag_mode {
            resp = resp.interact(egui::Sense::click_and_drag());
        }
        if resp.clicked() {
            push_click(ui, root, multi_mode, actions);
        }
        resp
    };

    // Drag-and-drop reparenting (only when drag mode is on, so normal touch
    // scrolling of the tree keeps working on Android by default).
    if drag_mode {
        // Every row is a drag source…
        row_response.dnd_set_drag_payload(root);

        // …and a drop target: dropping row A onto row B parents A under B.
        if let Some(payload) = row_response.dnd_hover_payload::<Ref>() {
            if *payload != root {
                ui.painter().rect_stroke(
                    row_response.rect,
                    2.0_f32,
                    egui::Stroke::new(1.5_f32, Color32::from_rgb(120, 220, 120)),
                    egui::StrokeKind::Outside,
                );
            }
        }
        if let Some(payload) = row_response.dnd_release_payload::<Ref>() {
            if *payload != root {
                actions.push(TreeAction::Reparent { child: *payload, new_parent: root });
            }
        }
    }
}

fn push_click(ui: &Ui, root: Ref, multi_mode: bool, actions: &mut Vec<TreeAction>) {
    let ctrl_held = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
    if multi_mode || ctrl_held {
        actions.push(TreeAction::ToggleMulti(root));
    } else {
        actions.push(TreeAction::Select(root));
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
