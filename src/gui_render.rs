//! Roblox GUI preview rendered through egui over the Bevy viewport.
//! This first stage covers StarterGui/ScreenGui layout, frames, text and input
//! selection. Image and 3D BillboardGui rendering build on the same tree.

use bevy_egui::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use rbx_dom_weak::{types::{Ref, Variant}, WeakDom};

struct GuiNode {
    referent: Ref,
    class: String,
    rect: Rect,
    clip: Rect,
    z: i32,
    background: Color32,
    border: Color32,
    border_size: f32,
    text: String,
    text_color: Color32,
    text_size: f32,
    image: Option<String>,
    image_color: Color32,
}

fn number(value: Option<&Variant>, fallback: f32) -> f32 {
    match value {
        Some(Variant::Float32(v)) => *v,
        Some(Variant::Float64(v)) => *v as f32,
        Some(Variant::Int32(v)) => *v as f32,
        Some(Variant::Int64(v)) => *v as f32,
        _ => fallback,
    }
}

fn color(value: Option<&Variant>, alpha: u8, fallback: [u8; 3]) -> Color32 {
    match value {
        Some(Variant::Color3(v)) => Color32::from_rgba_unmultiplied(
            (v.r * 255.0).round() as u8, (v.g * 255.0).round() as u8,
            (v.b * 255.0).round() as u8, alpha),
        Some(Variant::Color3uint8(v)) => Color32::from_rgba_unmultiplied(v.r, v.g, v.b, alpha),
        _ => Color32::from_rgba_unmultiplied(fallback[0], fallback[1], fallback[2], alpha),
    }
}

fn content(value: Option<&Variant>) -> Option<String> {
    match value {
        Some(Variant::String(value)) if !value.is_empty() => Some(value.clone()),
        Some(Variant::ContentId(value)) if !value.as_str().is_empty() => Some(value.as_str().to_string()),
        Some(Variant::Content(value)) => value.as_uri().map(str::to_string),
        Some(Variant::Int64(value)) if *value > 0 => Some(format!("rbxassetid://{value}")),
        _ => None,
    }
}

fn visible(instance: &rbx_dom_weak::Instance) -> bool {
    !matches!(instance.properties.get(&rbx_dom_weak::ustr("Visible")), Some(Variant::Bool(false)))
}

fn gui_rect(instance: &rbx_dom_weak::Instance, parent: Rect) -> Rect {
    let position = match instance.properties.get(&rbx_dom_weak::ustr("Position")) {
        Some(Variant::UDim2(v)) => Vec2::new(
            parent.width() * v.x.scale + v.x.offset as f32,
            parent.height() * v.y.scale + v.y.offset as f32),
        _ => Vec2::ZERO,
    };
    let size = match instance.properties.get(&rbx_dom_weak::ustr("Size")) {
        Some(Variant::UDim2(v)) => Vec2::new(
            parent.width() * v.x.scale + v.x.offset as f32,
            parent.height() * v.y.scale + v.y.offset as f32),
        _ => Vec2::new(100.0, 100.0),
    }.max(Vec2::ZERO);
    let anchor = match instance.properties.get(&rbx_dom_weak::ustr("AnchorPoint")) {
        Some(Variant::Vector2(v)) => Vec2::new(v.x, v.y),
        _ => Vec2::ZERO,
    };
    let min = parent.min + position - size * anchor;
    Rect::from_min_size(min, size)
}

fn collect(dom: &WeakDom, referent: Ref, parent_rect: Rect, parent_clip: Rect,
           inherited_z: i32, nodes: &mut Vec<GuiNode>) {
    let Some(instance) = dom.get_by_ref(referent) else { return; };
    if !visible(instance) { return; }
    let is_screen = instance.class == "ScreenGui";
    let rect = if is_screen { parent_rect } else { gui_rect(instance, parent_rect) };
    let clips = matches!(instance.properties.get(&rbx_dom_weak::ustr("ClipsDescendants")), Some(Variant::Bool(true)));
    let child_clip = if clips { parent_clip.intersect(rect) } else { parent_clip };
    let z = inherited_z + match instance.properties.get(&rbx_dom_weak::ustr("ZIndex")) {
        Some(Variant::Int32(v)) => *v,
        Some(Variant::Int64(v)) => *v as i32,
        _ => 1,
    };

    if matches!(instance.class.as_str(), "Frame" | "TextLabel" | "TextButton" |
        "ImageLabel" | "ImageButton" | "ScrollingFrame" | "ViewportFrame") {
        let transparency = number(instance.properties.get(&rbx_dom_weak::ustr("BackgroundTransparency")), 0.0).clamp(0.0, 1.0);
        let alpha = ((1.0 - transparency) * 255.0).round() as u8;
        let text_transparency = number(instance.properties.get(&rbx_dom_weak::ustr("TextTransparency")), 0.0).clamp(0.0, 1.0);
        nodes.push(GuiNode {
            referent,
            class: instance.class.to_string(),
            rect,
            clip: parent_clip.intersect(rect),
            z,
            background: color(instance.properties.get(&rbx_dom_weak::ustr("BackgroundColor3")), alpha, [255,255,255]),
            border: color(instance.properties.get(&rbx_dom_weak::ustr("BorderColor3")), 255, [27,42,53]),
            border_size: number(instance.properties.get(&rbx_dom_weak::ustr("BorderSizePixel")), 1.0).max(0.0),
            text: match instance.properties.get(&rbx_dom_weak::ustr("Text")) { Some(Variant::String(v)) => v.clone(), _ => String::new() },
            text_color: color(instance.properties.get(&rbx_dom_weak::ustr("TextColor3")), ((1.0-text_transparency)*255.0) as u8, [0,0,0]),
            text_size: number(instance.properties.get(&rbx_dom_weak::ustr("TextSize")), 14.0).clamp(6.0, 100.0),
            image: content(instance.properties.get(&rbx_dom_weak::ustr("Image"))
                .or_else(|| instance.properties.get(&rbx_dom_weak::ustr("ImageContent")))),
            image_color: color(
                instance.properties.get(&rbx_dom_weak::ustr("ImageColor3")),
                ((1.0-number(instance.properties.get(&rbx_dom_weak::ustr("ImageTransparency")), 0.0).clamp(0.0,1.0))*255.0) as u8,
                [255,255,255],
            ),
        });
    }
    for child in instance.children() {
        collect(dom, *child, rect, child_clip, z, nodes);
    }
}

/// Draw enabled ScreenGuis under StarterGui and return the topmost clicked
/// Roblox referent, allowing the editor to synchronize viewport and Explorer.
pub fn draw_starter_gui(
    ui: &mut egui::Ui,
    viewport: Rect,
    dom: &WeakDom,
    textures: &mut std::collections::HashMap<String, egui::TextureHandle>,
) -> Option<Ref> {
    let Some(starter) = dom.root().children().iter().find_map(|referent| {
        dom.get_by_ref(*referent).filter(|instance| instance.class == "StarterGui").map(|_| *referent)
    }) else { return None; };
    let mut nodes = Vec::new();
    if let Some(starter) = dom.get_by_ref(starter) {
        for child in starter.children() {
            let Some(gui) = dom.get_by_ref(*child) else { continue; };
            if gui.class != "ScreenGui" || matches!(gui.properties.get(&rbx_dom_weak::ustr("Enabled")), Some(Variant::Bool(false))) { continue; }
            collect(dom, *child, viewport, viewport, 0, &mut nodes);
        }
    }
    nodes.sort_by_key(|node| node.z);
    let mut clicked = None;
    for node in nodes {
        if node.clip.width() <= 0.0 || node.clip.height() <= 0.0 { continue; }
        let response = ui.interact(node.clip, ui.make_persistent_id(("roblox_gui", format!("{:?}", node.referent))), egui::Sense::click());
        let painter = ui.painter().with_clip_rect(node.clip);
        painter.rect_filled(node.rect, 0.0, node.background);
        if node.border_size > 0.0 {
            painter.rect_stroke(node.rect, 0.0, Stroke::new(node.border_size, node.border), egui::StrokeKind::Inside);
        }
        if let Some(uri) = &node.image {
            if !textures.contains_key(uri) {
                let decoded = crate::asset_downloader::get_cached_image(uri).or_else(|| {
                    crate::asset_downloader::extract_asset_id(uri)
                        .and_then(|id| crate::asset_downloader::get_cached_image(&id))
                });
                if let Some(image) = decoded {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width, image.height], &image.rgba,
                    );
                    textures.insert(uri.clone(), ui.ctx().load_texture(
                        uri, color_image, egui::TextureOptions::LINEAR,
                    ));
                }
            }
            if let Some(texture) = textures.get(uri) {
                painter.image(texture.id(), node.rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), node.image_color);
            }
        }
        if !node.text.is_empty() {
            painter.text(node.rect.center(), egui::Align2::CENTER_CENTER, node.text,
                FontId::proportional(node.text_size), node.text_color);
        }
        if response.clicked() { clicked = Some(node.referent); }
    }
    clicked
}
