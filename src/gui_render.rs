//! Roblox GUI preview rendered through egui over the Bevy viewport.
//! This first stage covers StarterGui/ScreenGui layout, frames, text and input
//! selection. Image and 3D BillboardGui rendering build on the same tree.

use bevy_egui::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use rbx_dom_weak::{types::{Ref, Variant}, WeakDom};

struct GuiNode {
    referent: Ref,
    class: String,
    rect: Rect,
    content_rect: Rect,
    clip: Rect,
    display_order: i32,
    z: i32,
    global_z: bool,
    sort_path: Vec<(i32, usize)>,
    order: usize,
    background: Color32,
    border: Color32,
    border_size: f32,
    corner_radius: f32,
    ui_stroke: Option<(f32, Color32)>,
    text: String,
    text_color: Color32,
    text_size: f32,
    text_x_alignment: i32,
    text_y_alignment: i32,
    text_wrapped: bool,
    text_scaled: bool,
    text_min_size: f32,
    text_max_size: f32,
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

fn vector2(value: Option<&Variant>, fallback: Vec2) -> Vec2 {
    match value {
        Some(Variant::Vector2(value)) => Vec2::new(value.x, value.y),
        _ => fallback,
    }
}

/// Resolve Roblox's absolute GuiObject rectangle, including inherited UIScale,
/// UISizeConstraint and UIAspectRatioConstraint. Constraints are children of
/// the object they affect, unlike CSS constraints.
fn is_gui_object(class: &str) -> bool {
    matches!(class, "Frame" | "CanvasGroup" | "TextLabel" | "TextButton" |
        "TextBox" | "ImageLabel" | "ImageButton" | "ScrollingFrame" |
        "ViewportFrame" | "VideoFrame")
}

fn gui_rect(
    dom: &WeakDom,
    painter: &egui::Painter,
    instance: &rbx_dom_weak::Instance,
    parent: Rect,
    scale: f32,
) -> Rect {
    // GuiObject.SizeConstraint controls which parent axis resolves each scale
    // component: RelativeXY=0, RelativeXX=1, RelativeYY=2.
    let size_constraint = enum_value(instance.properties.get(&rbx_dom_weak::ustr("SizeConstraint")), 0);
    let (x_extent, y_extent) = match size_constraint {
        1 => (parent.width(), parent.width()),
        2 => (parent.height(), parent.height()),
        _ => (parent.width(), parent.height()),
    };
    let position = match instance.properties.get(&rbx_dom_weak::ustr("Position")) {
        Some(Variant::UDim2(v)) => Vec2::new(
            parent.width() * v.x.scale + v.x.offset as f32 * scale,
            parent.height() * v.y.scale + v.y.offset as f32 * scale),
        _ => Vec2::ZERO,
    };
    let mut size = match instance.properties.get(&rbx_dom_weak::ustr("Size")) {
        Some(Variant::UDim2(v)) => Vec2::new(
            x_extent * v.x.scale + v.x.offset as f32 * scale,
            y_extent * v.y.scale + v.y.offset as f32 * scale),
        _ => Vec2::new(100.0 * scale, 100.0 * scale),
    }.max(Vec2::ZERO);

    // AutomaticSize uses authored Size as a minimum. Text is measured with the
    // same egui font engine used for painting so layout and display agree.
    let automatic = enum_value(instance.properties.get(&rbx_dom_weak::ustr("AutomaticSize")), 0);
    if automatic != 0 && matches!(instance.class.as_str(), "TextLabel" | "TextButton" | "TextBox") {
        let text = match instance.properties.get(&rbx_dom_weak::ustr("Text")) {
            Some(Variant::String(text)) => text.clone(),
            _ => String::new(),
        };
        if !text.is_empty() {
            let font_size = number(instance.properties.get(&rbx_dom_weak::ustr("TextSize")), 14.0).max(1.0) * scale;
            let wrapped = bool_value(instance.properties.get(&rbx_dom_weak::ustr("TextWrapped")), false);
            let wrap_width = if wrapped && size.x > 0.0 { size.x } else { f32::INFINITY };
            let galley = painter.layout(text, FontId::proportional(font_size), Color32::WHITE, wrap_width);
            let provisional = Rect::from_min_size(Pos2::ZERO, size);
            let content = padded_rect(dom, instance, provisional, scale);
            let padding = size - content.size();
            if automatic == 1 || automatic == 3 { size.x = size.x.max(galley.size().x + padding.x); }
            if automatic == 2 || automatic == 3 { size.y = size.y.max(galley.size().y + padding.y); }
        }
    }

    // Containers automatically expand to the absolute bounds of their direct
    // GuiObject children. Child gui_rect calls recursively resolve nested
    // AutomaticSize, so the result propagates from leaves toward the root.
    if automatic != 0 {
        let provisional_outer = Rect::from_min_size(Pos2::ZERO, size);
        let provisional_content = padded_rect(dom, instance, provisional_outer, scale);
        let mut required = Vec2::ZERO;
        for child_ref in instance.children() {
            let Some(child) = dom.get_by_ref(*child_ref) else { continue; };
            if !is_gui_object(&child.class) || !visible(child) { continue; }
            let child_scale = child_of_class(dom, child, "UIScale")
                .map(|modifier| number(modifier.properties.get(&rbx_dom_weak::ustr("Scale")), 1.0).max(0.0))
                .unwrap_or(1.0);
            let child_rect = gui_rect(dom, painter, child, provisional_content, scale * child_scale);
            required.x = required.x.max((child_rect.right() - provisional_content.left()).max(0.0));
            required.y = required.y.max((child_rect.bottom() - provisional_content.top()).max(0.0));
        }
        let padding = size - provisional_content.size();
        if automatic == 1 || automatic == 3 { size.x = size.x.max(required.x + padding.x); }
        if automatic == 2 || automatic == 3 { size.y = size.y.max(required.y + padding.y); }
    }

    let size_limits = child_of_class(dom, instance, "UISizeConstraint").map(|constraint| {
        let min = vector2(constraint.properties.get(&rbx_dom_weak::ustr("MinSize")), Vec2::ZERO) * scale;
        let max = vector2(constraint.properties.get(&rbx_dom_weak::ustr("MaxSize")), Vec2::splat(10_000.0)) * scale;
        (min.min(max), min.max(max))
    });
    if let Some((min, max)) = size_limits {
        size = size.clamp(min, max);
    }
    let mut aspect_ratio = None;
    if let Some(constraint) = child_of_class(dom, instance, "UIAspectRatioConstraint") {
        let ratio = number(constraint.properties.get(&rbx_dom_weak::ustr("AspectRatio")), 1.0).max(0.0001);
        aspect_ratio = Some(ratio);
        // DominantAxis: Width=0, Height=1. AspectType=ScaleWithParentSize
        // uses the parent's dominant dimension before applying the ratio.
        let dominant = enum_value(constraint.properties.get(&rbx_dom_weak::ustr("DominantAxis")), 0);
        let aspect_type = enum_value(constraint.properties.get(&rbx_dom_weak::ustr("AspectType")), 0);
        if aspect_type == 1 {
            if dominant == 1 { size = Vec2::new(parent.height() * ratio, parent.height()); }
            else { size = Vec2::new(parent.width(), parent.width() / ratio); }
        } else if size.x / size.y.max(0.0001) > ratio {
            size.x = size.y * ratio;
        } else {
            size.y = size.x / ratio;
        }
    }
    // Re-apply size bounds after aspect correction while preserving the ratio.
    // This avoids the common incorrect result where aspect expansion escapes
    // MaxSize or aspect shrinking falls below MinSize.
    if let Some((min, max)) = size_limits {
        if let Some(ratio) = aspect_ratio {
            let down = (max.x / size.x.max(0.0001)).min(max.y / size.y.max(0.0001)).min(1.0);
            size *= down;
            let up = (min.x / size.x.max(0.0001)).max(min.y / size.y.max(0.0001)).max(1.0);
            size *= up;
            // Impossible constraints favor fitting inside MaxSize, matching the
            // visual safety expected by Roblox containers.
            if size.x > max.x || size.y > max.y {
                if max.x / max.y.max(0.0001) > ratio { size = Vec2::new(max.y * ratio, max.y); }
                else { size = Vec2::new(max.x, max.x / ratio); }
            }
        } else {
            size = size.clamp(min, max);
        }
    }
    let anchor = vector2(instance.properties.get(&rbx_dom_weak::ustr("AnchorPoint")), Vec2::ZERO);
    let min = parent.min + position - size * anchor;
    Rect::from_min_size(min, size)
}

fn udim_pixels(value: Option<&Variant>, extent: f32, scale: f32) -> f32 {
    match value {
        Some(Variant::UDim(value)) => extent * value.scale + value.offset as f32 * scale,
        _ => 0.0,
    }
}

fn child_of_class<'a>(dom: &'a WeakDom, instance: &rbx_dom_weak::Instance, class: &str)
    -> Option<&'a rbx_dom_weak::Instance>
{
    instance.children().iter().find_map(|child| {
        dom.get_by_ref(*child).filter(|candidate| candidate.class == class)
    })
}

fn padded_rect(dom: &WeakDom, instance: &rbx_dom_weak::Instance, rect: Rect, scale: f32) -> Rect {
    let Some(padding) = child_of_class(dom, instance, "UIPadding") else { return rect; };
    let left = udim_pixels(padding.properties.get(&rbx_dom_weak::ustr("PaddingLeft")), rect.width(), scale);
    let right = udim_pixels(padding.properties.get(&rbx_dom_weak::ustr("PaddingRight")), rect.width(), scale);
    let top = udim_pixels(padding.properties.get(&rbx_dom_weak::ustr("PaddingTop")), rect.height(), scale);
    let bottom = udim_pixels(padding.properties.get(&rbx_dom_weak::ustr("PaddingBottom")), rect.height(), scale);
    Rect::from_min_max(
        Pos2::new(rect.left() + left, rect.top() + top),
        Pos2::new((rect.right() - right).max(rect.left() + left),
                  (rect.bottom() - bottom).max(rect.top() + top)),
    )
}

fn collect(dom: &WeakDom, painter: &egui::Painter, referent: Ref,
           parent_rect: Rect, parent_clip: Rect, display_order: i32,
           global_z: bool, inherited_scale: f32, parent_path: &[(i32, usize)],
           sequence: &mut usize, nodes: &mut Vec<GuiNode>) {
    let Some(instance) = dom.get_by_ref(referent) else { return; };
    if !visible(instance) { return; }
    let local_scale = child_of_class(dom, instance, "UIScale")
        .map(|modifier| number(modifier.properties.get(&rbx_dom_weak::ustr("Scale")), 1.0).max(0.0))
        .unwrap_or(1.0);
    let scale = inherited_scale * local_scale;
    let is_screen = instance.class == "ScreenGui";
    let rect = if is_screen { parent_rect } else { gui_rect(dom, painter, instance, parent_rect, scale) };
    let clips = matches!(instance.properties.get(&rbx_dom_weak::ustr("ClipsDescendants")), Some(Variant::Bool(true)));
    let child_clip = if clips { parent_clip.intersect(rect) } else { parent_clip };
    let z = match instance.properties.get(&rbx_dom_weak::ustr("ZIndex")) {
        Some(Variant::Int32(v)) => *v,
        Some(Variant::Int64(v)) => *v as i32,
        _ => 1,
    };
    let current_order = *sequence;
    let mut sort_path = parent_path.to_vec();
    sort_path.push((z, current_order));
    let content_rect = padded_rect(dom, instance, rect, scale);

    if is_gui_object(&instance.class) {
        let transparency = number(instance.properties.get(&rbx_dom_weak::ustr("BackgroundTransparency")), 0.0).clamp(0.0, 1.0);
        let alpha = ((1.0 - transparency) * 255.0).round() as u8;
        let text_transparency = number(instance.properties.get(&rbx_dom_weak::ustr("TextTransparency")), 0.0).clamp(0.0, 1.0);
        let corner_radius = child_of_class(dom, instance, "UICorner")
            .map(|corner| udim_pixels(corner.properties.get(&rbx_dom_weak::ustr("CornerRadius")), rect.width().min(rect.height()), scale))
            .unwrap_or(0.0)
            .clamp(0.0, rect.width().min(rect.height()) * 0.5);
        let ui_stroke = child_of_class(dom, instance, "UIStroke").and_then(|stroke| {
            let thickness = number(stroke.properties.get(&rbx_dom_weak::ustr("Thickness")), 1.0).max(0.0) * scale;
            let transparency = number(stroke.properties.get(&rbx_dom_weak::ustr("Transparency")), 0.0).clamp(0.0, 1.0);
            (thickness > 0.0 && transparency < 1.0).then(|| (thickness, color(
                stroke.properties.get(&rbx_dom_weak::ustr("Color")),
                ((1.0 - transparency) * 255.0) as u8, [0, 0, 0],
            )))
        });
        let (text_min_size, text_max_size) = child_of_class(dom, instance, "UITextSizeConstraint")
            .map(|constraint| (
                number(constraint.properties.get(&rbx_dom_weak::ustr("MinTextSize")), 1.0).max(1.0) * scale,
                number(constraint.properties.get(&rbx_dom_weak::ustr("MaxTextSize")), 100.0).max(1.0) * scale,
            ))
            .map(|(min, max)| (min.min(max), max.max(min)))
            .unwrap_or((1.0, 400.0));
        let order = *sequence;
        *sequence += 1;
        nodes.push(GuiNode {
            referent,
            class: instance.class.to_string(),
            rect,
            content_rect,
            clip: parent_clip.intersect(rect),
            display_order,
            z,
            global_z,
            sort_path: sort_path.clone(),
            order,
            background: color(instance.properties.get(&rbx_dom_weak::ustr("BackgroundColor3")), alpha, [255,255,255]),
            border: color(instance.properties.get(&rbx_dom_weak::ustr("BorderColor3")), 255, [27,42,53]),
            border_size: number(instance.properties.get(&rbx_dom_weak::ustr("BorderSizePixel")), 1.0).max(0.0) * scale,
            corner_radius,
            ui_stroke,
            text: match instance.properties.get(&rbx_dom_weak::ustr("Text")) { Some(Variant::String(v)) => v.clone(), _ => String::new() },
            text_color: color(instance.properties.get(&rbx_dom_weak::ustr("TextColor3")), ((1.0-text_transparency)*255.0) as u8, [0,0,0]),
            text_size: (number(instance.properties.get(&rbx_dom_weak::ustr("TextSize")), 14.0) * scale).clamp(1.0, 400.0),
            text_x_alignment: enum_value(instance.properties.get(&rbx_dom_weak::ustr("TextXAlignment")), 1),
            text_y_alignment: enum_value(instance.properties.get(&rbx_dom_weak::ustr("TextYAlignment")), 1),
            text_wrapped: bool_value(instance.properties.get(&rbx_dom_weak::ustr("TextWrapped")), false),
            text_scaled: bool_value(instance.properties.get(&rbx_dom_weak::ustr("TextScaled")), false),
            text_min_size,
            text_max_size,
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
    let child_parent_rect = if instance.class == "ScrollingFrame" {
        let canvas_position = vector2(
            instance.properties.get(&rbx_dom_weak::ustr("CanvasPosition")),
            Vec2::ZERO,
        ) * scale;
        content_rect.translate(-canvas_position)
    } else {
        content_rect
    };
    for child in instance.children() {
        collect(dom, painter, *child, child_parent_rect, child_clip, display_order,
            global_z, scale, &sort_path, sequence, nodes);
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
    let layout_painter = ui.painter().clone();
    if let Some(starter) = dom.get_by_ref(starter) {
        for (screen_order, child) in starter.children().iter().enumerate() {
            let Some(gui) = dom.get_by_ref(*child) else { continue; };
            if gui.class != "ScreenGui" || matches!(gui.properties.get(&rbx_dom_weak::ustr("Enabled")), Some(Variant::Bool(false))) { continue; }
            let display_order = enum_value(gui.properties.get(&rbx_dom_weak::ustr("DisplayOrder")), 0);
            let global_z = enum_value(gui.properties.get(&rbx_dom_weak::ustr("ZIndexBehavior")), 1) == 0;
            let root_path = vec![(0, screen_order)];
            collect(dom, &layout_painter, *child, viewport, viewport, display_order,
                global_z, 1.0, &root_path, &mut sequence, &mut nodes);
        }
    }
    nodes.sort_by(|left, right| {
        left.display_order.cmp(&right.display_order).then_with(|| {
            if left.global_z && right.global_z {
                left.z.cmp(&right.z).then(left.order.cmp(&right.order))
            } else {
                left.sort_path.cmp(&right.sort_path).then(left.order.cmp(&right.order))
            }
        })
    });
    let mut clicked = None;
    for node in nodes {
        if node.clip.width() <= 0.0 || node.clip.height() <= 0.0 { continue; }
        let response = ui.interact(node.clip, ui.make_persistent_id(("roblox_gui", format!("{:?}", node.referent))), egui::Sense::click());
        let painter = ui.painter().with_clip_rect(node.clip);
        painter.rect_filled(node.rect, node.corner_radius, node.background);
        if node.border_size > 0.0 {
            painter.rect_stroke(node.rect, node.corner_radius, Stroke::new(node.border_size, node.border), egui::StrokeKind::Inside);
        }
        if let Some((thickness, color)) = node.ui_stroke {
            painter.rect_stroke(node.rect, node.corner_radius, Stroke::new(thickness, color), egui::StrokeKind::Middle);
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
                painter.image(texture.id(), node.content_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), node.image_color);
            }
        }
        if !node.text.is_empty() {
            let text_rect = node.content_rect;
            let wrap_width = if node.text_wrapped { text_rect.width() } else { f32::INFINITY };
            let mut font_size = if node.text_scaled { text_rect.height().max(1.0) } else { node.text_size };
            font_size = font_size.clamp(node.text_min_size, node.text_max_size);
            let mut galley = painter.layout(node.text.clone(), FontId::proportional(font_size), node.text_color, wrap_width);
            if node.text_scaled && (galley.size().x > text_rect.width() || galley.size().y > text_rect.height()) {
                let scale = (text_rect.width() / galley.size().x.max(1.0))
                    .min(text_rect.height() / galley.size().y.max(1.0));
                font_size = (font_size * scale).clamp(node.text_min_size, node.text_max_size);
                galley = painter.layout(node.text.clone(), FontId::proportional(font_size), node.text_color, wrap_width);
            }
            let x = match node.text_x_alignment {
                0 => text_rect.left(),
                2 => text_rect.right() - galley.size().x,
                _ => text_rect.center().x - galley.size().x * 0.5,
            };
            let y = match node.text_y_alignment {
                0 => text_rect.top(),
                2 => text_rect.bottom() - galley.size().y,
                _ => text_rect.center().y - galley.size().y * 0.5,
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
