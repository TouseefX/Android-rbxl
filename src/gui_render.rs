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
    order: usize,
    background: Color32,
    border: Color32,
    border_size: f32,
    text: String,
    text_color: Color32,
    text_size: f32,
    text_x_alignment: i32,
    text_y_alignment: i32,
    text_wrapped: bool,
    text_scaled: bool,
    text_stroke: Color32,
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

fn enum_value(value: Option<&Variant>, fallback: i32) -> i32 {
    match value {
        Some(Variant::Enum(value)) => value.clone().to_u32() as i32,
        Some(Variant::Int32(value)) => *value,
        Some(Variant::Int64(value)) => *value as i32,
        _ => fallback,
    }
}

fn bool_value(value: Option<&Variant>, fallback: bool) -> bool {
    match value { Some(Variant::Bool(value)) => *value, _ => fallback }
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
           inherited_z: i32, display_order: i32, sequence: &mut usize,
           nodes: &mut Vec<GuiNode>) {
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
        let order = *sequence;
        *sequence += 1;
        nodes.push(GuiNode {
            referent,
            class: instance.class.to_string(),
            rect,
            clip: parent_clip.intersect(rect),
            z: display_order.saturating_mul(1_000_000).saturating_add(z),
            order,
            background: color(instance.properties.get(&rbx_dom_weak::ustr("BackgroundColor3")), alpha, [255,255,255]),
            border: color(instance.properties.get(&rbx_dom_weak::ustr("BorderColor3")), 255, [27,42,53]),
            border_size: number(instance.properties.get(&rbx_dom_weak::ustr("BorderSizePixel")), 1.0).max(0.0),
            text: match instance.properties.get(&rbx_dom_weak::ustr("Text")) { Some(Variant::String(v)) => v.clone(), _ => String::new() },
            text_color: color(instance.properties.get(&rbx_dom_weak::ustr("TextColor3")), ((1.0-text_transparency)*255.0) as u8, [0,0,0]),
            text_size: number(instance.properties.get(&rbx_dom_weak::ustr("TextSize")), 14.0).clamp(1.0, 200.0),
            text_x_alignment: enum_value(instance.properties.get(&rbx_dom_weak::ustr("TextXAlignment")), 1),
            text_y_alignment: enum_value(instance.properties.get(&rbx_dom_weak::ustr("TextYAlignment")), 1),
            text_wrapped: bool_value(instance.properties.get(&rbx_dom_weak::ustr("TextWrapped")), false),
            text_scaled: bool_value(instance.properties.get(&rbx_dom_weak::ustr("TextScaled")), false),
            text_stroke: color(
                instance.properties.get(&rbx_dom_weak::ustr("TextStrokeColor3")),
                ((1.0-number(instance.properties.get(&rbx_dom_weak::ustr("TextStrokeTransparency")), 1.0).clamp(0.0,1.0))*255.0) as u8,
                [0,0,0],
            ),
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
        collect(dom, *child, rect, child_clip, z, display_order, sequence, nodes);
    }
}

/// Draw enabled ScreenGuis under StarterGui and return the topmost clicked
/// Roblox referent, allowing the editor to synchronize viewport and Explorer.
pub fn draw_starter_gui(
    ui: &mut egui::Ui,
    viewport: Rect,
    dom: &WeakDom,
    textures: &mut std::collections::HashMap<String, egui::TextureHandle>,
    selected: Option<Ref>,
) -> Option<Ref> {
    let Some(starter) = dom.root().children().iter().find_map(|referent| {
        dom.get_by_ref(*referent).filter(|instance| instance.class == "StarterGui").map(|_| *referent)
    }) else { return None; };
    let mut nodes = Vec::new();
    let mut sequence = 0;
    if let Some(starter) = dom.get_by_ref(starter) {
        for child in starter.children() {
            let Some(gui) = dom.get_by_ref(*child) else { continue; };
            if gui.class != "ScreenGui" || matches!(gui.properties.get(&rbx_dom_weak::ustr("Enabled")), Some(Variant::Bool(false))) { continue; }
            let display_order = enum_value(gui.properties.get(&rbx_dom_weak::ustr("DisplayOrder")), 0);
            collect(dom, *child, viewport, viewport, 0, display_order, &mut sequence, &mut nodes);
        }
    }
    nodes.sort_by_key(|node| (node.z, node.order));
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
            let wrap_width = if node.text_wrapped { node.rect.width() } else { f32::INFINITY };
            let mut font_size = if node.text_scaled { node.rect.height().max(1.0) } else { node.text_size };
            let mut galley = painter.layout(node.text.clone(), FontId::proportional(font_size), node.text_color, wrap_width);
            if node.text_scaled && (galley.size().x > node.rect.width() || galley.size().y > node.rect.height()) {
                let scale = (node.rect.width() / galley.size().x.max(1.0))
                    .min(node.rect.height() / galley.size().y.max(1.0));
                font_size = (font_size * scale).max(1.0);
                galley = painter.layout(node.text.clone(), FontId::proportional(font_size), node.text_color, wrap_width);
            }
            let x = match node.text_x_alignment {
                0 => node.rect.left(),
                2 => node.rect.right() - galley.size().x,
                _ => node.rect.center().x - galley.size().x * 0.5,
            };
            let y = match node.text_y_alignment {
                0 => node.rect.top(),
                2 => node.rect.bottom() - galley.size().y,
                _ => node.rect.center().y - galley.size().y * 0.5,
            };
            let pos = Pos2::new(x, y);
            if node.text_stroke.a() > 0 {
                for offset in [Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0), Vec2::new(0.0, 1.0)] {
                    painter.galley(pos + offset, galley.clone(), node.text_stroke);
                }
            }
            painter.galley(pos, galley, node.text_color);
        }
        if selected == Some(node.referent) {
            painter.rect_stroke(node.rect, 0.0, Stroke::new(2.0, Color32::from_rgb(0, 162, 255)), egui::StrokeKind::Outside);
            for corner in [node.rect.left_top(), node.rect.right_top(), node.rect.left_bottom(), node.rect.right_bottom()] {
                painter.rect_filled(Rect::from_center_size(corner, Vec2::splat(6.0)), 0.0, Color32::WHITE);
                painter.rect_stroke(Rect::from_center_size(corner, Vec2::splat(6.0)), 0.0,
                    Stroke::new(1.0, Color32::from_rgb(0, 110, 220)), egui::StrokeKind::Inside);
            }
        }
        if response.clicked() { clicked = Some(node.referent); }
    }
    clicked
}
