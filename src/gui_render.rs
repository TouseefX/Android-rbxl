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
    gradient: Option<(f32, Vec2, Vec<Color32>)>,
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
    image_rect_offset: Vec2,
    image_rect_size: Vec2,
    image_scale_type: i32,
    tile_size: Option<(f32, f32, f32, f32)>,
    slice_center: Option<[f32; 4]>,
    slice_scale: f32,
    canvas_size: Vec2,
    canvas_position: Vec2,
    scroll_bar_thickness: f32,
    scroll_bar_color: Color32,
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

fn udim2_tuple(value: Option<&Variant>) -> Option<(f32, f32, f32, f32)> {
    match value {
        Some(Variant::UDim2(value)) => Some((
            value.x.scale, value.x.offset as f32,
            value.y.scale, value.y.offset as f32,
        )),
        _ => None,
    }
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
        if let Some(layout) = child_of_class(dom, instance, "UITableLayout") {
            let row_major = enum_value(layout.properties.get(&rbx_dom_weak::ustr("MajorAxis")), 0) == 0;
            let (pxs, pxo, pys, pyo) = udim2_tuple(layout.properties.get(&rbx_dom_weak::ustr("Padding")))
                .unwrap_or((0.0, 0.0, 0.0, 0.0));
            let gap = Vec2::new(provisional_content.width()*pxs+pxo*scale, provisional_content.height()*pys+pyo*scale).max(Vec2::ZERO);
            let groups: Vec<&rbx_dom_weak::Instance> = instance.children().iter().filter_map(|referent| dom.get_by_ref(*referent)
                .filter(|child| is_gui_object(&child.class) && visible(child))).collect();
            let minor_count = groups.iter().map(|group| group.children().iter().filter(|referent| dom.get_by_ref(**referent)
                .map(|cell| is_gui_object(&cell.class) && visible(cell)).unwrap_or(false)).count()).max().unwrap_or(0);
            let (rows, columns) = if row_major { (groups.len(), minor_count) } else { (minor_count, groups.len()) };
            let mut widths = vec![0.0f32; columns];
            let mut heights = vec![0.0f32; rows];
            for (major, group) in groups.iter().enumerate() {
                for (minor, cell) in group.children().iter().filter_map(|referent| dom.get_by_ref(*referent)
                    .filter(|cell| is_gui_object(&cell.class) && visible(cell))).enumerate() {
                    let (row, column) = if row_major { (major, minor) } else { (minor, major) };
                    let rect = gui_rect(dom, painter, cell, provisional_content, scale);
                    widths[column] = widths[column].max(rect.width());
                    heights[row] = heights[row].max(rect.height());
                }
            }
            required = Vec2::new(widths.iter().sum::<f32>() + gap.x*columns.saturating_sub(1) as f32,
                heights.iter().sum::<f32>() + gap.y*rows.saturating_sub(1) as f32);
        } else if let Some(layout) = child_of_class(dom, instance, "UIPageLayout") {
            let requested = match layout.properties.get(&rbx_dom_weak::ustr("CurrentPage")) {
                Some(Variant::Ref(referent)) => Some(*referent),
                _ => None,
            };
            let active = requested.and_then(|referent| dom.get_by_ref(referent)
                .filter(|child| is_gui_object(&child.class) && visible(child)))
                .or_else(|| instance.children().iter().find_map(|referent| dom.get_by_ref(*referent)
                    .filter(|child| is_gui_object(&child.class) && visible(child))));
            required = active.map(|child| {
                let child_scale = child_of_class(dom, child, "UIScale")
                    .map(|modifier| number(modifier.properties.get(&rbx_dom_weak::ustr("Scale")), 1.0).max(0.0))
                    .unwrap_or(1.0);
                gui_rect(dom, painter, child, provisional_content, scale * child_scale).size()
            }).unwrap_or(Vec2::ZERO);
        } else if let Some(layout) = child_of_class(dom, instance, "UIGridLayout") {
            let (cxs, cxo, cys, cyo) = udim2_tuple(layout.properties.get(&rbx_dom_weak::ustr("CellSize")))
                .unwrap_or((0.0, 100.0, 0.0, 100.0));
            let (pxs, pxo, pys, pyo) = udim2_tuple(layout.properties.get(&rbx_dom_weak::ustr("CellPadding")))
                .unwrap_or((0.0, 5.0, 0.0, 5.0));
            let cell = Vec2::new(provisional_content.width()*cxs+cxo*scale, provisional_content.height()*cys+cyo*scale).max(Vec2::ZERO);
            let gap = Vec2::new(provisional_content.width()*pxs+pxo*scale, provisional_content.height()*pys+pyo*scale).max(Vec2::ZERO);
            let count = instance.children().iter().filter(|child| dom.get_by_ref(**child)
                .map(|child| is_gui_object(&child.class) && visible(child)).unwrap_or(false)).count();
            let horizontal = enum_value(layout.properties.get(&rbx_dom_weak::ustr("FillDirection")), 0) == 0;
            let maximum = number(layout.properties.get(&rbx_dom_weak::ustr("FillDirectionMaxCells")), 0.0).max(0.0) as usize;
            let fit_columns = ((provisional_content.width()+gap.x)/(cell.x+gap.x).max(1.0)).floor().max(1.0) as usize;
            let fit_rows = ((provisional_content.height()+gap.y)/(cell.y+gap.y).max(1.0)).floor().max(1.0) as usize;
            let vertical_capacity = if maximum > 0 { maximum } else { fit_rows }.max(1);
            let columns = if horizontal { if maximum > 0 { maximum } else { fit_columns } }
                else { ((count + vertical_capacity - 1) / vertical_capacity).max(1) };
            let rows = if horizontal { ((count + columns - 1) / columns).max(1) } else { vertical_capacity };
            required = Vec2::new(
                cell.x*columns.min(count.max(1)) as f32 + gap.x*columns.min(count.max(1)).saturating_sub(1) as f32,
                cell.y*rows.min(count.max(1)) as f32 + gap.y*rows.min(count.max(1)).saturating_sub(1) as f32,
            );
        } else if let Some(layout) = child_of_class(dom, instance, "UIListLayout") {
            let horizontal = enum_value(layout.properties.get(&rbx_dom_weak::ustr("FillDirection")), 1) == 0;
            let gap = udim_pixels(layout.properties.get(&rbx_dom_weak::ustr("Padding")),
                if horizontal { provisional_content.width() } else { provisional_content.height() }, scale);
            let mut count = 0usize;
            let mut main = 0.0f32;
            let mut cross = 0.0f32;
            for child_ref in instance.children() {
                let Some(child) = dom.get_by_ref(*child_ref) else { continue; };
                if !is_gui_object(&child.class) || !visible(child) { continue; }
                let child_scale = child_of_class(dom, child, "UIScale")
                    .map(|modifier| number(modifier.properties.get(&rbx_dom_weak::ustr("Scale")), 1.0).max(0.0))
                    .unwrap_or(1.0);
                let rect = gui_rect(dom, painter, child, provisional_content, scale * child_scale);
                main += if horizontal { rect.width() } else { rect.height() };
                cross = cross.max(if horizontal { rect.height() } else { rect.width() });
                count += 1;
            }
            main += gap * count.saturating_sub(1) as f32;
            required = if horizontal { Vec2::new(main, cross) } else { Vec2::new(cross, main) };
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

fn multiply_color(a: Color32, b: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (a.r() as u16 * b.r() as u16 / 255) as u8,
        (a.g() as u16 * b.g() as u16 / 255) as u8,
        (a.b() as u16 * b.b() as u16 / 255) as u8,
        (a.a() as u16 * b.a() as u16 / 255) as u8,
    )
}

fn gradient_samples(instance: &rbx_dom_weak::Instance, base: Color32) -> Option<(f32, Vec2, Vec<Color32>)> {
    let rotation = number(instance.properties.get(&rbx_dom_weak::ustr("Rotation")), 0.0);
    let offset = vector2(instance.properties.get(&rbx_dom_weak::ustr("Offset")), Vec2::ZERO);
    let colors = match instance.properties.get(&rbx_dom_weak::ustr("Color")) {
        Some(Variant::ColorSequence(sequence)) => &sequence.keypoints,
        _ => return None,
    };
    if colors.is_empty() { return None; }
    let transparencies = match instance.properties.get(&rbx_dom_weak::ustr("Transparency")) {
        Some(Variant::NumberSequence(sequence)) => Some(&sequence.keypoints),
        _ => None,
    };
    let sample_color = |time: f32| {
        let upper = colors.iter().position(|point| point.time >= time).unwrap_or(colors.len().saturating_sub(1));
        let lower = upper.saturating_sub(1);
        let a = &colors[lower];
        let b = &colors[upper];
        let mix = if b.time > a.time { (time - a.time) / (b.time - a.time) } else { 0.0 };
        let rgb = Color32::from_rgb(
            ((a.value.r + (b.value.r - a.value.r) * mix) * 255.0) as u8,
            ((a.value.g + (b.value.g - a.value.g) * mix) * 255.0) as u8,
            ((a.value.b + (b.value.b - a.value.b) * mix) * 255.0) as u8,
        );
        let transparency = transparencies.and_then(|points| {
            if points.is_empty() { return None; }
            let upper = points.iter().position(|point| point.time >= time).unwrap_or(points.len() - 1);
            let lower = upper.saturating_sub(1);
            let a = &points[lower];
            let b = &points[upper];
            let mix = if b.time > a.time { (time - a.time) / (b.time - a.time) } else { 0.0 };
            Some(a.value + (b.value - a.value) * mix)
        }).unwrap_or(0.0).clamp(0.0, 1.0);
        multiply_color(base, Color32::from_rgba_unmultiplied(rgb.r(), rgb.g(), rgb.b(), ((1.0-transparency)*255.0) as u8))
    };
    Some((rotation, offset, (0..=32).map(|index| sample_color(index as f32 / 32.0)).collect()))
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
           forced_rect: Option<Rect>, overrides: &mut std::collections::HashMap<Ref, Rect>,
           scroll_offsets: &std::collections::HashMap<Ref, Vec2>, sequence: &mut usize,
           nodes: &mut Vec<GuiNode>) {
    let Some(instance) = dom.get_by_ref(referent) else { return; };
    if !visible(instance) { return; }
    let local_scale = child_of_class(dom, instance, "UIScale")
        .map(|modifier| number(modifier.properties.get(&rbx_dom_weak::ustr("Scale")), 1.0).max(0.0))
        .unwrap_or(1.0);
    let scale = inherited_scale * local_scale;
    let is_screen = instance.class == "ScreenGui";
    let rect = forced_rect.or_else(|| overrides.get(&referent).copied()).unwrap_or_else(|| {
        if is_screen { parent_rect } else { gui_rect(dom, painter, instance, parent_rect, scale) }
    });
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
        let background = color(instance.properties.get(&rbx_dom_weak::ustr("BackgroundColor3")), alpha, [255,255,255]);
        let gradient = child_of_class(dom, instance, "UIGradient")
            .and_then(|modifier| bool_value(modifier.properties.get(&rbx_dom_weak::ustr("Enabled")), true)
                .then(|| gradient_samples(modifier, background)).flatten());
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
        let mut canvas_size = match instance.properties.get(&rbx_dom_weak::ustr("CanvasSize")) {
            Some(Variant::UDim2(value)) if instance.class == "ScrollingFrame" => Vec2::new(
                rect.width() * value.x.scale + value.x.offset as f32 * scale,
                rect.height() * value.y.scale + value.y.offset as f32 * scale,
            ).max(rect.size()),
            _ => rect.size(),
        };
        let automatic_canvas = enum_value(instance.properties.get(&rbx_dom_weak::ustr("AutomaticCanvasSize")), 0);
        if instance.class == "ScrollingFrame" && automatic_canvas != 0 {
            let mut required = Vec2::ZERO;
            for child_ref in instance.children() {
                let Some(child) = dom.get_by_ref(*child_ref) else { continue; };
                if !is_gui_object(&child.class) || !visible(child) { continue; }
                let child_rect = gui_rect(dom, painter, child, content_rect, scale);
                required.x = required.x.max((child_rect.right()-content_rect.left()).max(0.0));
                required.y = required.y.max((child_rect.bottom()-content_rect.top()).max(0.0));
            }
            if automatic_canvas == 1 || automatic_canvas == 3 { canvas_size.x = canvas_size.x.max(required.x); }
            if automatic_canvas == 2 || automatic_canvas == 3 { canvas_size.y = canvas_size.y.max(required.y); }
        }
        let scroll_bar_transparency = number(instance.properties.get(&rbx_dom_weak::ustr("ScrollBarImageTransparency")), 0.0).clamp(0.0, 1.0);
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
            background,
            border: color(instance.properties.get(&rbx_dom_weak::ustr("BorderColor3")), 255, [27,42,53]),
            border_size: number(instance.properties.get(&rbx_dom_weak::ustr("BorderSizePixel")), 1.0).max(0.0) * scale,
            corner_radius,
            ui_stroke,
            gradient,
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
            image_rect_offset: vector2(instance.properties.get(&rbx_dom_weak::ustr("ImageRectOffset")), Vec2::ZERO),
            image_rect_size: vector2(instance.properties.get(&rbx_dom_weak::ustr("ImageRectSize")), Vec2::ZERO),
            image_scale_type: enum_value(instance.properties.get(&rbx_dom_weak::ustr("ScaleType")), 0),
            tile_size: udim2_tuple(instance.properties.get(&rbx_dom_weak::ustr("TileSize"))),
            slice_center: match instance.properties.get(&rbx_dom_weak::ustr("SliceCenter")) {
                Some(Variant::Rect(rect)) => Some([rect.min.x, rect.min.y, rect.max.x, rect.max.y]),
                _ => None,
            },
            slice_scale: number(instance.properties.get(&rbx_dom_weak::ustr("SliceScale")), 1.0).max(0.0),
            canvas_size,
            canvas_position: vector2(instance.properties.get(&rbx_dom_weak::ustr("CanvasPosition")), Vec2::ZERO) * scale,
            scroll_bar_thickness: number(instance.properties.get(&rbx_dom_weak::ustr("ScrollBarThickness")), 12.0).max(0.0) * scale,
            scroll_bar_color: color(instance.properties.get(&rbx_dom_weak::ustr("ScrollBarImageColor3")),
                ((1.0-scroll_bar_transparency)*255.0) as u8, [255,255,255]),
        });
    }
    let child_parent_rect = if instance.class == "ScrollingFrame" {
        let authored_position = vector2(
            instance.properties.get(&rbx_dom_weak::ustr("CanvasPosition")),
            Vec2::ZERO,
        ) * scale;
        let preview_position = scroll_offsets.get(&referent).copied().unwrap_or(Vec2::ZERO);
        content_rect.translate(-(authored_position + preview_position))
    } else {
        content_rect
    };
    if let Some(layout) = child_of_class(dom, instance, "UITableLayout") {
        let row_major = enum_value(layout.properties.get(&rbx_dom_weak::ustr("MajorAxis")), 0) == 0;
        let (pxs, pxo, pys, pyo) = udim2_tuple(layout.properties.get(&rbx_dom_weak::ustr("Padding")))
            .unwrap_or((0.0, 0.0, 0.0, 0.0));
        let gap = Vec2::new(child_parent_rect.width()*pxs + pxo*scale, child_parent_rect.height()*pys + pyo*scale).max(Vec2::ZERO);
        let fill_columns = bool_value(layout.properties.get(&rbx_dom_weak::ustr("FillEmptySpaceColumns")), false);
        let fill_rows = bool_value(layout.properties.get(&rbx_dom_weak::ustr("FillEmptySpaceRows")), false);
        let sort_order = enum_value(layout.properties.get(&rbx_dom_weak::ustr("SortOrder")), 0);
        let mut groups: Vec<Ref> = instance.children().iter().filter_map(|referent| {
            dom.get_by_ref(*referent).filter(|child| is_gui_object(&child.class) && visible(child)).map(|_| *referent)
        }).collect();
        groups.sort_by(|left, right| {
            let a = dom.get_by_ref(*left).unwrap(); let b = dom.get_by_ref(*right).unwrap();
            if sort_order == 1 {
                enum_value(a.properties.get(&rbx_dom_weak::ustr("LayoutOrder")), 0)
                    .cmp(&enum_value(b.properties.get(&rbx_dom_weak::ustr("LayoutOrder")), 0))
            } else { a.name.cmp(&b.name) }
        });
        let mut matrix: Vec<Vec<Ref>> = Vec::new();
        for group in &groups {
            let Some(group_instance) = dom.get_by_ref(*group) else { continue; };
            let mut cells: Vec<Ref> = group_instance.children().iter().filter_map(|referent| {
                dom.get_by_ref(*referent).filter(|child| is_gui_object(&child.class) && visible(child)).map(|_| *referent)
            }).collect();
            cells.sort_by(|left, right| {
                let a = dom.get_by_ref(*left).unwrap(); let b = dom.get_by_ref(*right).unwrap();
                if sort_order == 1 {
                    enum_value(a.properties.get(&rbx_dom_weak::ustr("LayoutOrder")), 0)
                        .cmp(&enum_value(b.properties.get(&rbx_dom_weak::ustr("LayoutOrder")), 0))
                } else { a.name.cmp(&b.name) }
            });
            matrix.push(cells);
        }
        let columns = if row_major { matrix.iter().map(Vec::len).max().unwrap_or(1) } else { groups.len() }.max(1);
        let rows = if row_major { groups.len() } else { matrix.iter().map(Vec::len).max().unwrap_or(1) }.max(1);
        let mut column_widths = vec![0.0f32; columns];
        let mut row_heights = vec![0.0f32; rows];
        for (major, cells) in matrix.iter().enumerate() {
            for (minor, cell_ref) in cells.iter().enumerate() {
                let (row, column) = if row_major { (major, minor) } else { (minor, major) };
                let Some(cell) = dom.get_by_ref(*cell_ref) else { continue; };
                let natural = gui_rect(dom, painter, cell, child_parent_rect, scale);
                column_widths[column] = column_widths[column].max(natural.width());
                row_heights[row] = row_heights[row].max(natural.height());
            }
        }
        if fill_columns {
            let width = ((child_parent_rect.width() - gap.x*(columns-1) as f32) / columns as f32).max(0.0);
            column_widths.fill(width);
        }
        if fill_rows {
            let height = ((child_parent_rect.height() - gap.y*(rows-1) as f32) / rows as f32).max(0.0);
            row_heights.fill(height);
        }
        let mut ys = vec![child_parent_rect.top(); rows];
        let mut xs = vec![child_parent_rect.left(); columns];
        for index in 1..columns { xs[index] = xs[index-1] + column_widths[index-1] + gap.x; }
        for index in 1..rows { ys[index] = ys[index-1] + row_heights[index-1] + gap.y; }
        for (major, group_ref) in groups.iter().enumerate() {
            let group_rect = if row_major {
                Rect::from_min_size(Pos2::new(child_parent_rect.left(), ys[major]), Vec2::new(column_widths.iter().sum::<f32>() + gap.x*(columns-1) as f32, row_heights[major]))
            } else {
                Rect::from_min_size(Pos2::new(xs[major], child_parent_rect.top()), Vec2::new(column_widths[major], row_heights.iter().sum::<f32>() + gap.y*(rows-1) as f32))
            };
            overrides.insert(*group_ref, group_rect);
            for (minor, cell_ref) in matrix.get(major).into_iter().flatten().enumerate() {
                let (row, column) = if row_major { (major, minor) } else { (minor, major) };
                overrides.insert(*cell_ref, Rect::from_min_size(Pos2::new(xs[column], ys[row]), Vec2::new(column_widths[column], row_heights[row])));
            }
        }
        for child in instance.children() {
            collect(dom, painter, *child, child_parent_rect, child_clip, display_order,
                global_z, scale, &sort_path, None, overrides, scroll_offsets, sequence, nodes);
        }
    } else if let Some(layout) = child_of_class(dom, instance, "UIPageLayout") {
        let sort_order = enum_value(layout.properties.get(&rbx_dom_weak::ustr("SortOrder")), 0);
        let mut pages: Vec<(usize, Ref, i32, String)> = instance.children().iter().enumerate()
            .filter_map(|(index, referent)| {
                let child = dom.get_by_ref(*referent)?;
                if !is_gui_object(&child.class) || !visible(child) { return None; }
                let order = match child.properties.get(&rbx_dom_weak::ustr("LayoutOrder")) {
                    Some(Variant::Int32(value)) => *value,
                    Some(Variant::Int64(value)) => *value as i32,
                    _ => 0,
                };
                Some((index, *referent, order, child.name.to_string()))
            }).collect();
        pages.sort_by(|left, right| {
            if sort_order == 1 { left.2.cmp(&right.2).then(left.0.cmp(&right.0)) }
            else { left.3.cmp(&right.3).then(left.0.cmp(&right.0)) }
        });
        let requested_page = match layout.properties.get(&rbx_dom_weak::ustr("CurrentPage")) {
            Some(Variant::Ref(referent)) => Some(*referent),
            _ => None,
        };
        let active = requested_page
            .and_then(|requested| pages.iter().find(|(_, referent, _, _)| *referent == requested))
            .or_else(|| pages.first());
        if let Some((_, child, _, _)) = active {
            let child_instance = dom.get_by_ref(*child).expect("page disappeared during layout");
            let child_scale = child_of_class(dom, child_instance, "UIScale")
                .map(|modifier| number(modifier.properties.get(&rbx_dom_weak::ustr("Scale")), 1.0).max(0.0))
                .unwrap_or(1.0);
            let natural = gui_rect(dom, painter, child_instance, child_parent_rect, scale * child_scale);
            let horizontal_alignment = enum_value(layout.properties.get(&rbx_dom_weak::ustr("HorizontalAlignment")), 1);
            let vertical_alignment = enum_value(layout.properties.get(&rbx_dom_weak::ustr("VerticalAlignment")), 1);
            let x = match horizontal_alignment {
                0 => child_parent_rect.left(),
                2 => child_parent_rect.right() - natural.width(),
                _ => child_parent_rect.center().x - natural.width() * 0.5,
            };
            let y = match vertical_alignment {
                0 => child_parent_rect.top(),
                2 => child_parent_rect.bottom() - natural.height(),
                _ => child_parent_rect.center().y - natural.height() * 0.5,
            };
            collect(dom, painter, *child, child_parent_rect, child_clip, display_order,
                global_z, scale, &sort_path, Some(Rect::from_min_size(Pos2::new(x, y), natural.size())), overrides, scroll_offsets, sequence, nodes);
        }
    } else if let Some(layout) = child_of_class(dom, instance, "UIGridLayout") {
        let (cxs, cxo, cys, cyo) = udim2_tuple(layout.properties.get(&rbx_dom_weak::ustr("CellSize")))
            .unwrap_or((0.0, 100.0, 0.0, 100.0));
        let (pxs, pxo, pys, pyo) = udim2_tuple(layout.properties.get(&rbx_dom_weak::ustr("CellPadding")))
            .unwrap_or((0.0, 5.0, 0.0, 5.0));
        let cell = Vec2::new(
            (child_parent_rect.width() * cxs + cxo * scale).max(0.0),
            (child_parent_rect.height() * cys + cyo * scale).max(0.0),
        );
        let gap = Vec2::new(
            (child_parent_rect.width() * pxs + pxo * scale).max(0.0),
            (child_parent_rect.height() * pys + pyo * scale).max(0.0),
        );
        let horizontal = enum_value(layout.properties.get(&rbx_dom_weak::ustr("FillDirection")), 0) == 0;
        let maximum = number(layout.properties.get(&rbx_dom_weak::ustr("FillDirectionMaxCells")), 0.0).max(0.0) as usize;
        let start_corner = enum_value(layout.properties.get(&rbx_dom_weak::ustr("StartCorner")), 0);
        let horizontal_alignment = enum_value(layout.properties.get(&rbx_dom_weak::ustr("HorizontalAlignment")), 0);
        let vertical_alignment = enum_value(layout.properties.get(&rbx_dom_weak::ustr("VerticalAlignment")), 0);
        let sort_order = enum_value(layout.properties.get(&rbx_dom_weak::ustr("SortOrder")), 0);
        let mut children: Vec<(usize, Ref, i32, String)> = instance.children().iter().enumerate()
            .filter_map(|(index, referent)| {
                let child = dom.get_by_ref(*referent)?;
                if !is_gui_object(&child.class) || !visible(child) { return None; }
                let order = match child.properties.get(&rbx_dom_weak::ustr("LayoutOrder")) {
                    Some(Variant::Int32(value)) => *value,
                    Some(Variant::Int64(value)) => *value as i32,
                    _ => 0,
                };
                Some((index, *referent, order, child.name.to_string()))
            }).collect();
        children.sort_by(|left, right| {
            if sort_order == 1 { left.2.cmp(&right.2).then(left.0.cmp(&right.0)) }
            else { left.3.cmp(&right.3).then(left.0.cmp(&right.0)) }
        });
        let fit_columns = ((child_parent_rect.width() + gap.x) / (cell.x + gap.x).max(1.0)).floor().max(1.0) as usize;
        let fit_rows = ((child_parent_rect.height() + gap.y) / (cell.y + gap.y).max(1.0)).floor().max(1.0) as usize;
        let count = children.len();
        let vertical_capacity = if maximum > 0 { maximum } else { fit_rows }.max(1);
        let columns = if horizontal { if maximum > 0 { maximum } else { fit_columns } }
            else { ((count + vertical_capacity - 1) / vertical_capacity).max(1) };
        let rows = if horizontal { ((count + columns - 1) / columns).max(1) }
            else { vertical_capacity };
        let effective_columns = columns.min(count.max(1));
        let effective_rows = rows.min(count.max(1));
        let occupied = Vec2::new(
            cell.x * effective_columns as f32 + gap.x * effective_columns.saturating_sub(1) as f32,
            cell.y * effective_rows as f32 + gap.y * effective_rows.saturating_sub(1) as f32,
        );
        let origin = child_parent_rect.min + Vec2::new(
            match horizontal_alignment { 1 => (child_parent_rect.width()-occupied.x)*0.5, 2 => child_parent_rect.width()-occupied.x, _ => 0.0 }.max(0.0),
            match vertical_alignment { 1 => (child_parent_rect.height()-occupied.y)*0.5, 2 => child_parent_rect.height()-occupied.y, _ => 0.0 }.max(0.0),
        );
        for (slot, (_, child, _, _)) in children.into_iter().enumerate() {
            let (mut column, mut row) = if horizontal { (slot % effective_columns, slot / effective_columns) }
                else { (slot / effective_rows, slot % effective_rows) };
            if start_corner == 1 || start_corner == 3 { column = effective_columns.saturating_sub(1).saturating_sub(column); }
            if start_corner == 2 || start_corner == 3 { row = effective_rows.saturating_sub(1).saturating_sub(row); }
            let min = origin + Vec2::new(column as f32 * (cell.x + gap.x), row as f32 * (cell.y + gap.y));
            collect(dom, painter, child, child_parent_rect, child_clip, display_order,
                global_z, scale, &sort_path, Some(Rect::from_min_size(min, cell)), overrides, scroll_offsets, sequence, nodes);
        }
    } else if let Some(layout) = child_of_class(dom, instance, "UIListLayout") {
        let horizontal = enum_value(layout.properties.get(&rbx_dom_weak::ustr("FillDirection")), 1) == 0;
        let horizontal_alignment = enum_value(layout.properties.get(&rbx_dom_weak::ustr("HorizontalAlignment")), 0);
        let vertical_alignment = enum_value(layout.properties.get(&rbx_dom_weak::ustr("VerticalAlignment")), 0);
        let padding = udim_pixels(
            layout.properties.get(&rbx_dom_weak::ustr("Padding")),
            if horizontal { child_parent_rect.width() } else { child_parent_rect.height() }, scale,
        );
        let sort_order = enum_value(layout.properties.get(&rbx_dom_weak::ustr("SortOrder")), 0);
        let mut children: Vec<(usize, Ref, i32, String, f32, Rect)> = instance.children().iter().enumerate()
            .filter_map(|(index, referent)| {
                let child = dom.get_by_ref(*referent)?;
                if !is_gui_object(&child.class) || !visible(child) { return None; }
                let child_scale = child_of_class(dom, child, "UIScale")
                    .map(|modifier| number(modifier.properties.get(&rbx_dom_weak::ustr("Scale")), 1.0).max(0.0))
                    .unwrap_or(1.0);
                let child_rect = gui_rect(dom, painter, child, child_parent_rect, scale * child_scale);
                let order = match child.properties.get(&rbx_dom_weak::ustr("LayoutOrder")) {
                    Some(Variant::Int32(value)) => *value,
                    Some(Variant::Int64(value)) => *value as i32,
                    _ => 0,
                };
                Some((index, *referent, order, child.name.to_string(), child_scale, child_rect))
            }).collect();
        children.sort_by(|left, right| {
            if sort_order == 1 { left.2.cmp(&right.2).then(left.0.cmp(&right.0)) }
            else { left.3.cmp(&right.3).then(left.0.cmp(&right.0)) }
        });
        let total = children.iter().map(|(_, _, _, _, _, rect)| if horizontal { rect.width() } else { rect.height() }).sum::<f32>()
            + padding * children.len().saturating_sub(1) as f32;
        let available = if horizontal { child_parent_rect.width() } else { child_parent_rect.height() };
        let main_alignment = if horizontal { horizontal_alignment } else { vertical_alignment };
        let mut cursor = match main_alignment {
            1 => (available - total) * 0.5,
            2 => available - total,
            _ => 0.0,
        }.max(0.0);
        for (_, child, _, _, _child_scale, natural) in children {
            let cross_available = if horizontal { child_parent_rect.height() } else { child_parent_rect.width() };
            let cross_size = if horizontal { natural.height() } else { natural.width() };
            let cross_alignment = if horizontal { vertical_alignment } else { horizontal_alignment };
            let cross = match cross_alignment {
                1 => (cross_available - cross_size) * 0.5,
                2 => cross_available - cross_size,
                _ => 0.0,
            }.max(0.0);
            let min = if horizontal {
                child_parent_rect.min + Vec2::new(cursor, cross)
            } else {
                child_parent_rect.min + Vec2::new(cross, cursor)
            };
            let arranged = Rect::from_min_size(min, natural.size());
            collect(dom, painter, child, child_parent_rect, child_clip, display_order,
                global_z, scale, &sort_path, Some(arranged), overrides, scroll_offsets, sequence, nodes);
            cursor += if horizontal { natural.width() } else { natural.height() } + padding;
        }
    } else {
        for child in instance.children() {
            collect(dom, painter, *child, child_parent_rect, child_clip, display_order,
                global_z, scale, &sort_path, None, overrides, scroll_offsets, sequence, nodes);
        }
    }
}

fn paint_gradient(painter: &egui::Painter, rect: Rect, gradient: &(f32, Vec2, Vec<Color32>)) {
    let (rotation, offset, samples) = gradient;
    if samples.len() < 2 { return; }
    let radians = rotation.to_radians();
    let direction = Vec2::new(radians.cos(), radians.sin());
    let perpendicular = Vec2::new(-direction.y, direction.x);
    let along = direction.x.abs() * rect.width() * 0.5 + direction.y.abs() * rect.height() * 0.5;
    let across = perpendicular.x.abs() * rect.width() * 0.5 + perpendicular.y.abs() * rect.height() * 0.5 + 2.0;
    let center = rect.center() + Vec2::new(offset.x * rect.width(), offset.y * rect.height());
    for index in 0..samples.len() - 1 {
        let t0 = index as f32 / (samples.len() - 1) as f32;
        let t1 = (index + 1) as f32 / (samples.len() - 1) as f32;
        let a = -along + along * 2.0 * t0;
        let b = -along + along * 2.0 * t1;
        let points = vec![
            center + direction * a - perpendicular * across,
            center + direction * b - perpendicular * across,
            center + direction * b + perpendicular * across,
            center + direction * a + perpendicular * across,
        ];
        painter.add(egui::Shape::convex_polygon(points, samples[index], Stroke::NONE));
    }
}

fn paint_image(painter: &egui::Painter, node: &GuiNode, texture: &egui::TextureHandle) {
    let texture_size = texture.size_vec2().max(Vec2::splat(1.0));
    let source_size = if node.image_rect_size.x > 0.0 && node.image_rect_size.y > 0.0 {
        node.image_rect_size
    } else {
        texture_size
    };
    let uv_min = Pos2::new(node.image_rect_offset.x / texture_size.x, node.image_rect_offset.y / texture_size.y);
    let uv_max = Pos2::new(
        (node.image_rect_offset.x + source_size.x) / texture_size.x,
        (node.image_rect_offset.y + source_size.y) / texture_size.y,
    );
    let uv = Rect::from_min_max(uv_min, uv_max);
    let bounds = node.content_rect;
    if bounds.width() <= 0.0 || bounds.height() <= 0.0 { return; }
    match node.image_scale_type {
        // Slice (nine-slice)
        1 => {
            let Some(slice) = node.slice_center else {
                painter.image(texture.id(), bounds, uv, node.image_color);
                return;
            };
            let mut left = (slice[0] - node.image_rect_offset.x).max(0.0) * node.slice_scale;
            let mut top = (slice[1] - node.image_rect_offset.y).max(0.0) * node.slice_scale;
            let mut right = (node.image_rect_offset.x + source_size.x - slice[2]).max(0.0) * node.slice_scale;
            let mut bottom = (node.image_rect_offset.y + source_size.y - slice[3]).max(0.0) * node.slice_scale;
            if left + right > bounds.width() {
                let factor = bounds.width() / (left + right).max(1.0);
                left *= factor; right *= factor;
            }
            if top + bottom > bounds.height() {
                let factor = bounds.height() / (top + bottom).max(1.0);
                top *= factor; bottom *= factor;
            }
            let dx = [bounds.left(), bounds.left() + left, bounds.right() - right, bounds.right()];
            let dy = [bounds.top(), bounds.top() + top, bounds.bottom() - bottom, bounds.bottom()];
            let ux = [uv.left(), slice[0] / texture_size.x, slice[2] / texture_size.x, uv.right()];
            let uy = [uv.top(), slice[1] / texture_size.y, slice[3] / texture_size.y, uv.bottom()];
            for y in 0..3 {
                for x in 0..3 {
                    if dx[x + 1] <= dx[x] || dy[y + 1] <= dy[y] { continue; }
                    painter.image(texture.id(),
                        Rect::from_min_max(Pos2::new(dx[x], dy[y]), Pos2::new(dx[x + 1], dy[y + 1])),
                        Rect::from_min_max(Pos2::new(ux[x], uy[y]), Pos2::new(ux[x + 1], uy[y + 1])),
                        node.image_color);
                }
            }
        }
        // Tile
        2 => {
            let (xs, xo, ys, yo) = node.tile_size.unwrap_or((0.0, 100.0, 0.0, 100.0));
            let tile = Vec2::new(
                (bounds.width() * xs + xo).max(1.0),
                (bounds.height() * ys + yo).max(1.0),
            );
            let columns = (bounds.width() / tile.x).ceil().max(1.0) as usize;
            let rows = (bounds.height() / tile.y).ceil().max(1.0) as usize;
            // Protect malformed assets from creating millions of paint jobs.
            if columns.saturating_mul(rows) > 4096 { return; }
            for row in 0..rows {
                for column in 0..columns {
                    let min = bounds.min + Vec2::new(column as f32 * tile.x, row as f32 * tile.y);
                    let max = (min + tile).min(bounds.max);
                    let fraction = (max - min) / tile;
                    let tile_uv = Rect::from_min_max(uv.min, Pos2::new(
                        uv.min.x + uv.width() * fraction.x,
                        uv.min.y + uv.height() * fraction.y,
                    ));
                    painter.image(texture.id(), Rect::from_min_max(min, max), tile_uv, node.image_color);
                }
            }
        }
        // Fit
        3 => {
            let image_aspect = source_size.x / source_size.y.max(1.0);
            let bounds_aspect = bounds.width() / bounds.height().max(1.0);
            let size = if bounds_aspect > image_aspect {
                Vec2::new(bounds.height() * image_aspect, bounds.height())
            } else {
                Vec2::new(bounds.width(), bounds.width() / image_aspect)
            };
            painter.image(texture.id(), Rect::from_center_size(bounds.center(), size), uv, node.image_color);
        }
        // Crop
        4 => {
            let image_aspect = source_size.x / source_size.y.max(1.0);
            let bounds_aspect = bounds.width() / bounds.height().max(1.0);
            let mut crop_uv = uv;
            if bounds_aspect > image_aspect {
                let visible = image_aspect / bounds_aspect;
                let margin = uv.height() * (1.0 - visible) * 0.5;
                crop_uv.min.y += margin;
                crop_uv.max.y -= margin;
            } else {
                let visible = bounds_aspect / image_aspect;
                let margin = uv.width() * (1.0 - visible) * 0.5;
                crop_uv.min.x += margin;
                crop_uv.max.x -= margin;
            }
            painter.image(texture.id(), bounds, crop_uv, node.image_color);
        }
        // Stretch
        _ => painter.image(texture.id(), bounds, uv, node.image_color),
    }
}

/// Draw enabled ScreenGuis under StarterGui and return the topmost clicked
/// Roblox referent, allowing the editor to synchronize viewport and Explorer.
pub fn draw_starter_gui(
    ui: &mut egui::Ui,
    viewport: Rect,
    dom: &WeakDom,
    textures: &mut std::collections::HashMap<String, egui::TextureHandle>,
    scroll_offsets: &mut std::collections::HashMap<Ref, Vec2>,
    selected: Option<Ref>,
) -> Option<Ref> {
    let Some(starter) = dom.root().children().iter().find_map(|referent| {
        dom.get_by_ref(*referent).filter(|instance| instance.class == "StarterGui").map(|_| *referent)
    }) else { return None; };
    let mut nodes = Vec::new();
    let mut sequence = 0;
    let mut overrides = std::collections::HashMap::new();
    let layout_painter = ui.painter().clone();
    if let Some(starter) = dom.get_by_ref(starter) {
        for (screen_order, child) in starter.children().iter().enumerate() {
            let Some(gui) = dom.get_by_ref(*child) else { continue; };
            if gui.class != "ScreenGui" || matches!(gui.properties.get(&rbx_dom_weak::ustr("Enabled")), Some(Variant::Bool(false))) { continue; }
            let display_order = enum_value(gui.properties.get(&rbx_dom_weak::ustr("DisplayOrder")), 0);
            let global_z = enum_value(gui.properties.get(&rbx_dom_weak::ustr("ZIndexBehavior")), 1) == 0;
            let root_path = vec![(0, screen_order)];
            collect(dom, &layout_painter, *child, viewport, viewport, display_order,
                global_z, 1.0, &root_path, None, &mut overrides, scroll_offsets, &mut sequence, &mut nodes);
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
    let mut scroll_bars: Vec<(Rect, Color32)> = Vec::new();
    for node in nodes {
        if node.clip.width() <= 0.0 || node.clip.height() <= 0.0 { continue; }
        let response = ui.interact(node.clip, ui.make_persistent_id(("roblox_gui", format!("{:?}", node.referent))), egui::Sense::click());
        if node.class == "ScrollingFrame" {
            let maximum = (node.canvas_size - node.content_rect.size()).max(Vec2::ZERO);
            if response.hovered() && (maximum.x > 0.0 || maximum.y > 0.0) {
                let delta = ui.input(|input| input.smooth_scroll_delta);
                if delta != Vec2::ZERO {
                    let entry = scroll_offsets.entry(node.referent).or_insert(Vec2::ZERO);
                    let horizontal_wheel = if maximum.y <= 0.0 { delta.y } else { 0.0 };
                    entry.x = (entry.x - delta.x - horizontal_wheel)
                        .clamp(-node.canvas_position.x, maximum.x-node.canvas_position.x);
                    entry.y = (entry.y - delta.y)
                        .clamp(-node.canvas_position.y, maximum.y-node.canvas_position.y);
                    ui.ctx().request_repaint();
                }
            }
            let effective = (node.canvas_position + scroll_offsets.get(&node.referent).copied().unwrap_or(Vec2::ZERO)).clamp(Vec2::ZERO, maximum);
            let thickness = node.scroll_bar_thickness;
            if thickness > 0.0 && maximum.y > 0.0 {
                let track = Rect::from_min_max(Pos2::new(node.rect.right()-thickness, node.rect.top()), node.rect.right_bottom());
                let thumb_height = (track.height() * node.content_rect.height()/node.canvas_size.y.max(1.0)).max(thickness);
                let travel = (track.height()-thumb_height).max(0.0);
                let top = track.top() + travel * effective.y/maximum.y.max(1.0);
                scroll_bars.push((Rect::from_min_size(Pos2::new(track.left(), top), Vec2::new(thickness, thumb_height)), node.scroll_bar_color));
            }
            if thickness > 0.0 && maximum.x > 0.0 {
                let track = Rect::from_min_max(Pos2::new(node.rect.left(), node.rect.bottom()-thickness), node.rect.right_bottom());
                let thumb_width = (track.width() * node.content_rect.width()/node.canvas_size.x.max(1.0)).max(thickness);
                let travel = (track.width()-thumb_width).max(0.0);
                let left = track.left() + travel * effective.x/maximum.x.max(1.0);
                scroll_bars.push((Rect::from_min_size(Pos2::new(left, track.top()), Vec2::new(thumb_width, thickness)), node.scroll_bar_color));
            }
        }
        let painter = ui.painter().with_clip_rect(node.clip);
        painter.rect_filled(node.rect, node.corner_radius, node.background);
        if let Some(gradient) = &node.gradient {
            paint_gradient(&painter.with_clip_rect(node.rect.intersect(node.clip)), node.rect, gradient);
        }
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
                paint_image(&painter, &node, texture);
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
    let overlay_painter = ui.painter().with_clip_rect(viewport);
    for (rect, color) in scroll_bars {
        overlay_painter.rect_filled(rect, 2.0, color);
    }
    clicked
}
