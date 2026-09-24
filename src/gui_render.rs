//! Roblox GUI preview rendered through egui over the Bevy viewport.
//! This first stage covers StarterGui/ScreenGui layout, frames, text and input
//! selection. Image and 3D BillboardGui rendering build on the same tree.

use bevy_egui::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use rbx_dom_weak::{types::{Ref, Variant}, WeakDom};

pub fn install_roblox_fonts(ctx:&egui::Context) {
    let mut fonts=egui::FontDefinitions::default();
    for (name,bytes) in [
        ("BuilderSansThin",include_bytes!("../content/fonts/BuilderSans-Thin-100.otf").as_slice()),
        ("BuilderSansLight",include_bytes!("../content/fonts/BuilderSans-Light-300.otf").as_slice()),
        ("BuilderSansRegular",include_bytes!("../content/fonts/BuilderSans-Regular-400.otf").as_slice()),
        ("BuilderSansMedium",include_bytes!("../content/fonts/BuilderSans-Medium-500.otf").as_slice()),
        ("BuilderSansBold",include_bytes!("../content/fonts/BuilderSans-Bold-700.otf").as_slice()),
        ("BuilderSansExtraBold",include_bytes!("../content/fonts/BuilderSans-ExtraBold-800.otf").as_slice()),
        ("BuilderExtendedLight",include_bytes!("../content/fonts/BuilderExtended-Light-300.otf").as_slice()),
        ("BuilderExtendedRegular",include_bytes!("../content/fonts/BuilderExtended-Regular-400.otf").as_slice()),
        ("BuilderExtendedSemiBold",include_bytes!("../content/fonts/BuilderExtended-SemiBold-600.otf").as_slice()),
        ("BuilderExtendedBold",include_bytes!("../content/fonts/BuilderExtended-Bold-700.otf").as_slice()),
        ("BuilderExtendedExtraBold",include_bytes!("../content/fonts/BuilderExtended-ExtraBold-800.otf").as_slice()),
        ("BuilderMonoLight",include_bytes!("../content/fonts/BuilderMono-Light-300.otf").as_slice()),
        ("BuilderMonoRegular",include_bytes!("../content/fonts/BuilderMono-Regular-400.otf").as_slice()),
        ("BuilderMonoBold",include_bytes!("../content/fonts/BuilderMono-Bold-700.otf").as_slice()),
        ("SourceSansRegular",include_bytes!("../content/fonts/SourceSans3-Regular.otf").as_slice()),
        ("SourceSansLight",include_bytes!("../content/fonts/SourceSans3-Light.otf").as_slice()),
        ("SourceSansSemiBold",include_bytes!("../content/fonts/SourceSans3-Semibold.otf").as_slice()),
        ("SourceSansBold",include_bytes!("../content/fonts/SourceSans3-Bold.otf").as_slice()),
        ("SourceSansItalic",include_bytes!("../content/fonts/SourceSans3-It.otf").as_slice()),
        ("MontserratRegular",include_bytes!("../content/fonts/Montserrat-Regular.otf").as_slice()),
        ("MontserratMedium",include_bytes!("../content/fonts/Montserrat-Medium.otf").as_slice()),
        ("MontserratBold",include_bytes!("../content/fonts/Montserrat-Bold.otf").as_slice()),
        ("MontserratBlack",include_bytes!("../content/fonts/Montserrat-Black.otf").as_slice()),
        ("ArimoRegular",include_bytes!("../content/fonts/Arimo-Regular.ttf").as_slice()),
        ("ArimoBold",include_bytes!("../content/fonts/Arimo-Bold.ttf").as_slice()),
    ] { fonts.font_data.insert(name.into(),egui::FontData::from_static(bytes).into()); }
    for name in ["BuilderSansThin","BuilderSansLight","BuilderSansRegular","BuilderSansMedium","BuilderSansBold","BuilderSansExtraBold","BuilderExtendedLight","BuilderExtendedRegular","BuilderExtendedSemiBold","BuilderExtendedBold","BuilderExtendedExtraBold","BuilderMonoLight","BuilderMonoRegular","BuilderMonoBold","SourceSansRegular","SourceSansLight","SourceSansSemiBold","SourceSansBold","SourceSansItalic","MontserratRegular","MontserratMedium","MontserratBold","MontserratBlack","ArimoRegular","ArimoBold"] {
        fonts.families.insert(egui::FontFamily::Name(name.into()),vec![name.into()]);
    }
    ctx.set_fonts(fonts);
}

fn roblox_font_family(monospace:bool,extended:bool,source_sans:bool,montserrat:bool,arimo:bool,italic:bool,weight:u16)->egui::FontFamily {
    let name=if source_sans {if italic{"SourceSansItalic"}else if weight>=700{"SourceSansBold"}else if weight>=600{"SourceSansSemiBold"}else if weight<=300{"SourceSansLight"}else{"SourceSansRegular"}}
    else if montserrat {if weight>=800{"MontserratBlack"}else if weight>=700{"MontserratBold"}else if weight>=500{"MontserratMedium"}else{"MontserratRegular"}}
    else if arimo {if weight>=700{"ArimoBold"}else{"ArimoRegular"}}
    else if monospace {if weight>=600{"BuilderMonoBold"}else if weight<=300{"BuilderMonoLight"}else{"BuilderMonoRegular"}}
    else if extended {if weight>=800{"BuilderExtendedExtraBold"}else if weight>=700{"BuilderExtendedBold"}else if weight>=600{"BuilderExtendedSemiBold"}else if weight<=300{"BuilderExtendedLight"}else{"BuilderExtendedRegular"}}
    else if weight>=800{"BuilderSansExtraBold"}else if weight>=600{"BuilderSansBold"}else if weight>=500{"BuilderSansMedium"}else if weight<=100{"BuilderSansThin"}else if weight<=300{"BuilderSansLight"}else{"BuilderSansRegular"};
    egui::FontFamily::Name(name.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiRuntimeEventKind { Layout, MouseEnter, MouseLeave, MouseButton1Down, MouseButton1Up, MouseButton1Click, Activated, ActivatedKeyboard, InputChanged, Focused, FocusLost, SelectionGained, SelectionLost, TextChanged }
#[derive(Debug, Clone, Copy)]
pub struct GuiRuntimeEvent { pub referent: Ref, pub kind: GuiRuntimeEventKind, pub position: [f32;2], pub delta: [f32;2] }

#[derive(Clone, Copy)]
struct RoundedMask { rect: Rect, radius: f32, rotation: f32 }
#[derive(Clone)]
struct GuiGradient { rotation:f32, offset:Vec2, colors:Vec<Color32>, scale:f32, tile_mode:i32, kind:i32 }

#[derive(Clone, Copy)]
struct SurfaceWarp { source: Rect, rotation: f32, quad: [Pos2; 4] }

struct GuiNode {
    referent: Ref,
    class: String,
    rect: Rect,
    background_rect: Rect,
    content_rect: Rect,
    gui_scale: f32,
    clip: Rect,
    rounded_clips: Vec<RoundedMask>,
    surface_warp: Option<SurfaceWarp>,
    display_order: i32,
    z: i32,
    global_z: bool,
    sort_path: Vec<(i32, usize)>,
    order: usize,
    background: Color32,
    border: Color32,
    border_size: f32,
    corner_radius: f32,
    rotation: f32,
    ui_stroke: Option<(f32, Color32, f32, egui::StrokeKind)>,
    text_ui_stroke: Option<(f32, Color32)>,
    gradient: Option<GuiGradient>,
    gradient_rect: Rect,
    group_gradients: Vec<(GuiGradient,Rect)>,
    text: String,
    text_color: Color32,
    text_size: f32,
    text_x_alignment: i32,
    text_y_alignment: i32,
    text_wrapped: bool,
    text_scaled: bool,
    text_truncate: i32,
    rich_text: bool,
    line_height: f32,
    font_monospace: bool,
    font_extended: bool,
    font_source_sans: bool,
    font_montserrat: bool,
    font_arimo: bool,
    font_weight: u16,
    font_italic: bool,
    font_bold: bool,
    text_min_size: f32,
    text_max_size: f32,
    text_stroke: Color32,
    multiline: bool,
    clear_text_on_focus: bool,
    placeholder_text: String,
    placeholder_color: Color32,
    image: Option<String>,
    hover_image: Option<String>,
    pressed_image: Option<String>,
    image_color: Color32,
    auto_button_color: bool,
    interactable: bool,
    selectable: bool,
    selection_order: i32,
    image_rect_offset: Vec2,
    image_rect_size: Vec2,
    image_scale_type: i32,
    image_pixelated: bool,
    tile_size: Option<(f32, f32, f32, f32)>,
    slice_center: Option<[f32; 4]>,
    slice_scale: f32,
    canvas_size: Vec2,
    canvas_position: Vec2,
    scroll_bar_thickness: f32,
    scroll_bar_color: Color32,
    scrolling_direction: i32,
    scrolling_enabled: bool,
    scroll_rate: f32,
    vertical_scroll_bar_left: bool,
    scroll_top_image: Option<String>,
    scroll_mid_image: Option<String>,
    scroll_bottom_image: Option<String>,
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
        let mut text = match instance.properties.get(&rbx_dom_weak::ustr("Text")) {
            Some(Variant::String(text)) => text.clone(),
            _ => String::new(),
        };
        if bool_value(instance.properties.get(&rbx_dom_weak::ustr("RichText")), false) {
            let mut plain = String::new();
            let mut in_tag = false;
            for character in text.chars() {
                if character == '<' { in_tag = true; continue; }
                if character == '>' { in_tag = false; continue; }
                if !in_tag { plain.push(character); }
            }
            text = decode_rich_entities(&plain);
        }
        if !text.is_empty() {
            let font_size = number(instance.properties.get(&rbx_dom_weak::ustr("TextSize")), 14.0).max(1.0) * scale;
            let wrapped = bool_value(instance.properties.get(&rbx_dom_weak::ustr("TextWrapped")), false);
            let wrap_width = if wrapped && size.x > 0.0 { size.x } else { f32::INFINITY };
            let line_height = number(instance.properties.get(&rbx_dom_weak::ustr("LineHeight")), 1.0).max(0.1);
            let legacy_font = enum_value(instance.properties.get(&rbx_dom_weak::ustr("Font")), 3);
            let face_mono = matches!(instance.properties.get(&rbx_dom_weak::ustr("FontFace")),
                Some(Variant::Font(font)) if font.family.to_ascii_lowercase().contains("mono") || font.family.to_ascii_lowercase().contains("code"));
            let face_extended = matches!(instance.properties.get(&rbx_dom_weak::ustr("FontFace")),Some(Variant::Font(font)) if font.family.to_ascii_lowercase().contains("extended"));
            let face_source = matches!(instance.properties.get(&rbx_dom_weak::ustr("FontFace")),Some(Variant::Font(font)) if font.family.to_ascii_lowercase().contains("source sans") || font.family.to_ascii_lowercase().contains("sourcesans"));
            let face_montserrat = matches!(instance.properties.get(&rbx_dom_weak::ustr("FontFace")),Some(Variant::Font(font)) if font.family.to_ascii_lowercase().contains("montserrat")||font.family.to_ascii_lowercase().contains("gotham"));
            let face_arimo = matches!(instance.properties.get(&rbx_dom_weak::ustr("FontFace")),Some(Variant::Font(font)) if font.family.to_ascii_lowercase().contains("arimo")||font.family.to_ascii_lowercase().contains("arial"));
            let face_italic = matches!(instance.properties.get(&rbx_dom_weak::ustr("FontFace")),Some(Variant::Font(font)) if format!("{:?}",font.style).to_ascii_lowercase().contains("italic"));
            let family = roblox_font_family(matches!(legacy_font,10|41) || face_mono,face_extended,matches!(legacy_font,3|4|5|6|16)||face_source,matches!(legacy_font,17|18|19|20)||face_montserrat,matches!(legacy_font,1|2|50|51)||face_arimo,legacy_font==6||face_italic,if matches!(legacy_font,20|49){800}else if matches!(legacy_font,2|4|16|19|48|51){700}else if matches!(legacy_font,18|47){500}else if legacy_font==5{300}else{400});
            let mut job = egui::text::LayoutJob::simple(text, FontId::new(font_size, family), Color32::WHITE, wrap_width);
            if let Some(section) = job.sections.first_mut() { section.format.line_height = Some(font_size * line_height); }
            let galley = painter.layout_job(job);
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
            let mut child_sizes = Vec::new();
            for child_ref in instance.children() {
                let Some(child) = dom.get_by_ref(*child_ref) else { continue; };
                if !is_gui_object(&child.class) || !visible(child) { continue; }
                let child_scale = child_of_class(dom, child, "UIScale")
                    .map(|modifier| number(modifier.properties.get(&rbx_dom_weak::ustr("Scale")), 1.0).max(0.0))
                    .unwrap_or(1.0);
                let rect = gui_rect(dom, painter, child, provisional_content, scale * child_scale);
                main += if horizontal { rect.width() } else { rect.height() };
                cross = cross.max(if horizontal { rect.height() } else { rect.width() });
                child_sizes.push(rect.size());
                count += 1;
            }
            main += gap * count.saturating_sub(1) as f32;
            if bool_value(layout.properties.get(&rbx_dom_weak::ustr("Wraps")), false) {
                let available = if horizontal { provisional_content.width() } else { provisional_content.height() };
                let mut line_main=0.0f32; let mut line_cross=0.0f32; let mut total_cross=0.0f32; let mut widest=0.0f32; let mut lines=0usize;
                for child in child_sizes {
                    let child_main=if horizontal { child.x } else { child.y };
                    let child_cross=if horizontal { child.y } else { child.x };
                    if line_main>0.0 && line_main+gap+child_main>available {
                        widest=widest.max(line_main); total_cross+=line_cross; lines+=1; line_main=0.0; line_cross=0.0;
                    }
                    if line_main>0.0 { line_main+=gap; }
                    line_main+=child_main; line_cross=line_cross.max(child_cross);
                }
                if line_main>0.0 { widest=widest.max(line_main); total_cross+=line_cross; lines+=1; }
                total_cross+=gap*lines.saturating_sub(1) as f32;
                required=if horizontal { Vec2::new(widest,total_cross) } else { Vec2::new(total_cross,widest) };
            } else {
                required = if horizontal { Vec2::new(main, cross) } else { Vec2::new(cross, main) };
            }
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

fn shade_color(color: Color32, factor: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (color.r() as f32 * factor).round() as u8,
        (color.g() as f32 * factor).round() as u8,
        (color.b() as f32 * factor).round() as u8,
        color.a(),
    )
}

fn multiply_color(a: Color32, b: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (a.r() as u16 * b.r() as u16 / 255) as u8,
        (a.g() as u16 * b.g() as u16 / 255) as u8,
        (a.b() as u16 * b.b() as u16 / 255) as u8,
        (a.a() as u16 * b.a() as u16 / 255) as u8,
    )
}

fn gradient_samples(instance: &rbx_dom_weak::Instance) -> Option<GuiGradient> {
    let rotation = number(instance.properties.get(&rbx_dom_weak::ustr("Rotation")), 0.0);
    let offset = vector2(instance.properties.get(&rbx_dom_weak::ustr("Offset")), Vec2::ZERO);
    let scale=number(instance.properties.get(&rbx_dom_weak::ustr("Scale")),1.0).abs().max(0.0001);
    let tile_mode=enum_value(instance.properties.get(&rbx_dom_weak::ustr("TileMode")),0);
    let kind=enum_value(instance.properties.get(&rbx_dom_weak::ustr("Type")),0);
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
            ((a.color.r + (b.color.r - a.color.r) * mix) * 255.0) as u8,
            ((a.color.g + (b.color.g - a.color.g) * mix) * 255.0) as u8,
            ((a.color.b + (b.color.b - a.color.b) * mix) * 255.0) as u8,
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
        Color32::from_rgba_unmultiplied(rgb.r(), rgb.g(), rgb.b(), ((1.0-transparency)*255.0) as u8)
    };
    Some(GuiGradient{rotation,offset,colors:(0..=32).map(|index|sample_color(index as f32/32.0)).collect(),scale,tile_mode,kind})
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
           global_z: bool, inherited_scale: f32, inherited_opacity: f32,
           inherited_tint: Color32, parent_path: &[(i32, usize)], forced_rect: Option<Rect>,
           overrides: &mut std::collections::HashMap<Ref, Rect>,
           scroll_offsets: &std::collections::HashMap<Ref, Vec2>, sequence: &mut usize,
           nodes: &mut Vec<GuiNode>) {
    let Some(instance) = dom.get_by_ref(referent) else { return; };
    if !visible(instance) { return; }
    // Roblox only flattens CanvasGroup under Sibling ZIndexBehavior. Under
    // Global it behaves as an ordinary clipping GuiObject.
    let composited_group = instance.class == "CanvasGroup" && !global_z;
    let group_transparency = if composited_group {
        number(instance.properties.get(&rbx_dom_weak::ustr("GroupTransparency")), 0.0).clamp(0.0, 1.0)
    } else { 0.0 };
    let opacity = inherited_opacity * (1.0 - group_transparency);
    let local_tint = if composited_group {
        color(instance.properties.get(&rbx_dom_weak::ustr("GroupColor3")), 255, [255, 255, 255])
    } else { Color32::WHITE };
    let tint = multiply_color(inherited_tint, local_tint);
    let modulation = Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), (opacity * 255.0) as u8);
    let local_scale = child_of_class(dom, instance, "UIScale")
        .map(|modifier| number(modifier.properties.get(&rbx_dom_weak::ustr("Scale")), 1.0).max(0.0))
        .unwrap_or(1.0);
    let scale = inherited_scale * local_scale;
    let is_screen = instance.class == "ScreenGui";
    let rect = forced_rect.or_else(|| overrides.get(&referent).copied()).unwrap_or_else(|| {
        if is_screen { parent_rect } else { gui_rect(dom, painter, instance, parent_rect, scale) }
    });
    // CanvasGroup's backing texture is exactly its AbsoluteSize, therefore it
    // always clips regardless of the serialized ClipsDescendants value.
    let clips = instance.class == "CanvasGroup" || matches!(instance.properties.get(&rbx_dom_weak::ustr("ClipsDescendants")), Some(Variant::Bool(true)));
    let child_clip = if clips { parent_clip.intersect(rect) } else { parent_clip };
    let z = match instance.properties.get(&rbx_dom_weak::ustr("ZIndex")) {
        Some(Variant::Int32(v)) => *v,
        Some(Variant::Int64(v)) => *v as i32,
        _ => 1,
    };
    let current_order = *sequence;
    let mut sort_path = parent_path.to_vec();
    sort_path.push((z, current_order));
    let mut content_rect = padded_rect(dom, instance, rect, scale);

    if is_gui_object(&instance.class) {
        let transparency = number(instance.properties.get(&rbx_dom_weak::ustr("BackgroundTransparency")), 0.0).clamp(0.0, 1.0);
        let alpha = ((1.0 - transparency) * 255.0).round() as u8;
        let background = multiply_color(
            color(instance.properties.get(&rbx_dom_weak::ustr("BackgroundColor3")), alpha, [255,255,255]),
            modulation,
        );
        let gradient = child_of_class(dom, instance, "UIGradient")
            .and_then(|modifier| bool_value(modifier.properties.get(&rbx_dom_weak::ustr("Enabled")), true)
                .then(|| gradient_samples(modifier)).flatten());
        let text_transparency = number(instance.properties.get(&rbx_dom_weak::ustr("TextTransparency")), 0.0).clamp(0.0, 1.0);
        let corner_radius = child_of_class(dom, instance, "UICorner")
            .map(|corner| udim_pixels(corner.properties.get(&rbx_dom_weak::ustr("CornerRadius")), rect.width().min(rect.height()), scale))
            .unwrap_or(0.0)
            .clamp(0.0, rect.width().min(rect.height()) * 0.5);
        let (ui_stroke, text_ui_stroke) = child_of_class(dom, instance, "UIStroke")
            .and_then(|stroke| {
                if !bool_value(stroke.properties.get(&rbx_dom_weak::ustr("Enabled")), true) { return None; }
                let sizing_scale = if enum_value(stroke.properties.get(&rbx_dom_weak::ustr("StrokeSizingMode")), 0) == 1 { scale } else { 1.0 };
                let thickness = number(stroke.properties.get(&rbx_dom_weak::ustr("Thickness")), 1.0).max(0.0) * sizing_scale;
                let transparency = number(stroke.properties.get(&rbx_dom_weak::ustr("Transparency")), 0.0).clamp(0.0, 1.0);
                if thickness <= 0.0 || transparency >= 1.0 { return None; }
                let stroke_color = multiply_color(color(stroke.properties.get(&rbx_dom_weak::ustr("Color")),
                    ((1.0-transparency)*255.0) as u8, [0,0,0]), modulation);
                let contextual_text = enum_value(stroke.properties.get(&rbx_dom_weak::ustr("ApplyStrokeMode")), 0) == 0
                    && matches!(instance.class.as_str(), "TextLabel" | "TextButton" | "TextBox");
                if contextual_text {
                    Some((None, Some((thickness, stroke_color))))
                } else {
                    let offset = udim_pixels(stroke.properties.get(&rbx_dom_weak::ustr("BorderOffset")),
                        rect.width().min(rect.height()), scale);
                    let position = match enum_value(stroke.properties.get(&rbx_dom_weak::ustr("BorderStrokePosition")), 0) {
                        1 => egui::StrokeKind::Inside,
                        2 => egui::StrokeKind::Outside,
                        _ => egui::StrokeKind::Middle,
                    };
                    Some((Some((thickness, stroke_color, offset, position)), None))
                }
            }).unwrap_or((None, None));
        let legacy_font = enum_value(instance.properties.get(&rbx_dom_weak::ustr("Font")), 3);
        let font_face = match instance.properties.get(&rbx_dom_weak::ustr("FontFace")) {
            Some(Variant::Font(font)) => Some(font), _ => None,
        };
        let face_family = font_face.map(|font| font.family.to_ascii_lowercase()).unwrap_or_default();
        let face_style = font_face.map(|font| format!("{:?}", font.style).to_ascii_lowercase()).unwrap_or_default();
        let face_weight = font_face.map(|font| format!("{:?}", font.weight).to_ascii_lowercase()).unwrap_or_default();
        let font_monospace = matches!(legacy_font,10|41) || face_family.contains("mono") || face_family.contains("code");
        let font_extended = face_family.contains("extended");
        let font_source_sans = matches!(legacy_font,3|4|5|6|16) || face_family.contains("source sans") || face_family.contains("sourcesans");
        let font_montserrat = matches!(legacy_font,17|18|19|20) || face_family.contains("montserrat") || face_family.contains("gotham");
        let font_arimo = matches!(legacy_font,1|2|50|51) || face_family.contains("arimo") || face_family.contains("arial");
        let font_weight = if face_weight.contains("extra")||face_weight.contains("800")||face_weight.contains("900") {800} else if face_weight.contains("semi")||face_weight.contains("bold")||face_weight.contains("600")||face_weight.contains("700") {700} else if face_weight.contains("medium")||face_weight.contains("500") {500} else if face_weight.contains("light")||face_weight.contains("300") {300} else if face_weight.contains("thin")||face_weight.contains("100") {100} else {match legacy_font {20|49=>800,2|4|16|19|48|51=>700,18|47=>500,5=>300,_=>400}};
        let font_italic = legacy_font == 6 || face_style.contains("italic");
        let font_bold = font_weight>=600;
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
        let scroll_bar_thickness = number(instance.properties.get(&rbx_dom_weak::ustr("ScrollBarThickness")), 12.0).max(0.0) * scale;
        if instance.class == "ScrollingFrame" && scroll_bar_thickness > 0.0 {
            let horizontal_inset = enum_value(instance.properties.get(&rbx_dom_weak::ustr("HorizontalScrollBarInset")), 0);
            let vertical_inset = enum_value(instance.properties.get(&rbx_dom_weak::ustr("VerticalScrollBarInset")), 0);
            let horizontal_visible = canvas_size.x > content_rect.width();
            let vertical_visible = canvas_size.y > content_rect.height();
            if horizontal_inset == 2 || (horizontal_inset == 1 && horizontal_visible) {
                content_rect.max.y = (content_rect.max.y-scroll_bar_thickness).max(content_rect.min.y);
            }
            if vertical_inset == 2 || (vertical_inset == 1 && vertical_visible) {
                let left = enum_value(instance.properties.get(&rbx_dom_weak::ustr("VerticalScrollBarPosition")), 0) == 1;
                if left { content_rect.min.x = (content_rect.min.x+scroll_bar_thickness).min(content_rect.max.x); }
                else { content_rect.max.x = (content_rect.max.x-scroll_bar_thickness).max(content_rect.min.x); }
            }
        }
        let scroll_bar_transparency = number(instance.properties.get(&rbx_dom_weak::ustr("ScrollBarImageTransparency")), 0.0).clamp(0.0, 1.0);
        let order = *sequence;
        *sequence += 1;
        nodes.push(GuiNode {
            referent,
            class: instance.class.to_string(),
            rect,
            background_rect:rect,
            content_rect,
            gui_scale: scale,
            clip: parent_clip.intersect(rect),
            rounded_clips: Vec::new(),
            surface_warp: None,
            display_order,
            z,
            global_z,
            sort_path: sort_path.clone(),
            order,
            background,
            border: multiply_color(color(instance.properties.get(&rbx_dom_weak::ustr("BorderColor3")), 255, [27,42,53]), modulation),
            border_size: number(instance.properties.get(&rbx_dom_weak::ustr("BorderSizePixel")), 1.0).max(0.0) * scale,
            corner_radius,
            rotation: number(instance.properties.get(&rbx_dom_weak::ustr("Rotation")), 0.0),
            ui_stroke,
            text_ui_stroke,
            gradient,
            gradient_rect: rect,
            group_gradients: Vec::new(),
            text: match instance.properties.get(&rbx_dom_weak::ustr("Text")) { Some(Variant::String(v)) => v.clone(), _ => String::new() },
            text_color: multiply_color(color(instance.properties.get(&rbx_dom_weak::ustr("TextColor3")), ((1.0-text_transparency)*255.0) as u8, [0,0,0]), modulation),
            text_size: (number(instance.properties.get(&rbx_dom_weak::ustr("TextSize")), 14.0) * scale).clamp(1.0, 400.0),
            text_x_alignment: enum_value(instance.properties.get(&rbx_dom_weak::ustr("TextXAlignment")), 1),
            text_y_alignment: enum_value(instance.properties.get(&rbx_dom_weak::ustr("TextYAlignment")), 1),
            text_wrapped: bool_value(instance.properties.get(&rbx_dom_weak::ustr("TextWrapped")), false),
            text_scaled: bool_value(instance.properties.get(&rbx_dom_weak::ustr("TextScaled")), false),
            text_truncate: enum_value(instance.properties.get(&rbx_dom_weak::ustr("TextTruncate")), 0),
            rich_text: bool_value(instance.properties.get(&rbx_dom_weak::ustr("RichText")), false),
            line_height: number(instance.properties.get(&rbx_dom_weak::ustr("LineHeight")), 1.0).max(0.1),
            font_monospace,
            font_extended,
            font_source_sans,
            font_montserrat,
            font_arimo,
            font_weight,
            font_italic,
            font_bold,
            text_min_size,
            text_max_size,
            text_stroke: multiply_color(color(
                instance.properties.get(&rbx_dom_weak::ustr("TextStrokeColor3")),
                ((1.0-number(instance.properties.get(&rbx_dom_weak::ustr("TextStrokeTransparency")), 1.0).clamp(0.0,1.0))*255.0) as u8,
                [0,0,0],
            ), modulation),
            multiline: bool_value(instance.properties.get(&rbx_dom_weak::ustr("MultiLine")), false),
            clear_text_on_focus: bool_value(instance.properties.get(&rbx_dom_weak::ustr("ClearTextOnFocus")), true),
            placeholder_text: match instance.properties.get(&rbx_dom_weak::ustr("PlaceholderText")) {
                Some(Variant::String(value)) => value.clone(), _ => String::new(),
            },
            placeholder_color: multiply_color(color(
                instance.properties.get(&rbx_dom_weak::ustr("PlaceholderColor3")), 255, [178,178,178],
            ), modulation),
            image: content(instance.properties.get(&rbx_dom_weak::ustr("Image"))
                .or_else(|| instance.properties.get(&rbx_dom_weak::ustr("ImageContent")))),
            hover_image: content(instance.properties.get(&rbx_dom_weak::ustr("HoverImage"))),
            pressed_image: content(instance.properties.get(&rbx_dom_weak::ustr("PressedImage"))),
            image_color: multiply_color(color(
                instance.properties.get(&rbx_dom_weak::ustr("ImageColor3")),
                ((1.0-number(instance.properties.get(&rbx_dom_weak::ustr("ImageTransparency")), 0.0).clamp(0.0,1.0))*255.0) as u8,
                [255,255,255],
            ), modulation),
            auto_button_color: bool_value(instance.properties.get(&rbx_dom_weak::ustr("AutoButtonColor")), true),
            interactable: bool_value(instance.properties.get(&rbx_dom_weak::ustr("Active")), true)
                && bool_value(instance.properties.get(&rbx_dom_weak::ustr("Interactable")), true),
            selectable: bool_value(instance.properties.get(&rbx_dom_weak::ustr("Selectable")),
                matches!(instance.class.as_str(),"TextButton"|"ImageButton"|"TextBox")),
            selection_order: number(instance.properties.get(&rbx_dom_weak::ustr("SelectionOrder")),0.0) as i32,
            image_rect_offset: vector2(instance.properties.get(&rbx_dom_weak::ustr("ImageRectOffset")), Vec2::ZERO),
            image_rect_size: vector2(instance.properties.get(&rbx_dom_weak::ustr("ImageRectSize")), Vec2::ZERO),
            image_scale_type: enum_value(instance.properties.get(&rbx_dom_weak::ustr("ScaleType")), 0),
            image_pixelated: enum_value(instance.properties.get(&rbx_dom_weak::ustr("ResampleMode")), 0) == 1,
            tile_size: udim2_tuple(instance.properties.get(&rbx_dom_weak::ustr("TileSize"))),
            slice_center: match instance.properties.get(&rbx_dom_weak::ustr("SliceCenter")) {
                Some(Variant::Rect(rect)) => Some([rect.min.x, rect.min.y, rect.max.x, rect.max.y]),
                _ => None,
            },
            slice_scale: number(instance.properties.get(&rbx_dom_weak::ustr("SliceScale")), 1.0).max(0.0),
            canvas_size,
            canvas_position: vector2(instance.properties.get(&rbx_dom_weak::ustr("CanvasPosition")), Vec2::ZERO) * scale,
            scroll_bar_thickness,
            scroll_bar_color: multiply_color(color(instance.properties.get(&rbx_dom_weak::ustr("ScrollBarImageColor3")),
                ((1.0-scroll_bar_transparency)*255.0) as u8, [255,255,255]), modulation),
            scrolling_direction: enum_value(instance.properties.get(&rbx_dom_weak::ustr("ScrollingDirection")), 4),
            scrolling_enabled: bool_value(instance.properties.get(&rbx_dom_weak::ustr("ScrollingEnabled")), true),
            scroll_rate: number(instance.properties.get(&rbx_dom_weak::ustr("ScrollRate")), 1.0).max(0.0),
            vertical_scroll_bar_left: enum_value(instance.properties.get(&rbx_dom_weak::ustr("VerticalScrollBarPosition")), 0) == 1,
            scroll_top_image: content(instance.properties.get(&rbx_dom_weak::ustr("TopImage"))
                .or_else(|| instance.properties.get(&rbx_dom_weak::ustr("TopImageContent")))),
            scroll_mid_image: content(instance.properties.get(&rbx_dom_weak::ustr("MidImage"))
                .or_else(|| instance.properties.get(&rbx_dom_weak::ustr("MidImageContent")))),
            scroll_bottom_image: content(instance.properties.get(&rbx_dom_weak::ustr("BottomImage"))
                .or_else(|| instance.properties.get(&rbx_dom_weak::ustr("BottomImageContent")))),
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
                global_z, scale, opacity, tint, &sort_path, None, overrides, scroll_offsets, sequence, nodes);
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
                global_z, scale, opacity, tint, &sort_path, Some(Rect::from_min_size(Pos2::new(x, y), natural.size())), overrides, scroll_offsets, sequence, nodes);
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
                global_z, scale, opacity, tint, &sort_path, Some(Rect::from_min_size(min, cell)), overrides, scroll_offsets, sequence, nodes);
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
        let mut children: Vec<(usize, Ref, i32, String, f32, Rect, f32, f32)> = instance.children().iter().enumerate()
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
                let (grow, shrink) = child_of_class(dom, child, "UIFlexItem").map(|item| {
                    match enum_value(item.properties.get(&rbx_dom_weak::ustr("FlexMode")), 0) {
                        1 => (1.0, 0.0),
                        2 => (0.0, 1.0),
                        3 => (1.0, 1.0),
                        4 => (
                            number(item.properties.get(&rbx_dom_weak::ustr("GrowRatio")), 0.0).max(0.0),
                            number(item.properties.get(&rbx_dom_weak::ustr("ShrinkRatio")), 0.0).max(0.0),
                        ),
                        _ => (0.0, 0.0),
                    }
                }).unwrap_or((f32::NAN, f32::NAN));
                Some((index, *referent, order, child.name.to_string(), child_scale, child_rect, grow, shrink))
            }).collect();
        children.sort_by(|left, right| {
            if sort_order == 1 { left.2.cmp(&right.2).then(left.0.cmp(&right.0)) }
            else { left.3.cmp(&right.3).then(left.0.cmp(&right.0)) }
        });
        let wraps = bool_value(layout.properties.get(&rbx_dom_weak::ustr("Wraps")), false);
        if wraps {
            let available_main = if horizontal { child_parent_rect.width() } else { child_parent_rect.height() };
            let mut lines: Vec<Vec<(usize, Ref, i32, String, f32, Rect, f32, f32)>> = Vec::new();
            for item in children {
                let item_main = if horizontal { item.5.width() } else { item.5.height() };
                let occupied = lines.last().map(|line| line.iter().map(|item| if horizontal { item.5.width() } else { item.5.height() }).sum::<f32>()
                    + padding*line.len().saturating_sub(1) as f32).unwrap_or(0.0);
                if !lines.is_empty() && !lines.last().unwrap().is_empty() && occupied+padding+item_main > available_main {
                    lines.push(Vec::new());
                }
                if lines.is_empty() { lines.push(Vec::new()); }
                lines.last_mut().unwrap().push(item);
            }
            let line_cross: Vec<f32> = lines.iter().map(|line| line.iter()
                .map(|item| if horizontal { item.5.height() } else { item.5.width() }).fold(0.0, f32::max)).collect();
            let total_cross = line_cross.iter().sum::<f32>() + padding*lines.len().saturating_sub(1) as f32;
            let available_cross = if horizontal { child_parent_rect.height() } else { child_parent_rect.width() };
            let overall_cross_alignment = if horizontal { vertical_alignment } else { horizontal_alignment };
            let mut cross_cursor = match overall_cross_alignment { 1 => (available_cross-total_cross)*0.5, 2 => available_cross-total_cross, _ => 0.0 }.max(0.0);
            let layout_line_alignment = enum_value(layout.properties.get(&rbx_dom_weak::ustr("ItemLineAlignment")), 0);
            for (line_index, mut line) in lines.into_iter().enumerate() {
                let mut line_main = line.iter().map(|item| if horizontal { item.5.width() } else { item.5.height() }).sum::<f32>()
                    + padding*line.len().saturating_sub(1) as f32;
                let remaining = (available_main-line_main).max(0.0);
                let flex = enum_value(layout.properties.get(&rbx_dom_weak::ustr(
                    if horizontal { "HorizontalFlex" } else { "VerticalFlex" })), 0);
                let ratios: Vec<f32> = line.iter().map(|item| {
                    if item.6.is_nan() { if flex == 1 { 1.0 } else { 0.0 } } else { item.6 }
                }).collect();
                let ratio_total: f32 = ratios.iter().sum();
                if ratio_total > 0.0 && remaining > 0.0 {
                    for (item, ratio) in line.iter_mut().zip(ratios) {
                        let basis = if horizontal { item.5.width() } else { item.5.height() };
                        let adjusted = basis+remaining*ratio/ratio_total;
                        let size = if horizontal { Vec2::new(adjusted,item.5.height()) } else { Vec2::new(item.5.width(),adjusted) };
                        item.5=Rect::from_min_size(item.5.min,size);
                    }
                    line_main=available_main;
                }
                let main_alignment = if horizontal { horizontal_alignment } else { vertical_alignment };
                let mut main_cursor = match main_alignment { 1 => (available_main-line_main)*0.5, 2 => available_main-line_main, _ => 0.0 }.max(0.0);
                for (_, child, _, _, _, natural, _, _) in line {
                    let cross_size = if horizontal { natural.height() } else { natural.width() };
                    let item_alignment = dom.get_by_ref(child).and_then(|instance| child_of_class(dom, instance, "UIFlexItem"))
                        .map(|item| enum_value(item.properties.get(&rbx_dom_weak::ustr("ItemLineAlignment")), 0)).unwrap_or(0);
                    let alignment = if item_alignment != 0 { item_alignment } else { layout_line_alignment };
                    let cross_offset = match alignment { 2 => (line_cross[line_index]-cross_size)*0.5, 3 => line_cross[line_index]-cross_size, _ => 0.0 }.max(0.0);
                    let mut arranged_size = natural.size();
                    if alignment == 4 {
                        if horizontal { arranged_size.y = line_cross[line_index]; } else { arranged_size.x = line_cross[line_index]; }
                    }
                    let min = if horizontal { child_parent_rect.min+Vec2::new(main_cursor, cross_cursor+cross_offset) }
                        else { child_parent_rect.min+Vec2::new(cross_cursor+cross_offset, main_cursor) };
                    collect(dom, painter, child, child_parent_rect, child_clip, display_order,
                        global_z, scale, opacity, tint, &sort_path, Some(Rect::from_min_size(min, arranged_size)), overrides, scroll_offsets, sequence, nodes);
                    main_cursor += (if horizontal { arranged_size.x } else { arranged_size.y }) + padding;
                }
                cross_cursor += line_cross[line_index]+padding;
            }
        } else {
        let mut total = children.iter().map(|(_, _, _, _, _, rect, _, _)| if horizontal { rect.width() } else { rect.height() }).sum::<f32>()
            + padding * children.len().saturating_sub(1) as f32;
        let available = if horizontal { child_parent_rect.width() } else { child_parent_rect.height() };
        let flex = enum_value(layout.properties.get(&rbx_dom_weak::ustr(
            if horizontal { "HorizontalFlex" } else { "VerticalFlex" })), 0);
        let remaining = available-total;
        let extra = remaining.max(0.0);
        let mut effective_padding = padding;
        let mut flex_inset = 0.0;
        if !children.is_empty() {
            let ratios: Vec<f32> = children.iter().map(|item| {
                let explicit = if remaining >= 0.0 { item.6 } else { item.7 };
                if explicit.is_nan() { if flex == 1 { 1.0 } else { 0.0 } } else { explicit }
            }).collect();
            let ratio_total: f32 = ratios.iter().sum();
            if ratio_total > 0.0 && remaining != 0.0 {
                for (item, ratio) in children.iter_mut().zip(ratios) {
                    let basis = if horizontal { item.5.width() } else { item.5.height() };
                    let adjusted = (basis + remaining*ratio/ratio_total).max(0.0);
                    let size = if horizontal { Vec2::new(adjusted, item.5.height()) }
                        else { Vec2::new(item.5.width(), adjusted) };
                    item.5 = Rect::from_min_size(item.5.min, size);
                }
                total = children.iter().map(|item| if horizontal { item.5.width() } else { item.5.height() }).sum::<f32>()
                    + padding*children.len().saturating_sub(1) as f32;
            } else if remaining > 0.0 {
                match flex {
                    2 => { effective_padding += extra/children.len() as f32; flex_inset = extra/(children.len() as f32*2.0); total=available; }
                    3 if children.len()>1 => { effective_padding += extra/(children.len()-1) as f32; total=available; }
                    4 => { effective_padding += extra/(children.len()+1) as f32; flex_inset=extra/(children.len()+1) as f32; total=available; }
                    _ => {}
                }
            }
        }
        let main_alignment = if horizontal { horizontal_alignment } else { vertical_alignment };
        let mut cursor = flex_inset + match main_alignment {
            1 => (available - total) * 0.5,
            2 => available - total,
            _ => 0.0,
        }.max(0.0);
        for (_, child, _, _, _child_scale, natural, _, _) in children {
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
                global_z, scale, opacity, tint, &sort_path, Some(arranged), overrides, scroll_offsets, sequence, nodes);
            cursor += (if horizontal { natural.width() } else { natural.height() }) + effective_padding;
        }
        }
    } else {
        for child in instance.children() {
            collect(dom, painter, *child, child_parent_rect, child_clip, display_order,
                global_z, scale, opacity, tint, &sort_path, None, overrides, scroll_offsets, sequence, nodes);
        }
    }
}

fn paint_gradient(painter: &egui::Painter, rect: Rect, gradient_rect:Rect, base: Color32,
                  gradient: Option<&GuiGradient>, group_gradients:&[(GuiGradient,Rect)], object_rotation: f32,
                  corner_radius: f32, rounded_clips: &[RoundedMask], surface_warp: Option<SurfaceWarp>) {
    if (gradient.is_none()&&group_gradients.is_empty()) || rect.width() <= 0.0 || rect.height() <= 0.0 { return; }
    // A regular mesh gives smooth interpolation at arbitrary angles and lets
    // UICorner alpha-mask the same gradient without leaking through corners.
    let divisions = if corner_radius > 0.0 { 16usize } else { 8usize };
    let mut mesh = egui::Mesh::default();
    for y in 0..=divisions {
        for x in 0..=divisions {
            let fx=x as f32/divisions as f32; let fy=y as f32/divisions as f32;
            let unrotated=Pos2::new(rect.left()+rect.width()*fx, rect.top()+rect.height()*fy);
            let mut color=apply_gradients(base,unrotated,gradient,gradient_rect,group_gradients);
            let mut coverage=rounded_coverage(unrotated,rect,corner_radius);
            let mut pos=if object_rotation.abs()>=0.001 { rotate_point(unrotated,rect.center(),object_rotation.to_radians()) } else { unrotated };
            if let Some(warp)=surface_warp { pos=warp_surface_point(pos,warp); }
            for mask in rounded_clips {
                let mask_local=rotate_point(pos,mask.rect.center(),-mask.rotation.to_radians());
                coverage*=rounded_coverage(mask_local,mask.rect,mask.radius);
            }
            color=Color32::from_rgba_unmultiplied(color.r(),color.g(),color.b(),(color.a() as f32*coverage) as u8);
            mesh.vertices.push(egui::epaint::Vertex { pos, uv: egui::epaint::WHITE_UV, color });
        }
    }
    let stride=divisions+1;
    for y in 0..divisions { for x in 0..divisions {
        let a=(y*stride+x) as u32; let b=a+1; let c=a+stride as u32; let d=c+1;
        mesh.add_triangle(a,b,d); mesh.add_triangle(a,d,c);
    }}
    painter.add(egui::Shape::mesh(mesh));
}

fn warp_surface_point(point: Pos2, warp: SurfaceWarp) -> Pos2 {
    let local = rotate_point(point, warp.source.center(), -warp.rotation);
    let u = ((local.x-warp.source.left())/warp.source.width().max(0.001)).clamp(0.0,1.0);
    let v = ((local.y-warp.source.top())/warp.source.height().max(0.001)).clamp(0.0,1.0);
    let top = warp.quad[0]+(warp.quad[1]-warp.quad[0])*u;
    let bottom = warp.quad[3]+(warp.quad[2]-warp.quad[3])*u;
    top+(bottom-top)*v
}

fn inverse_surface_point(point: Pos2, warp: SurfaceWarp) -> Pos2 {
    let mut u=0.5f32; let mut v=0.5f32;
    for _ in 0..6 {
        let top=warp.quad[0]+(warp.quad[1]-warp.quad[0])*u;
        let bottom=warp.quad[3]+(warp.quad[2]-warp.quad[3])*u;
        let current=top+(bottom-top)*v;
        let du=(warp.quad[1]-warp.quad[0])*(1.0-v)+(warp.quad[2]-warp.quad[3])*v;
        let dv=bottom-top;
        let error=current-point;
        let determinant=du.x*dv.y-du.y*dv.x;
        if determinant.abs()<1e-5 { break; }
        u-=(error.x*dv.y-error.y*dv.x)/determinant;
        v-=(du.x*error.y-du.y*error.x)/determinant;
    }
    let local=Pos2::new(warp.source.left()+warp.source.width()*u,warp.source.top()+warp.source.height()*v);
    rotate_point(local,warp.source.center(),warp.rotation)
}

fn rotated_bounds(rect: Rect, rotation: f32) -> Rect {
    if rotation.abs() < 0.001 { return rect; }
    let radians = rotation.to_radians();
    let points = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()]
        .map(|point| rotate_point(point, rect.center(), radians));
    let mut bounds = Rect::NOTHING;
    for point in points { bounds.extend_with(point); }
    bounds
}

fn decode_rich_entities(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"")
        .replace("&apos;", "'").replace("&amp;", "&")
}

fn parse_rich_color(value: &str, alpha: u8) -> Option<Color32> {
    let value = value.trim();
    let hex = value.trim_start_matches('#');
    if hex.len() == 6 {
        let rgb = u32::from_str_radix(hex, 16).ok()?;
        return Some(Color32::from_rgba_unmultiplied((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha));
    }
    let body = value.strip_prefix("rgb(").and_then(|value| value.strip_suffix(')'))?;
    let values: Vec<u8> = body.split(',').filter_map(|part| part.trim().parse().ok()).collect();
    (values.len() == 3).then(|| Color32::from_rgba_unmultiplied(values[0], values[1], values[2], alpha))
}

fn rich_attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let start = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[start..];
    if let Some(quote) = rest.chars().next().filter(|value| *value == '\"' || *value == '\'') {
        let value = &rest[quote.len_utf8()..];
        Some(value.split(quote).next().unwrap_or_default().to_string())
    } else {
        Some(rest.split_whitespace().next().unwrap_or_default().trim_end_matches('/').to_string())
    }
}

fn rich_layout_job(node: &GuiNode, font_size: f32, base_color: Color32,
                   wrap_width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let mut stack = vec![egui::TextFormat {
        font_id: FontId::new(font_size, roblox_font_family(node.font_monospace,node.font_extended,node.font_source_sans,node.font_montserrat,node.font_arimo,node.font_italic,node.font_weight)),
        line_height: Some(font_size * node.line_height),
        color: base_color,
        italics: node.font_italic && !node.font_source_sans,
        extra_letter_spacing: 0.0,
        ..Default::default()
    }];
    let text = node.text.as_str();
    let mut cursor = 0;
    while cursor < text.len() {
        let Some(relative) = text[cursor..].find('<') else {
            job.append(&decode_rich_entities(&text[cursor..]), 0.0, stack.last().unwrap().clone());
            break;
        };
        let open = cursor + relative;
        if open > cursor {
            job.append(&decode_rich_entities(&text[cursor..open]), 0.0, stack.last().unwrap().clone());
        }
        let Some(close_relative) = text[open..].find('>') else {
            job.append(&decode_rich_entities(&text[open..]), 0.0, stack.last().unwrap().clone());
            break;
        };
        let close = open + close_relative;
        let raw_tag = text[open + 1..close].trim();
        let tag = raw_tag.to_ascii_lowercase();
        if tag.starts_with('/') {
            if stack.len() > 1 { stack.pop(); }
        } else if tag == "br" || tag == "br/" {
            job.append("\n", 0.0, stack.last().unwrap().clone());
        } else {
            let mut format = stack.last().unwrap().clone();
            if tag == "i" { format.italics = true; }
            if tag == "u" { format.underline = Stroke::new(1.0, format.color); }
            if tag == "s" || tag == "strike" { format.strikethrough = Stroke::new(1.0, format.color); }
            if tag == "b" { format.font_id.family=roblox_font_family(node.font_monospace,node.font_extended,node.font_source_sans,node.font_montserrat,node.font_arimo,node.font_italic,700); }
            if tag.starts_with("font") {
                if let Some(value) = rich_attribute(raw_tag, "size").and_then(|value| value.parse::<f32>().ok()) {
                    let scaled = (value * font_size / node.text_size.max(1.0)).max(1.0);
                    format.font_id.size = scaled;
                    format.line_height = Some(scaled * node.line_height);
                }
                if let Some(color) = rich_attribute(raw_tag, "color")
                    .and_then(|value| parse_rich_color(&value, base_color.a()))
                {
                    format.color = color;
                }
                if let Some(transparency) = rich_attribute(raw_tag, "transparency").and_then(|value| value.parse::<f32>().ok()) {
                    format.color = Color32::from_rgba_unmultiplied(format.color.r(), format.color.g(), format.color.b(),
                        (format.color.a() as f32 * (1.0-transparency.clamp(0.0,1.0))) as u8);
                }
                if rich_attribute(raw_tag, "face").map(|face| face.to_ascii_lowercase().contains("code")).unwrap_or(false) {
                    format.font_id.family = roblox_font_family(true,false,false,false,false,false,node.font_weight);
                }
            }
            stack.push(format);
        }
        cursor = close + 1;
    }
    job
}

fn layout_text(painter: &egui::Painter, node: &GuiNode, font_size: f32,
               color: Color32, wrap_width: f32, available_height: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = if node.rich_text {
        rich_layout_job(node, font_size, color, wrap_width)
    } else {
        let family = roblox_font_family(node.font_monospace,node.font_extended,node.font_source_sans,node.font_montserrat,node.font_arimo,node.font_italic,node.font_weight);
        let mut job = egui::text::LayoutJob::simple(
            node.text.clone(), FontId::new(font_size, family), color, wrap_width,
        );
        if let Some(section) = job.sections.first_mut() {
            section.format.line_height = Some(font_size * node.line_height);
            section.format.italics = node.font_italic && !node.font_source_sans;
            section.format.extra_letter_spacing = 0.0;
        }
        job
    };
    if node.text_truncate != 0 {
        job.wrap.max_width = wrap_width;
        job.wrap.max_rows = ((available_height / (font_size * node.line_height).max(1.0)).floor() as usize).max(1);
        job.wrap.break_anywhere = node.text_truncate == 1;
        job.wrap.overflow_character = Some('…');
    }
    painter.layout_job(job)
}

fn mask_galley(mut galley: std::sync::Arc<egui::Galley>, origin: Pos2,
               gradient: Option<&GuiGradient>, group_gradients:&[(GuiGradient,Rect)], rounded_clips: &[RoundedMask],
               rect: Rect, gradient_rect:Rect, object_rotation: f32, surface_warp: Option<SurfaceWarp>) -> std::sync::Arc<egui::Galley> {
    let mutable = std::sync::Arc::make_mut(&mut galley);
    for placed_row in &mut mutable.rows {
        let row_origin = origin + placed_row.pos.to_vec2();
        let row = std::sync::Arc::make_mut(&mut placed_row.row);
        for vertex in &mut row.visuals.mesh.vertices {
            let point=row_origin+vertex.pos.to_vec2();
            vertex.color=apply_gradients(vertex.color,point,gradient,gradient_rect,group_gradients);
            let mut screen_point=if object_rotation.abs()>=0.001 { rotate_point(point,rect.center(),object_rotation.to_radians()) } else { point };
            if let Some(warp)=surface_warp {
                screen_point=warp_surface_point(screen_point,warp);
                vertex.pos=screen_point-origin.to_vec2()-placed_row.pos.to_vec2();
            }
            for mask in rounded_clips {
                let local=rotate_point(screen_point,mask.rect.center(),-mask.rotation.to_radians());
                let coverage=rounded_coverage(local,mask.rect,mask.radius);
                vertex.color=Color32::from_rgba_unmultiplied(vertex.color.r(),vertex.color.g(),vertex.color.b(),(vertex.color.a() as f32*coverage) as u8);
            }
        }
    }
    galley
}

fn paint_galley(painter: &egui::Painter, pos: Pos2, galley: std::sync::Arc<egui::Galley>,
                color: Color32, override_color: bool, rotation: f32, pivot: Pos2) {
    let radians = rotation.to_radians();
    let rotated_pos = if rotation.abs() < 0.001 { pos } else { rotate_point(pos, pivot, radians) };
    let mut shape = egui::epaint::TextShape::new(rotated_pos, galley, color);
    if override_color { shape = shape.with_override_text_color(color); }
    if rotation.abs() >= 0.001 { shape = shape.with_angle(radians); }
    painter.add(shape);
}

fn rotate_point(point: Pos2, pivot: Pos2, radians: f32) -> Pos2 {
    let offset = point - pivot;
    let (sin, cos) = radians.sin_cos();
    pivot + Vec2::new(offset.x*cos - offset.y*sin, offset.x*sin + offset.y*cos)
}

fn apply_gradients(mut color:Color32,point:Pos2,gradient:Option<&GuiGradient>,gradient_rect:Rect,groups:&[(GuiGradient,Rect)])->Color32 {
    if let Some(gradient)=gradient{color=multiply_color(color,sample_gradient(gradient,point,gradient_rect));}
    for (gradient,rect) in groups{color=multiply_color(color,sample_gradient(gradient,point,*rect));}color
}

fn sample_gradient(gradient: &GuiGradient, point: Pos2, rect: Rect) -> Color32 {
    let samples=&gradient.colors;if samples.is_empty(){return Color32::WHITE;}
    let radians=gradient.rotation.to_radians();let direction=Vec2::new(radians.cos(),radians.sin());
    let center=rect.center()+Vec2::new(gradient.offset.x*rect.width(),gradient.offset.y*rect.height());
    let extent=direction.x.abs()*rect.width()*0.5+direction.y.abs()*rect.height()*0.5;
    let mut position=match gradient.kind {
        1=>(point-center).length()/(rect.width().hypot(rect.height())*0.5).max(0.001),
        2=>{let delta=point-center;((delta.y.atan2(delta.x)-radians)/std::f32::consts::TAU).rem_euclid(1.0)},
        _=>(point-center).dot(direction)/(extent*2.0).max(0.001)+0.5,
    };
    position=(position-0.5)/gradient.scale+0.5;
    position=match gradient.tile_mode {1=>position.rem_euclid(1.0),2=>{let value=position.rem_euclid(2.0);if value>1.0{2.0-value}else{value}},_=>position.clamp(0.0,1.0)};
    let scaled=position*(samples.len()-1) as f32;
    let lower = scaled.floor() as usize;
    let upper = (lower+1).min(samples.len()-1);
    let mix = scaled-lower as f32;
    let a = samples[lower]; let b = samples[upper];
    Color32::from_rgba_unmultiplied(
        (a.r() as f32+(b.r() as f32-a.r() as f32)*mix) as u8,
        (a.g() as f32+(b.g() as f32-a.g() as f32)*mix) as u8,
        (a.b() as f32+(b.b() as f32-a.b() as f32)*mix) as u8,
        (a.a() as f32+(b.a() as f32-a.a() as f32)*mix) as u8)
}

fn paint_masked_solid(painter: &egui::Painter, rect: Rect, color: Color32, radius: f32,
                      rotation: f32, rounded_clips: &[RoundedMask], surface_warp: Option<SurfaceWarp>) {
    let divisions=16usize; let mut mesh=egui::Mesh::default();
    for y in 0..=divisions { for x in 0..=divisions {
        let fx=x as f32/divisions as f32; let fy=y as f32/divisions as f32;
        let local=Pos2::new(rect.left()+rect.width()*fx,rect.top()+rect.height()*fy);
        let mut coverage=rounded_coverage(local,rect,radius);
        let mut pos=if rotation.abs()>=0.001 { rotate_point(local,rect.center(),rotation.to_radians()) } else { local };
        if let Some(warp)=surface_warp { pos=warp_surface_point(pos,warp); }
        for mask in rounded_clips {
            let mask_local=rotate_point(pos,mask.rect.center(),-mask.rotation.to_radians());
            coverage*=rounded_coverage(mask_local,mask.rect,mask.radius);
        }
        let vertex_color=Color32::from_rgba_unmultiplied(color.r(),color.g(),color.b(),(color.a() as f32*coverage) as u8);
        mesh.vertices.push(egui::epaint::Vertex { pos,uv:egui::epaint::WHITE_UV,color:vertex_color });
    }}
    let stride=divisions+1;
    for y in 0..divisions { for x in 0..divisions { let a=(y*stride+x) as u32; let b=a+1; let c=a+stride as u32; let d=c+1;
        mesh.add_triangle(a,b,d); mesh.add_triangle(a,d,c); }}
    painter.add(egui::Shape::mesh(mesh));
}

fn rounded_coverage(point: Pos2, rect: Rect, radius: f32) -> f32 {
    if radius <= 0.0 { return 1.0; }
    let radius = radius.min(rect.width().min(rect.height())*0.5);
    let inner = Rect::from_min_max(rect.min+Vec2::splat(radius), rect.max-Vec2::splat(radius));
    let nearest = Pos2::new(point.x.clamp(inner.left(),inner.right()), point.y.clamp(inner.top(),inner.bottom()));
    (radius-(point-nearest).length()+0.75).clamp(0.0,1.0)
}

fn paint_texture_quad(painter: &egui::Painter, texture: egui::TextureId, destination: Rect,
                      uv: Rect, tint: Color32, rotation: f32, pivot: Pos2,
                      gradient: Option<&GuiGradient>, gradient_rect: Rect, group_gradients:&[(GuiGradient,Rect)],
                      corner_radius: f32, rounded_clips: &[RoundedMask], surface_warp: Option<SurfaceWarp>) {
    if gradient.is_none() && group_gradients.is_empty() && rotation.abs() < 0.001 && corner_radius <= 0.0 && rounded_clips.is_empty() && surface_warp.is_none() {
        painter.image(texture, destination, uv, tint);
        return;
    }
    let divisions = if corner_radius > 0.0 || !rounded_clips.is_empty() { 16usize } else if gradient.is_some()||!group_gradients.is_empty() { 8usize } else { 1usize };
    let mut mesh = egui::Mesh::with_texture(texture);
    for y in 0..=divisions {
        for x in 0..=divisions {
            let fx=x as f32/divisions as f32; let fy=y as f32/divisions as f32;
            let mut pos=Pos2::new(destination.left()+destination.width()*fx, destination.top()+destination.height()*fy);
            let uv_pos=Pos2::new(uv.left()+uv.width()*fx, uv.top()+uv.height()*fy);
            let mut color=apply_gradients(tint,pos,gradient,gradient_rect,group_gradients);
            let mut coverage=rounded_coverage(pos,gradient_rect,corner_radius);
            if rotation.abs() >= 0.001 { pos=rotate_point(pos,pivot,rotation.to_radians()); }
            if let Some(warp)=surface_warp { pos=warp_surface_point(pos,warp); }
            for mask in rounded_clips {
                let local=rotate_point(pos,mask.rect.center(),-mask.rotation.to_radians());
                coverage*=rounded_coverage(local,mask.rect,mask.radius);
            }
            color=Color32::from_rgba_unmultiplied(color.r(),color.g(),color.b(),(color.a() as f32*coverage) as u8);
            mesh.vertices.push(egui::epaint::Vertex { pos, uv: uv_pos, color });
        }
    }
    let stride=divisions+1;
    for y in 0..divisions { for x in 0..divisions {
        let a=(y*stride+x) as u32; let b=a+1; let c=a+stride as u32; let d=c+1;
        mesh.add_triangle(a,b,d); mesh.add_triangle(a,d,c);
    }}
    painter.add(egui::Shape::mesh(mesh));
}

fn paint_image(painter: &egui::Painter, node: &GuiNode, texture: &egui::TextureHandle, tint: Color32) {
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
                paint_texture_quad(painter, texture.id(), bounds, uv, tint, node.rotation, node.rect.center(), node.gradient.as_ref(), node.gradient_rect, &node.group_gradients, node.corner_radius, &node.rounded_clips, node.surface_warp);
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
                    paint_texture_quad(painter, texture.id(),
                        Rect::from_min_max(Pos2::new(dx[x], dy[y]), Pos2::new(dx[x + 1], dy[y + 1])),
                        Rect::from_min_max(Pos2::new(ux[x], uy[y]), Pos2::new(ux[x + 1], uy[y + 1])),
                        tint, node.rotation, node.rect.center(), node.gradient.as_ref(), node.gradient_rect, &node.group_gradients, node.corner_radius, &node.rounded_clips, node.surface_warp);
                }
            }
        }
        // Tile
        2 => {
            let (xs, xo, ys, yo) = node.tile_size.unwrap_or((0.0, 100.0, 0.0, 100.0));
            let tile = Vec2::new(
                (bounds.width() * xs + xo * node.gui_scale).max(1.0),
                (bounds.height() * ys + yo * node.gui_scale).max(1.0),
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
                    paint_texture_quad(painter, texture.id(), Rect::from_min_max(min, max), tile_uv,
                        tint, node.rotation, node.rect.center(), node.gradient.as_ref(), node.gradient_rect, &node.group_gradients, node.corner_radius, &node.rounded_clips, node.surface_warp);
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
            paint_texture_quad(painter, texture.id(), Rect::from_center_size(bounds.center(), size), uv,
                tint, node.rotation, node.rect.center(), node.gradient.as_ref(), node.gradient_rect, &node.group_gradients, node.corner_radius, &node.rounded_clips, node.surface_warp);
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
            paint_texture_quad(painter, texture.id(), bounds, crop_uv, tint, node.rotation, node.rect.center(), node.gradient.as_ref(), node.gradient_rect, &node.group_gradients, node.corner_radius, &node.rounded_clips, node.surface_warp);
        }
        // Stretch
        _ => paint_texture_quad(painter, texture.id(), bounds, uv, tint, node.rotation, node.rect.center(), node.gradient.as_ref(), node.gradient_rect, &node.group_gradients, node.corner_radius, &node.rounded_clips, node.surface_warp),
    }
}

struct ViewportBox { vertices: Vec<[f32; 3]>, faces: Vec<Vec<usize>>, color: Color32, referent: Ref }
struct ViewportMesh {
    vertices: Vec<[f32;3]>, faces: Vec<[u32;3]>, colors: Vec<[f32;4]>, uvs: Vec<[f32;2]>,
    color: Color32, texture: Option<String>, referent: Ref,
}
struct ViewportPolygon {
    depth: f32, points: Vec<Pos2>, depths: Vec<f32>, color: Color32,
    texture: Option<(String, Vec<Pos2>)>, referent: Ref,
}

fn subdivide_viewport_triangle(points:[Pos2;3],depths:[f32;3],uvs:Option<[Pos2;3]>,level:usize,
                               uri:Option<&str>,color:Color32,referent:Ref,output:&mut Vec<ViewportPolygon>){
    if level==0 {
        output.push(ViewportPolygon{depth:(depths[0]+depths[1]+depths[2])/3.0,points:points.to_vec(),depths:depths.to_vec(),color,
            texture:uri.map(|uri|(uri.to_string(),uvs.unwrap_or([Pos2::ZERO;3]).to_vec())),referent});return;
    }
    let midpoint=|a:Pos2,b:Pos2|Pos2::new((a.x+b.x)*0.5,(a.y+b.y)*0.5);
    let p01=midpoint(points[0],points[1]);let p12=midpoint(points[1],points[2]);let p20=midpoint(points[2],points[0]);
    let harmonic=|a:f32,b:f32|2.0/(1.0/a.max(0.0001)+1.0/b.max(0.0001));
    let d01=harmonic(depths[0],depths[1]);let d12=harmonic(depths[1],depths[2]);let d20=harmonic(depths[2],depths[0]);
    let split_uv=uvs.map(|uv|{let perspective=|a:Pos2,b:Pos2,da:f32,db:f32|{let wa=1.0/da.max(0.0001);let wb=1.0/db.max(0.0001);Pos2::new((a.x*wa+b.x*wb)/(wa+wb),(a.y*wa+b.y*wb)/(wa+wb))};let u01=perspective(uv[0],uv[1],depths[0],depths[1]);let u12=perspective(uv[1],uv[2],depths[1],depths[2]);let u20=perspective(uv[2],uv[0],depths[2],depths[0]);[[uv[0],u01,u20],[u01,uv[1],u12],[u20,u12,uv[2]],[u01,u12,u20]]});
    for(index,(next_points,next_depths))in [([points[0],p01,p20],[depths[0],d01,d20]),([p01,points[1],p12],[d01,depths[1],d12]),([p20,p12,points[2]],[d20,d12,depths[2]]),([p01,p12,p20],[d01,d12,d20])].into_iter().enumerate(){
        subdivide_viewport_triangle(next_points,next_depths,split_uv.map(|value|value[index]),level-1,uri,color,referent,output);
    }
}
#[derive(Clone, Copy)]
struct ViewportClipVertex { position: [f32;3], uv: [f32;2] }

fn clip_viewport_near(vertices: Vec<ViewportClipVertex>, camera: [f32;3], forward: [f32;3]) -> Vec<ViewportClipVertex> {
    let depth=|point:[f32;3]|(point[0]-camera[0])*forward[0]+(point[1]-camera[1])*forward[1]+(point[2]-camera[2])*forward[2];
    let near=0.01; let mut output=Vec::new();
    for index in 0..vertices.len() {
        let current=vertices[index]; let previous=vertices[(index+vertices.len()-1)%vertices.len()];
        let current_depth=depth(current.position); let previous_depth=depth(previous.position);
        let current_inside=current_depth>=near; let previous_inside=previous_depth>=near;
        if current_inside != previous_inside {
            let amount=((near-previous_depth)/(current_depth-previous_depth)).clamp(0.0,1.0);
            output.push(ViewportClipVertex { position:[
                previous.position[0]+(current.position[0]-previous.position[0])*amount,
                previous.position[1]+(current.position[1]-previous.position[1])*amount,
                previous.position[2]+(current.position[2]-previous.position[2])*amount,
            ], uv:[previous.uv[0]+(current.uv[0]-previous.uv[0])*amount,previous.uv[1]+(current.uv[1]-previous.uv[1])*amount] });
        }
        if current_inside { output.push(current); }
    }
    output
}

fn gather_viewport_parts(dom: &WeakDom, referent: Ref, output: &mut Vec<ViewportBox>, meshes: &mut Vec<ViewportMesh>) {
    let Some(instance) = dom.get_by_ref(referent) else { return; };
    if let (Some(Variant::CFrame(cf)), Some(Variant::Vector3(size))) = (
        instance.properties.get(&rbx_dom_weak::ustr("CFrame")),
        instance.properties.get(&rbx_dom_weak::ustr("Size")),
    ) {
        let mut corners = [[0.0; 3]; 8];
        for (index, corner) in corners.iter_mut().enumerate() {
            let local = [
                if index & 1 == 0 { -size.x * 0.5 } else { size.x * 0.5 },
                if index & 2 == 0 { -size.y * 0.5 } else { size.y * 0.5 },
                if index & 4 == 0 { -size.z * 0.5 } else { size.z * 0.5 },
            ];
            *corner = [
                cf.position.x + cf.orientation.x.x*local[0] + cf.orientation.x.y*local[1] + cf.orientation.x.z*local[2],
                cf.position.y + cf.orientation.y.x*local[0] + cf.orientation.y.y*local[1] + cf.orientation.y.z*local[2],
                cf.position.z + cf.orientation.z.x*local[0] + cf.orientation.z.y*local[1] + cf.orientation.z.z*local[2],
            ];
        }
        let part_alpha=((1.0-number(instance.properties.get(&rbx_dom_weak::ustr("Transparency")),0.0).clamp(0.0,1.0))*255.0) as u8;
        let part_color=color(instance.properties.get(&rbx_dom_weak::ustr("Color")),part_alpha,[163,162,165]);
        let mut mesh_id = if instance.class == "MeshPart" {
            content(instance.properties.get(&rbx_dom_weak::ustr("MeshId")))
                .or_else(||content(instance.properties.get(&rbx_dom_weak::ustr("MeshContent"))))
        } else { None };
        let mut texture = if instance.class == "MeshPart" {
            content(instance.properties.get(&rbx_dom_weak::ustr("TextureID")))
                .or_else(||content(instance.properties.get(&rbx_dom_weak::ustr("TextureId"))))
                .or_else(||content(instance.properties.get(&rbx_dom_weak::ustr("TextureContent"))))
        } else { None };
        let mut mesh_scale = [1.0,1.0,1.0];
        let mut mesh_offset = [0.0,0.0,0.0];
        let mut mesh_type = match instance.properties.get(&rbx_dom_weak::ustr("Shape")) {
            Some(Variant::Enum(value))=>match value.clone().to_u32(){0=>Some(3),2=>Some(4),3=>Some(2),_=>None},
            Some(Variant::String(value))=>match value.as_str(){"Ball"=>Some(3),"Cylinder"=>Some(4),"Wedge"=>Some(2),_=>None},
            _=>None,
        };
        for child in instance.children().filter_map(|child|dom.get_by_ref(*child)) {
            if child.class == "SurfaceAppearance" {
                texture=content(child.properties.get(&rbx_dom_weak::ustr("ColorMap")))
                    .or_else(||content(child.properties.get(&rbx_dom_weak::ustr("ColorMapContent")))).or(texture);
                continue;
            }
            if instance.class != "MeshPart" && (child.class == "SpecialMesh" || child.class == "BlockMesh") {
                mesh_id=content(child.properties.get(&rbx_dom_weak::ustr("MeshId"))).or(mesh_id);
                texture=content(child.properties.get(&rbx_dom_weak::ustr("TextureId"))).or(texture);
                if let Some(Variant::Vector3(value))=child.properties.get(&rbx_dom_weak::ustr("Scale")){mesh_scale=[value.x,value.y,value.z];}
                if let Some(Variant::Vector3(value))=child.properties.get(&rbx_dom_weak::ustr("Offset")){mesh_offset=[value.x,value.y,value.z];}
                mesh_type=match child.properties.get(&rbx_dom_weak::ustr("MeshType")) {
                    Some(Variant::Enum(value))=>Some(value.clone().to_u32()),
                    Some(Variant::String(value))=>Some(match value.as_str(){"Sphere"=>3,"Cylinder"=>4,"Wedge"=>2,"FileMesh"=>5,"Brick"=>6,_=>0}),
                    _=>mesh_type,
                };
            }
        }
        let mesh=mesh_id.and_then(|id|crate::asset_downloader::get_cached_mesh(&id));
        if let Some(mesh)=mesh {
            let range=[(mesh.aabb_max[0]-mesh.aabb_min[0]).max(0.001),(mesh.aabb_max[1]-mesh.aabb_min[1]).max(0.001),(mesh.aabb_max[2]-mesh.aabb_min[2]).max(0.001)];
            let midpoint=[(mesh.aabb_max[0]+mesh.aabb_min[0])*0.5,(mesh.aabb_max[1]+mesh.aabb_min[1])*0.5,(mesh.aabb_max[2]+mesh.aabb_min[2])*0.5];
            let vertices=mesh.vertices.iter().map(|vertex| {
                let local=[(vertex[0]-midpoint[0])*size.x*mesh_scale[0]/range[0]+mesh_offset[0],(vertex[1]-midpoint[1])*size.y*mesh_scale[1]/range[1]+mesh_offset[1],(vertex[2]-midpoint[2])*size.z*mesh_scale[2]/range[2]+mesh_offset[2]];
                [cf.position.x+cf.orientation.x.x*local[0]+cf.orientation.x.y*local[1]+cf.orientation.x.z*local[2],cf.position.y+cf.orientation.y.x*local[0]+cf.orientation.y.y*local[1]+cf.orientation.y.z*local[2],cf.position.z+cf.orientation.z.x*local[0]+cf.orientation.z.y*local[1]+cf.orientation.z.z*local[2]]
            }).collect();
            meshes.push(ViewportMesh{vertices,faces:mesh.faces,colors:mesh.colors,uvs:mesh.uvs,color:part_color,texture,referent});
        } else {
            let mut primitive_corners=corners;
            if mesh_type.is_some() {
                for (index,corner) in primitive_corners.iter_mut().enumerate(){let local=[if index&1==0{-size.x*mesh_scale[0]*0.5}else{size.x*mesh_scale[0]*0.5},if index&2==0{-size.y*mesh_scale[1]*0.5}else{size.y*mesh_scale[1]*0.5},if index&4==0{-size.z*mesh_scale[2]*0.5}else{size.z*mesh_scale[2]*0.5}];let local=[local[0]+mesh_offset[0],local[1]+mesh_offset[1],local[2]+mesh_offset[2]];*corner=[cf.position.x+cf.orientation.x.x*local[0]+cf.orientation.x.y*local[1]+cf.orientation.x.z*local[2],cf.position.y+cf.orientation.y.x*local[0]+cf.orientation.y.y*local[1]+cf.orientation.y.z*local[2],cf.position.z+cf.orientation.z.x*local[0]+cf.orientation.z.y*local[1]+cf.orientation.z.z*local[2]];}
            }
            let (vertices,faces)=if mesh_type==Some(3) || mesh_type==Some(4) {
                let segments=16usize; let rings=if mesh_type==Some(3){8}else{1};
                let mut local_vertices=Vec::new();
                if mesh_type==Some(3) {
                    for ring in 0..=rings { let latitude=std::f32::consts::PI*(ring as f32/rings as f32-0.5); for segment in 0..segments { let longitude=std::f32::consts::TAU*segment as f32/segments as f32; local_vertices.push([latitude.cos()*longitude.cos()*size.x*mesh_scale[0]*0.5+mesh_offset[0],latitude.sin()*size.y*mesh_scale[1]*0.5+mesh_offset[1],latitude.cos()*longitude.sin()*size.z*mesh_scale[2]*0.5+mesh_offset[2]]); } }
                } else {
                    for side in [-1.0,1.0] { for segment in 0..segments { let angle=std::f32::consts::TAU*segment as f32/segments as f32; local_vertices.push([side*size.x*mesh_scale[0]*0.5+mesh_offset[0],angle.cos()*size.y*mesh_scale[1]*0.5+mesh_offset[1],angle.sin()*size.z*mesh_scale[2]*0.5+mesh_offset[2]]); } }
                }
                let vertices=local_vertices.into_iter().map(|local|[cf.position.x+cf.orientation.x.x*local[0]+cf.orientation.x.y*local[1]+cf.orientation.x.z*local[2],cf.position.y+cf.orientation.y.x*local[0]+cf.orientation.y.y*local[1]+cf.orientation.y.z*local[2],cf.position.z+cf.orientation.z.x*local[0]+cf.orientation.z.y*local[1]+cf.orientation.z.z*local[2]]).collect();
                let mut faces=Vec::new();
                if mesh_type==Some(3) { for ring in 0..rings { for segment in 0..segments { let next=(segment+1)%segments; faces.push(vec![ring*segments+segment,ring*segments+next,(ring+1)*segments+next,(ring+1)*segments+segment]); } } }
                else { for segment in 0..segments { let next=(segment+1)%segments; faces.push(vec![segment,next,segments+next,segments+segment]); } faces.push((0..segments).rev().collect()); faces.push((segments..segments*2).collect()); }
                (vertices,faces)
            } else if instance.class == "WedgePart" || mesh_type==Some(2) {
                (vec![primitive_corners[0],primitive_corners[1],primitive_corners[4],primitive_corners[5],primitive_corners[6],primitive_corners[7]],vec![
                    vec![0,1,3,2],vec![2,3,5,4],vec![0,2,4],vec![1,5,3],vec![0,4,5,1]])
            } else if instance.class == "CornerWedgePart" {
                (vec![primitive_corners[0],primitive_corners[1],primitive_corners[4],primitive_corners[5],primitive_corners[7]],vec![
                    vec![0,1,3,2],vec![1,4,3],vec![2,3,4],vec![0,2,4],vec![0,4,1]])
            } else {
                (primitive_corners.to_vec(),vec![vec![0,1,3,2],vec![4,6,7,5],vec![0,4,5,1],vec![2,3,7,6],vec![0,2,6,4],vec![1,5,7,3]])
            };
            output.push(ViewportBox { vertices, faces, color: part_color, referent });
        }
    }
    for child in instance.children() { gather_viewport_parts(dom, *child, output, meshes); }
}

fn point_in_polygon(point: Pos2, polygon: &[Pos2]) -> bool {
    let mut inside=false;
    for index in 0..polygon.len() { let previous=polygon[(index+polygon.len()-1)%polygon.len()]; let current=polygon[index];
        if (current.y>point.y)!=(previous.y>point.y) && point.x<(previous.x-current.x)*(point.y-current.y)/(previous.y-current.y)+current.x { inside=!inside; }
    }
    inside
}

fn paint_viewport_frame(painter: &egui::Painter, ui: &egui::Ui, node: &GuiNode, dom: &WeakDom,
                        textures: &mut std::collections::HashMap<String, egui::TextureHandle>) -> Option<Ref> {
    let Some(frame) = dom.get_by_ref(node.referent) else { return None; };
    let ambient = color(frame.properties.get(&rbx_dom_weak::ustr("Ambient")), 255, [200,200,200]);
    let image_tint = color(frame.properties.get(&rbx_dom_weak::ustr("ImageColor3")), 255, [255,255,255]);
    let image_alpha = ((1.0-number(frame.properties.get(&rbx_dom_weak::ustr("ImageTransparency")),0.0).clamp(0.0,1.0))*255.0) as u8;
    let light_color = color(frame.properties.get(&rbx_dom_weak::ustr("LightColor")), 255, [255,255,255]);
    let light_direction = match frame.properties.get(&rbx_dom_weak::ustr("LightDirection")) {
        Some(Variant::Vector3(value)) => [value.x,value.y,value.z], _ => [-1.0,-1.0,-1.0],
    };
    let light_length=(light_direction[0]*light_direction[0]+light_direction[1]*light_direction[1]+light_direction[2]*light_direction[2]).sqrt().max(0.001);
    let light=[-light_direction[0]/light_length,-light_direction[1]/light_length,-light_direction[2]/light_length];
    let mut parts = Vec::new();
    let mut meshes = Vec::new();
    gather_viewport_parts(dom, node.referent, &mut parts, &mut meshes);
    if parts.is_empty() && meshes.is_empty() { return None; }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for part in &parts { for vertex in &part.vertices { for axis in 0..3 { min[axis]=min[axis].min(vertex[axis]); max[axis]=max[axis].max(vertex[axis]); } } }
    for mesh in &meshes { for vertex in &mesh.vertices { for axis in 0..3 { min[axis]=min[axis].min(vertex[axis]); max[axis]=max[axis].max(vertex[axis]); } } }
    let center = [(min[0]+max[0])*0.5, (min[1]+max[1])*0.5, (min[2]+max[2])*0.5];
    let extent = (max[0]-min[0]).max(max[1]-min[1]).max(max[2]-min[2]).max(1.0);
    let supplied_camera = dom.get_by_ref(node.referent).and_then(|frame| match frame.properties.get(&rbx_dom_weak::ustr("CurrentCamera")) {
        Some(Variant::Ref(referent)) => dom.get_by_ref(*referent), _ => None,
    }).and_then(|camera| match camera.properties.get(&rbx_dom_weak::ustr("CFrame")) {
        Some(Variant::CFrame(cf)) => Some((
            [cf.position.x,cf.position.y,cf.position.z],
            [-cf.orientation.x.z,-cf.orientation.y.z,-cf.orientation.z.z],
            [cf.orientation.x.x,cf.orientation.y.x,cf.orientation.z.x],
            [cf.orientation.x.y,cf.orientation.y.y,cf.orientation.z.y],
            number(camera.properties.get(&rbx_dom_weak::ustr("FieldOfView")), 70.0).clamp(1.0, 120.0),
        )), _ => None,
    });
    let (camera,forward,right,up,field_of_view) = supplied_camera.unwrap_or_else(|| {
        let camera=[center[0]+extent*1.5,center[1]+extent*1.2,center[2]+extent*1.5];
        let forward={let v=[center[0]-camera[0],center[1]-camera[1],center[2]-camera[2]];let l=(v[0]*v[0]+v[1]*v[1]+v[2]*v[2]).sqrt();[v[0]/l,v[1]/l,v[2]/l]};
        let right={let v=[forward[2],0.0,-forward[0]];let l=(v[0]*v[0]+v[2]*v[2]).sqrt().max(0.001);[v[0]/l,0.0,v[2]/l]};
        let up=[right[1]*forward[2]-right[2]*forward[1],right[2]*forward[0]-right[0]*forward[2],right[0]*forward[1]-right[1]*forward[0]];
        (camera,forward,right,up,70.0)
    });
    let focal_length = node.content_rect.height()*0.5/(field_of_view.to_radians()*0.5).tan();
    let project = |point: [f32;3]| -> Option<(Pos2,f32)> {
        let relative=[point[0]-camera[0],point[1]-camera[1],point[2]-camera[2]];
        let depth=relative[0]*forward[0]+relative[1]*forward[1]+relative[2]*forward[2];
        if depth < 0.01 { return None; }
        let x=(relative[0]*right[0]+relative[1]*right[1]+relative[2]*right[2])/depth;
        let y=(relative[0]*up[0]+relative[1]*up[1]+relative[2]*up[2])/depth;
        Some((node.content_rect.center()+Vec2::new(x,-y)*focal_length, depth))
    };
    let mut polygons = Vec::new();
    for part in parts {
        let part_center={let mut value=[0.0;3];for vertex in &part.vertices{for axis in 0..3{value[axis]+=vertex[axis]/part.vertices.len() as f32;}}value};
        for face in part.faces {
            if face.len()<3 {continue;}
            let a=part.vertices[face[0]];let b=part.vertices[face[1]];let c=part.vertices[face[2]];
            let ab=[b[0]-a[0],b[1]-a[1],b[2]-a[2]];let ac=[c[0]-a[0],c[1]-a[1],c[2]-a[2]];
            let mut normal=[ab[1]*ac[2]-ab[2]*ac[1],ab[2]*ac[0]-ab[0]*ac[2],ab[0]*ac[1]-ab[1]*ac[0]];
            let face_center={let mut value=[0.0;3];for index in &face{for axis in 0..3{value[axis]+=part.vertices[*index][axis]/face.len() as f32;}}value};
            if normal[0]*(face_center[0]-part_center[0])+normal[1]*(face_center[1]-part_center[1])+normal[2]*(face_center[2]-part_center[2])<0.0{normal=[-normal[0],-normal[1],-normal[2]];}
            let normal_length=(normal[0]*normal[0]+normal[1]*normal[1]+normal[2]*normal[2]).sqrt().max(0.001);let normal=[normal[0]/normal_length,normal[1]/normal_length,normal[2]/normal_length];
            let to_camera=[camera[0]-a[0],camera[1]-a[1],camera[2]-a[2]];
            if normal[0]*to_camera[0]+normal[1]*to_camera[1]+normal[2]*to_camera[2]<=0.0{continue;}
            let illumination=(normal[0]*light[0]+normal[1]*light[1]+normal[2]*light[2]).max(0.0);
            let lit=Color32::from_rgba_unmultiplied(
                ((part.color.r() as f32)*(ambient.r() as f32+light_color.r() as f32*illumination).min(255.0)*image_tint.r() as f32/(255.0*255.0)) as u8,
                ((part.color.g() as f32)*(ambient.g() as f32+light_color.g() as f32*illumination).min(255.0)*image_tint.g() as f32/(255.0*255.0)) as u8,
                ((part.color.b() as f32)*(ambient.b() as f32+light_color.b() as f32*illumination).min(255.0)*image_tint.b() as f32/(255.0*255.0)) as u8,
                ((part.color.a() as u16*image_alpha as u16)/255) as u8);
            let clipped=clip_viewport_near(face.iter().map(|index|ViewportClipVertex{position:part.vertices[*index],uv:[0.0,0.0]}).collect(),camera,forward);
            let projected:Vec<(Pos2,f32)>=clipped.iter().filter_map(|vertex|project(vertex.position)).collect();
            if projected.len()<3{continue;}
            let depth=projected.iter().map(|value|value.1).sum::<f32>()/projected.len() as f32;
            polygons.push(ViewportPolygon{depth,points:projected.iter().map(|value|value.0).collect(),depths:projected.iter().map(|value|value.1).collect(),color:lit,texture:None,referent:part.referent});
        }
    }
    for mesh in meshes {
        for face in mesh.faces {
            let indices=[face[0] as usize,face[1] as usize,face[2] as usize];
            if indices.iter().any(|index|*index>=mesh.vertices.len()){continue;}
            let [a,b,c]=indices.map(|index|mesh.vertices[index]);
            let ab=[b[0]-a[0],b[1]-a[1],b[2]-a[2]];let ac=[c[0]-a[0],c[1]-a[1],c[2]-a[2]];
            let normal=[ab[1]*ac[2]-ab[2]*ac[1],ab[2]*ac[0]-ab[0]*ac[2],ab[0]*ac[1]-ab[1]*ac[0]];
            let length=(normal[0]*normal[0]+normal[1]*normal[1]+normal[2]*normal[2]).sqrt().max(0.001);let normal=[normal[0]/length,normal[1]/length,normal[2]/length];
            let view=[camera[0]-a[0],camera[1]-a[1],camera[2]-a[2]];if normal[0]*view[0]+normal[1]*view[1]+normal[2]*view[2]<=0.0{continue;}
            let illumination=(normal[0]*light[0]+normal[1]*light[1]+normal[2]*light[2]).max(0.0);
            let vertex_color=if indices.iter().all(|index|*index<mesh.colors.len()) {
                let mut average=[0.0;4]; for index in indices { for channel in 0..4 { average[channel]+=mesh.colors[index][channel]/3.0; } } average
            } else {[1.0;4]};
            let lit=Color32::from_rgba_unmultiplied(
                ((mesh.color.r() as f32)*(ambient.r() as f32+light_color.r() as f32*illumination).min(255.0)*image_tint.r() as f32*vertex_color[0]/(255.0*255.0)) as u8,
                ((mesh.color.g() as f32)*(ambient.g() as f32+light_color.g() as f32*illumination).min(255.0)*image_tint.g() as f32*vertex_color[1]/(255.0*255.0)) as u8,
                ((mesh.color.b() as f32)*(ambient.b() as f32+light_color.b() as f32*illumination).min(255.0)*image_tint.b() as f32*vertex_color[2]/(255.0*255.0)) as u8,
                ((mesh.color.a() as f32*image_alpha as f32*vertex_color[3])/255.0) as u8);
            let source_uvs=indices.map(|index|mesh.uvs.get(index).copied().unwrap_or([0.0,0.0]));
            let clipped=clip_viewport_near(vec![ViewportClipVertex{position:a,uv:source_uvs[0]},ViewportClipVertex{position:b,uv:source_uvs[1]},ViewportClipVertex{position:c,uv:source_uvs[2]}],camera,forward);
            let projected:Vec<(Pos2,f32)>=clipped.iter().filter_map(|vertex|project(vertex.position)).collect();
            if projected.len()<3{continue;}
            let texture=mesh.texture.as_ref().map(|uri|(uri.clone(),clipped.iter().map(|vertex|Pos2::new(vertex.uv[0],vertex.uv[1])).collect()));
            let depth=projected.iter().map(|value|value.1).sum::<f32>()/projected.len() as f32;
            polygons.push(ViewportPolygon{depth,points:projected.iter().map(|value|value.0).collect(),depths:projected.iter().map(|value|value.1).collect(),color:lit,texture,referent:mesh.referent});
        }
    }
    // Painter-ordering whole faces fails when large triangles overlap in depth.
    // Subdivide projected faces and sort the smaller cells independently; this
    // approximates a depth buffer while retaining egui texture meshes.
    let mut depth_cells=Vec::new();
    for polygon in polygons {
        for index in 1..polygon.points.len().saturating_sub(1) {
            let points=[polygon.points[0],polygon.points[index],polygon.points[index+1]];
            let depths=[polygon.depths[0],polygon.depths[index],polygon.depths[index+1]];
            let area=((points[1].x-points[0].x)*(points[2].y-points[0].y)-(points[1].y-points[0].y)*(points[2].x-points[0].x)).abs()*0.5;
            let level=if area>20_000.0{3}else if area>2_000.0{2}else if area>200.0{1}else{0};
            let (uri,uvs)=polygon.texture.as_ref().map(|(uri,uvs)|(Some(uri.as_str()),Some([uvs[0],uvs[index],uvs[index+1]]))).unwrap_or((None,None));
            subdivide_viewport_triangle(points,depths,uvs,level,uri,polygon.color,polygon.referent,&mut depth_cells);
        }
    }
    depth_cells.sort_by(|a,b| b.depth.total_cmp(&a.depth));
    let pointer=ui.input(|input|input.pointer.hover_pos()); let mut hovered=None;
    for mut polygon in depth_cells {
        let mut vertex_colors=Vec::with_capacity(polygon.points.len());
        for point in &mut polygon.points {
            let local=*point;let mut coverage=if node.content_rect.contains(local){rounded_coverage(local,node.rect,node.corner_radius)}else{0.0};
            let mut screen=rotate_point(local,node.rect.center(),node.rotation.to_radians());if let Some(warp)=node.surface_warp{screen=warp_surface_point(screen,warp);}
            for mask in &node.rounded_clips{let mask_local=rotate_point(screen,mask.rect.center(),-mask.rotation.to_radians());coverage*=rounded_coverage(mask_local,mask.rect,mask.radius);}
            vertex_colors.push(Color32::from_rgba_unmultiplied(polygon.color.r(),polygon.color.g(),polygon.color.b(),(polygon.color.a() as f32*coverage) as u8));*point=screen;
        }
        let pointer_hits=pointer.map_or(false,|point|{if !point_in_polygon(point,&polygon.points){return false;}let affine=node.surface_warp.map(|warp|inverse_surface_point(point,warp)).unwrap_or(point);let local=rotate_point(affine,node.rect.center(),-node.rotation.to_radians());node.content_rect.contains(local)&&rounded_coverage(local,node.rect,node.corner_radius)>0.0&&node.rounded_clips.iter().all(|mask|{let mask_local=rotate_point(point,mask.rect.center(),-mask.rotation.to_radians());rounded_coverage(mask_local,mask.rect,mask.radius)>0.0})});
        if pointer_hits{hovered=Some(polygon.referent);}
        if let Some((uri,uvs))=polygon.texture {
            if let Some(texture)=ensure_gui_texture(ui,textures,&uri) {
                let mut mesh=egui::Mesh::with_texture(texture);
                for ((point,uv),color) in polygon.points.into_iter().zip(uvs).zip(vertex_colors) { mesh.vertices.push(egui::epaint::Vertex{pos:point,uv,color}); }
                for index in 1..mesh.vertices.len().saturating_sub(1) { mesh.indices.extend_from_slice(&[0,index as u32,index as u32+1]); }
                painter.add(egui::Shape::mesh(mesh)); continue;
            }
        }
        let mut mesh=egui::Mesh::default();for(point,color)in polygon.points.into_iter().zip(vertex_colors){mesh.vertices.push(egui::epaint::Vertex{pos:point,uv:egui::epaint::WHITE_UV,color});}mesh.indices.extend_from_slice(&[0,1,2]);painter.add(egui::Shape::mesh(mesh));
    }
    hovered
}

fn ensure_gui_texture(ui: &egui::Ui, textures: &mut std::collections::HashMap<String, egui::TextureHandle>,
                      uri: &str) -> Option<egui::TextureId> {
    if !textures.contains_key(uri) {
        let decoded = crate::asset_downloader::get_cached_image(uri).or_else(|| {
            crate::asset_downloader::extract_asset_id(uri)
                .and_then(|id| crate::asset_downloader::get_cached_image(&id))
        })?;
        let image = egui::ColorImage::from_rgba_unmultiplied([decoded.width, decoded.height], &decoded.rgba);
        textures.insert(uri.to_string(), ui.ctx().load_texture(uri, image, egui::TextureOptions::LINEAR));
    }
    textures.get(uri).map(egui::TextureHandle::id)
}

fn gather_class(dom: &WeakDom, referent: Ref, class: &str, output: &mut Vec<Ref>) {
    let Some(instance) = dom.get_by_ref(referent) else { return; };
    if instance.class == class { output.push(referent); }
    for child in instance.children() { gather_class(dom, *child, class, output); }
}

/// Draw enabled ScreenGuis and world-projected BillboardGuis, returning the
/// topmost clicked Roblox referent for Explorer synchronization.
pub fn draw_starter_gui(
    ui: &mut egui::Ui,
    viewport: Rect,
    dom: &WeakDom,
    textures: &mut std::collections::HashMap<String, egui::TextureHandle>,
    scroll_offsets: &mut std::collections::HashMap<Ref, Vec2>,
    text_inputs: &mut std::collections::HashMap<Ref, String>,
    hovered_gui: &mut std::collections::HashSet<Ref>,
    runtime_events: &mut Vec<GuiRuntimeEvent>,
    selected: Option<Ref>,
    orbit: &crate::bevy_render::OrbitCam,
    viewport_scene: &crate::bevy_render::ViewportScene,
) -> Option<Ref> {
    let starter = dom.root().children().iter().find_map(|referent| {
        dom.get_by_ref(*referent).filter(|instance| instance.class == "StarterGui").map(|_| *referent)
    });
    let mut nodes = Vec::new();
    let mut sequence = 0;
    let mut overrides = std::collections::HashMap::new();
    let layout_painter = ui.painter().clone();

    // BillboardGui uses the same 2D layout/render tree after projecting its
    // Adornee (or parent Part) through the Bevy orbit camera.
    let mut billboards = Vec::new();
    gather_class(dom, dom.root_ref(), "BillboardGui", &mut billboards);
    let aspect = viewport.width() / viewport.height().max(1.0);
    for (billboard_order, referent) in billboards.into_iter().enumerate() {
        let Some(billboard) = dom.get_by_ref(referent) else { continue; };
        if matches!(billboard.properties.get(&rbx_dom_weak::ustr("Enabled")), Some(Variant::Bool(false))) { continue; }
        let adornee = match billboard.properties.get(&rbx_dom_weak::ustr("Adornee")) {
            Some(Variant::Ref(value)) if !value.is_none() => *value,
            _ => billboard.parent(),
        };
        let Some(part) = dom.get_by_ref(adornee) else { continue; };
        let mut point = match part.properties.get(&rbx_dom_weak::ustr("WorldPosition"))
            .or_else(|| part.properties.get(&rbx_dom_weak::ustr("Position")))
        {
            Some(Variant::Vector3(position)) => [position.x, position.y, position.z],
            _ => match part.properties.get(&rbx_dom_weak::ustr("CFrame")) {
                Some(Variant::CFrame(cframe)) => [cframe.position.x, cframe.position.y, cframe.position.z],
                _ => continue,
            },
        };
        let part_size = match part.properties.get(&rbx_dom_weak::ustr("Size")) {
            Some(Variant::Vector3(size)) => [size.x, size.y, size.z],
            _ => [0.0, 0.0, 0.0],
        };
        let mut camera_offset = [0.0; 3];
        let mut world_offset = [0.0; 3];
        if let Some(Variant::Vector3(offset)) = billboard.properties.get(&rbx_dom_weak::ustr("StudsOffset")) {
            camera_offset[0] += offset.x; camera_offset[1] += offset.y; camera_offset[2] += offset.z;
        }
        if let Some(Variant::Vector3(offset)) = billboard.properties.get(&rbx_dom_weak::ustr("StudsOffsetWorldSpace")) {
            world_offset[0] += offset.x; world_offset[1] += offset.y; world_offset[2] += offset.z;
        }
        if let Some(Variant::Vector3(offset)) = billboard.properties.get(&rbx_dom_weak::ustr("ExtentsOffset")) {
            camera_offset[0] += offset.x*part_size[0]*0.5;
            camera_offset[1] += offset.y*part_size[1]*0.5;
            camera_offset[2] += offset.z*part_size[2]*0.5;
        }
        if let Some(Variant::Vector3(offset)) = billboard.properties.get(&rbx_dom_weak::ustr("ExtentsOffsetWorldSpace")) {
            world_offset[0] += offset.x*part_size[0]*0.5;
            world_offset[1] += offset.y*part_size[1]*0.5;
            world_offset[2] += offset.z*part_size[2]*0.5;
        }
        let camera_world = crate::bevy_render::camera_relative_offset(orbit, camera_offset);
        for axis in 0..3 { point[axis] += camera_world[axis] + world_offset[axis]; }
        let Some(projected) = crate::bevy_render::project_world_point(orbit, point, aspect) else { continue; };
        let max_distance = number(billboard.properties.get(&rbx_dom_weak::ustr("MaxDistance")), 0.0);
        if max_distance > 0.0 && projected[2] > max_distance { continue; }
        let always_on_top = bool_value(billboard.properties.get(&rbx_dom_weak::ustr("AlwaysOnTop")), false);
        if !always_on_top {
            if let Some(hit) = crate::bevy_render::pick_part(viewport_scene, orbit, [projected[0], projected[1]], aspect) {
                let occlusion_target = if part.class == "Attachment" { part.parent() } else { adornee };
                if hit != occlusion_target { continue; }
            }
        }
        let lower_limit = number(billboard.properties.get(&rbx_dom_weak::ustr("DistanceLowerLimit")), 0.0).max(0.0);
        let upper_limit = number(billboard.properties.get(&rbx_dom_weak::ustr("DistanceUpperLimit")), -1.0);
        let scale_distance = if upper_limit >= 0.0 {
            projected[2].clamp(lower_limit.min(upper_limit), upper_limit.max(lower_limit))
        } else {
            projected[2].max(lower_limit)
        };
        let pixels_per_stud = viewport.height() / (2.0 * (30.0_f32.to_radians().tan()) * scale_distance.max(0.01));
        let size = match billboard.properties.get(&rbx_dom_weak::ustr("Size")) {
            Some(Variant::UDim2(value)) => Vec2::new(
                value.x.scale*pixels_per_stud + value.x.offset as f32,
                value.y.scale*pixels_per_stud + value.y.offset as f32,
            ),
            _ => Vec2::new(100.0, 100.0),
        }.max(Vec2::ZERO);
        if size.x <= 0.0 || size.y <= 0.0 { continue; }
        let size_offset = vector2(billboard.properties.get(&rbx_dom_weak::ustr("SizeOffset")), Vec2::ZERO);
        let center = Pos2::new(viewport.left()+projected[0]*viewport.width(), viewport.top()+projected[1]*viewport.height())
            + Vec2::new(size_offset.x*size.x, -size_offset.y*size.y);
        let billboard_rect = Rect::from_center_size(center, size);
        let path = vec![(-1, billboard_order)];
        for child in billboard.children() {
            collect(dom, &layout_painter, *child, billboard_rect, viewport, -1_000_000,
                true, 1.0, 1.0, Color32::WHITE, &path, None, &mut overrides,
                scroll_offsets, &mut sequence, &mut nodes);
        }
    }

    // SurfaceGui shares the layout engine. Its part face is projected to a
    // screen rectangle; Bevy depth picking provides normal occlusion.
    let mut surfaces = Vec::new();
    gather_class(dom, dom.root_ref(), "SurfaceGui", &mut surfaces);
    for (surface_order, referent) in surfaces.into_iter().enumerate() {
        let Some(surface) = dom.get_by_ref(referent) else { continue; };
        if matches!(surface.properties.get(&rbx_dom_weak::ustr("Enabled")), Some(Variant::Bool(false))) { continue; }
        let adornee = match surface.properties.get(&rbx_dom_weak::ustr("Adornee")) {
            Some(Variant::Ref(value)) if !value.is_none() => *value,
            _ => surface.parent(),
        };
        let Some(part) = dom.get_by_ref(adornee) else { continue; };
        let Some(Variant::CFrame(cframe)) = part.properties.get(&rbx_dom_weak::ustr("CFrame")) else { continue; };
        let size = match part.properties.get(&rbx_dom_weak::ustr("Size")) {
            Some(Variant::Vector3(size)) => [size.x, size.y, size.z], _ => continue,
        };
        let face = enum_value(surface.properties.get(&rbx_dom_weak::ustr("Face")), 5);
        let (u, v, fixed) = match face {
            0 => ([0.0, 0.0, size[2]*0.5], [0.0, size[1]*0.5, 0.0], [size[0]*0.5, 0.0, 0.0]),
            1 => ([size[0]*0.5, 0.0, 0.0], [0.0, 0.0, size[2]*0.5], [0.0, size[1]*0.5, 0.0]),
            2 => ([-size[0]*0.5, 0.0, 0.0], [0.0, size[1]*0.5, 0.0], [0.0, 0.0, size[2]*0.5]),
            3 => ([0.0, 0.0, -size[2]*0.5], [0.0, size[1]*0.5, 0.0], [-size[0]*0.5, 0.0, 0.0]),
            4 => ([size[0]*0.5, 0.0, 0.0], [0.0, 0.0, -size[2]*0.5], [0.0, -size[1]*0.5, 0.0]),
            _ => ([size[0]*0.5, 0.0, 0.0], [0.0, size[1]*0.5, 0.0], [0.0, 0.0, -size[2]*0.5]),
        };
        let transform = |local: [f32; 3]| [
            cframe.position.x + cframe.orientation.x.x*local[0] + cframe.orientation.x.y*local[1] + cframe.orientation.x.z*local[2],
            cframe.position.y + cframe.orientation.y.x*local[0] + cframe.orientation.y.y*local[1] + cframe.orientation.y.z*local[2],
            cframe.position.z + cframe.orientation.z.x*local[0] + cframe.orientation.z.y*local[1] + cframe.orientation.z.z*local[2],
        ];
        let local_normal = match face { 0 => [1.0,0.0,0.0], 1 => [0.0,1.0,0.0], 2 => [0.0,0.0,1.0],
            3 => [-1.0,0.0,0.0], 4 => [0.0,-1.0,0.0], _ => [0.0,0.0,-1.0] };
        let world_normal_point = transform(local_normal);
        let center_world = transform([0.0, 0.0, 0.0]);
        let normal = [world_normal_point[0]-center_world[0], world_normal_point[1]-center_world[1], world_normal_point[2]-center_world[2]];
        let (sin_pitch, cos_pitch) = orbit.pitch.sin_cos();
        let (sin_yaw, cos_yaw) = orbit.yaw.sin_cos();
        let camera = [orbit.target[0]+orbit.dist*cos_pitch*sin_yaw, orbit.target[1]+orbit.dist*sin_pitch,
            orbit.target[2]+orbit.dist*cos_pitch*cos_yaw];
        let to_camera = [camera[0]-center_world[0], camera[1]-center_world[1], camera[2]-center_world[2]];
        if normal[0]*to_camera[0] + normal[1]*to_camera[1] + normal[2]*to_camera[2] <= 0.0 { continue; }
        let mut projected_points = Vec::new();
        for (su, sv) in [(-1.0,-1.0), (1.0,-1.0), (1.0,1.0), (-1.0,1.0)] {
            let local = [fixed[0]+u[0]*su+v[0]*sv, fixed[1]+u[1]*su+v[1]*sv, fixed[2]+u[2]*su+v[2]*sv];
            let Some(point) = crate::bevy_render::project_world_point(orbit, transform(local), aspect) else { projected_points.clear(); break; };
            projected_points.push(Pos2::new(viewport.left()+point[0]*viewport.width(), viewport.top()+point[1]*viewport.height()));
        }
        if projected_points.len() != 4 { continue; }
        let projected_bounds = projected_points.iter().fold(Rect::NOTHING, |mut bounds, point| {
            bounds.extend_with(*point); bounds
        });
        let surface_clip = projected_bounds.intersect(viewport);
        if surface_clip.width() <= 0.0 || surface_clip.height() <= 0.0 { continue; }
        let edge = |a: Pos2, b: Pos2| (b-a).length();
        let surface_width = (edge(projected_points[0], projected_points[1]) + edge(projected_points[3], projected_points[2])) * 0.5;
        let surface_height = (edge(projected_points[0], projected_points[3]) + edge(projected_points[1], projected_points[2])) * 0.5;
        let surface_center = Pos2::new(
            projected_points.iter().map(|point| point.x).sum::<f32>() * 0.25,
            projected_points.iter().map(|point| point.y).sum::<f32>() * 0.25,
        );
        let surface_rotation = (projected_points[1].y-projected_points[0].y)
            .atan2(projected_points[1].x-projected_points[0].x);
        let surface_rect = Rect::from_center_size(surface_center, Vec2::new(surface_width, surface_height));
        let always_on_top = bool_value(surface.properties.get(&rbx_dom_weak::ustr("AlwaysOnTop")), false);
        if !always_on_top {
            let center = transform(fixed);
            if let Some(projected) = crate::bevy_render::project_world_point(orbit, center, aspect) {
                if let Some(hit) = crate::bevy_render::pick_part(viewport_scene, orbit, [projected[0], projected[1]], aspect) {
                    if hit != adornee { continue; }
                }
            }
        }
        let sizing_mode = enum_value(surface.properties.get(&rbx_dom_weak::ustr("SizingMode")), 0);
        let canvas = if sizing_mode == 1 {
            let pixels_per_stud = number(surface.properties.get(&rbx_dom_weak::ustr("PixelsPerStud")), 50.0).max(1.0);
            let u_length = (u[0]*u[0] + u[1]*u[1] + u[2]*u[2]).sqrt()*2.0;
            let v_length = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt()*2.0;
            Vec2::new(u_length*pixels_per_stud, v_length*pixels_per_stud)
        } else {
            vector2(surface.properties.get(&rbx_dom_weak::ustr("CanvasSize")), Vec2::new(800.0, 600.0))
        };
        let screen_scale = (surface_rect.width()/canvas.x.max(1.0)).min(surface_rect.height()/canvas.y.max(1.0));
        let path = vec![(-2, surface_order)];
        let first_surface_node = nodes.len();
        for child in surface.children() {
            collect(dom, &layout_painter, *child, surface_rect, viewport, -900_000,
                true, screen_scale, 1.0, Color32::WHITE, &path, None, &mut overrides,
                scroll_offsets, &mut sequence, &mut nodes);
        }
        // Rotate the complete hierarchy with its Part face. Child-local
        // Rotation remains additive, just as it is for an oriented SurfaceGui.
        let perspective_warp = SurfaceWarp { source: surface_rect, rotation: surface_rotation,
            quad: [projected_points[0],projected_points[1],projected_points[2],projected_points[3]] };
        for node in &mut nodes[first_surface_node..] {
            let center = rotate_point(node.rect.center(), surface_center, surface_rotation);
            let content_center = rotate_point(node.content_rect.center(), surface_center, surface_rotation);
            let had_ancestor_clip = node.clip.width() < viewport.width()-0.5 || node.clip.height() < viewport.height()-0.5;
            let transformed_clip = if had_ancestor_clip {
                let clip_center = rotate_point(node.clip.center(), surface_center, surface_rotation);
                rotated_bounds(Rect::from_center_size(clip_center, node.clip.size()), surface_rotation.to_degrees())
                    .intersect(surface_clip)
            } else { surface_clip };
            node.rect = Rect::from_center_size(center, node.rect.size());
            node.content_rect = Rect::from_center_size(content_center, node.content_rect.size());
            node.rotation += surface_rotation.to_degrees();
            node.clip = transformed_clip;
            node.surface_warp = Some(perspective_warp);
        }
    }

    let mut gui_roots=Vec::new();
    if let Some(starter)=starter { gui_roots.push(starter); }
    gather_class(dom,dom.root_ref(),"PlayerGui",&mut gui_roots);
    let mut screen_order=0usize;
    for root in gui_roots {
        let Some(container)=dom.get_by_ref(root) else {continue;};
        for child in container.children() {
            let Some(gui) = dom.get_by_ref(*child) else { continue; };
            if gui.class != "ScreenGui" || matches!(gui.properties.get(&rbx_dom_weak::ustr("Enabled")), Some(Variant::Bool(false))) { continue; }
            let display_order = enum_value(gui.properties.get(&rbx_dom_weak::ustr("DisplayOrder")), 0);
            let global_z = enum_value(gui.properties.get(&rbx_dom_weak::ustr("ZIndexBehavior")), 1) == 0;
            // ScreenInsets.None=0, DeviceSafeInsets=1, CoreUISafeInsets=2,
            // TopbarSafeInsets=3. The editor viewport already excludes Android
            // system cutouts, so only Roblox's core/top-bar inset remains.
            let ignore_inset=bool_value(gui.properties.get(&rbx_dom_weak::ustr("IgnoreGuiInset")),false);
            let screen_insets=if ignore_inset { 0 } else { enum_value(gui.properties.get(&rbx_dom_weak::ustr("ScreenInsets")),2) };
            let root_rect=if screen_insets==2 || screen_insets==3 {
                Rect::from_min_max(Pos2::new(viewport.left(),(viewport.top()+58.0).min(viewport.bottom())),viewport.max)
            } else { viewport };
            let clip_to_safe=bool_value(gui.properties.get(&rbx_dom_weak::ustr("ClipToDeviceSafeArea")),true)&&screen_insets!=0;
            let root_clip=if clip_to_safe{root_rect}else{viewport};
            let root_path = vec![(0, screen_order)]; screen_order+=1;let first_node=nodes.len();
            collect(dom, &layout_painter, *child, root_rect, root_clip, display_order,
                global_z, 1.0, 1.0, Color32::WHITE, &root_path, None, &mut overrides, scroll_offsets, &mut sequence, &mut nodes);
            if enum_value(gui.properties.get(&rbx_dom_weak::ustr("SafeAreaCompatibility")),1)==1&&screen_insets!=0 {
                for node in &mut nodes[first_node..] {if node.rect.left()<=root_rect.left()+0.5&&node.rect.right()>=root_rect.right()-0.5&&node.rect.top()<=root_rect.top()+0.5&&node.rect.bottom()>=root_rect.bottom()-0.5{node.background_rect=viewport;}}
            }
        }
    }
    // Resolve rounded ClipsDescendants masks after all containers have their
    // final screen transforms (including SurfaceGui orientation).
    let visual_bounds: std::collections::HashMap<Ref, RoundedMask> = nodes.iter().map(|node| (
        node.referent, RoundedMask { rect: node.rect, radius: node.corner_radius, rotation: node.rotation }
    )).collect();
    let canvas_gradients:std::collections::HashMap<Ref,(GuiGradient,Rect)>=nodes.iter().filter(|node|node.class=="CanvasGroup"&&!node.global_z).filter_map(|node|node.gradient.clone().map(|gradient|(node.referent,(gradient,node.rect)))).collect();
    for node in &mut nodes {
        let mut parent = dom.get_by_ref(node.referent).map(|instance| instance.parent());
        while let Some(referent) = parent.filter(|referent| !referent.is_none()) {
            let Some(instance) = dom.get_by_ref(referent) else { break; };
            if instance.class=="CanvasGroup" || bool_value(instance.properties.get(&rbx_dom_weak::ustr("ClipsDescendants")), false) {
                if let Some(mask) = visual_bounds.get(&referent).filter(|mask| mask.radius > 0.0) {
                    node.rounded_clips.push(*mask);
                }
            }
            if let Some((gradient,rect))=canvas_gradients.get(&referent){node.group_gradients.push((gradient.clone(),*rect));}
            parent = Some(instance.parent());
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
    let keyboard_selection = if ui.input(|input|input.key_pressed(egui::Key::Tab)) {
        let reverse=ui.input(|input|input.modifiers.shift);
        let mut candidates:Vec<(i32,usize,Ref)>=nodes.iter().filter(|node|node.selectable&&node.interactable)
            .map(|node|(node.selection_order,node.order,node.referent)).collect();
        candidates.sort_by_key(|candidate|(candidate.0,candidate.1));
        if candidates.is_empty(){None}else{
            let current=candidates.iter().position(|candidate|Some(candidate.2)==selected);
            let index=match (current,reverse){(Some(0),true)=>candidates.len()-1,(Some(index),true)=>index-1,(Some(index),false)=>(index+1)%candidates.len(),(None,true)=>candidates.len()-1,(None,false)=>0};
            Some(candidates[index].2)
        }
    } else {
        let direction=ui.input(|input| {
            if input.key_pressed(egui::Key::ArrowLeft){Some((Vec2::new(-1.0,0.0),"NextSelectionLeft"))}
            else if input.key_pressed(egui::Key::ArrowRight){Some((Vec2::new(1.0,0.0),"NextSelectionRight"))}
            else if input.key_pressed(egui::Key::ArrowUp){Some((Vec2::new(0.0,-1.0),"NextSelectionUp"))}
            else if input.key_pressed(egui::Key::ArrowDown){Some((Vec2::new(0.0,1.0),"NextSelectionDown"))}
            else{None}
        });
        direction.and_then(|(direction,next_property)| {
            let current=nodes.iter().find(|node|Some(node.referent)==selected&&node.selectable&&node.interactable)?;
            if let Some(Variant::Ref(target))=dom.get_by_ref(current.referent).and_then(|instance|instance.properties.get(&rbx_dom_weak::ustr(next_property))) {
                if nodes.iter().any(|node|node.referent==*target&&node.selectable&&node.interactable){return Some(*target);}
            }
            let origin=current.rect.center();
            nodes.iter().filter(|node|node.referent!=current.referent&&node.selectable&&node.interactable).filter_map(|node|{
                let delta=node.rect.center()-origin; let forward=delta.dot(direction);
                if forward<=0.5{return None;}
                let perpendicular=(delta.x*direction.y-delta.y*direction.x).abs();
                Some((forward+perpendicular*2.0,node.selection_order,node.order,node.referent))
            }).min_by(|left,right|left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)).then(left.2.cmp(&right.2))).map(|candidate|candidate.3)
        })
    };
    runtime_events.clear();
    let mut current_hovered=std::collections::HashSet::new();
    let runtime_pointer=ui.input(|input|input.pointer.hover_pos()).unwrap_or(Pos2::ZERO);
    let pointer_delta=ui.input(|input|input.pointer.delta());
    let runtime_position=[runtime_pointer.x,runtime_pointer.y];
    let runtime_delta=[pointer_delta.x,pointer_delta.y];
    let mut clicked = keyboard_selection;
    // rect, color, owner, horizontal, canvas maximum, thumb travel
    let mut scroll_bars: Vec<(Rect, Color32, Ref, bool, f32, f32, f32, bool, [Option<String>; 3])> = Vec::new();
    for node in nodes {
        runtime_events.push(GuiRuntimeEvent{referent:node.referent,kind:GuiRuntimeEventKind::Layout,position:[node.rect.min.x-viewport.min.x,node.rect.min.y-viewport.min.y],delta:[node.rect.width(),node.rect.height()]});
        if node.clip.width() <= 0.0 || node.clip.height() <= 0.0 { continue; }
        let hit_rect = if let Some(warp)=node.surface_warp {
            let mut bounds=Rect::NOTHING;
            for point in [node.rect.left_top(),node.rect.right_top(),node.rect.right_bottom(),node.rect.left_bottom()] {
                bounds.extend_with(warp_surface_point(rotate_point(point,node.rect.center(),node.rotation.to_radians()),warp));
            }
            bounds.intersect(node.clip)
        } else { rotated_bounds(node.rect, node.rotation).intersect(node.clip) };
        let response = ui.interact(hit_rect, ui.make_persistent_id(("roblox_gui", format!("{:?}", node.referent))), egui::Sense::click());
        let pointer_inside = ui.input(|input| input.pointer.hover_pos()).map(|point| {
            let affine_point=node.surface_warp.map(|warp| inverse_surface_point(point,warp)).unwrap_or(point);
            let local = rotate_point(affine_point, node.rect.center(), -node.rotation.to_radians());
            let own_shape=node.rect.contains(local)&&rounded_coverage(local,node.rect,node.corner_radius)>0.0;
            let inherited_shape=node.rounded_clips.iter().all(|mask|{
                let local=rotate_point(point,mask.rect.center(),-mask.rotation.to_radians());
                rounded_coverage(local,mask.rect,mask.radius)>0.0
            });
            own_shape && inherited_shape && node.clip.contains(point)
        }).unwrap_or(false);
        if pointer_inside && node.interactable { current_hovered.insert(node.referent); }
        let is_button=matches!(node.class.as_str(),"TextButton"|"ImageButton");
        if is_button && node.interactable && response.clicked() && pointer_inside {
            for kind in [GuiRuntimeEventKind::MouseButton1Down,GuiRuntimeEventKind::MouseButton1Up,GuiRuntimeEventKind::MouseButton1Click,GuiRuntimeEventKind::Activated] {
                runtime_events.push(GuiRuntimeEvent{referent:node.referent,kind,position:runtime_position,delta:runtime_delta});
            }
        }
        let keyboard_activated=is_button&&node.interactable&&selected==Some(node.referent)&&ui.input(|input|input.key_pressed(egui::Key::Enter)||input.key_pressed(egui::Key::Space));
        if keyboard_activated {
            runtime_events.push(GuiRuntimeEvent{referent:node.referent,kind:GuiRuntimeEventKind::ActivatedKeyboard,position:runtime_position,delta:runtime_delta});
            clicked=Some(node.referent);
        }
        if node.class == "ScrollingFrame" {
            let allow_x = node.scrolling_direction != 2;
            let allow_y = node.scrolling_direction != 1;
            let raw_maximum = (node.canvas_size - node.content_rect.size()).max(Vec2::ZERO);
            let maximum = Vec2::new(if allow_x { raw_maximum.x } else { 0.0 }, if allow_y { raw_maximum.y } else { 0.0 });
            if node.scrolling_enabled && response.hovered() && pointer_inside && (maximum.x > 0.0 || maximum.y > 0.0) {
                let delta = ui.input(|input| input.smooth_scroll_delta);
                if delta != Vec2::ZERO {
                    let entry = scroll_offsets.entry(node.referent).or_insert(Vec2::ZERO);
                    let delta = delta*node.scroll_rate;
                    let horizontal_wheel = if maximum.y <= 0.0 { delta.y } else { 0.0 };
                    if allow_x {
                        entry.x = (entry.x - delta.x - horizontal_wheel)
                            .clamp(-node.canvas_position.x, maximum.x-node.canvas_position.x);
                    }
                    if allow_y {
                        entry.y = (entry.y - delta.y)
                            .clamp(-node.canvas_position.y, maximum.y-node.canvas_position.y);
                    }
                    ui.ctx().request_repaint();
                }
            }
            let effective = (node.canvas_position + scroll_offsets.get(&node.referent).copied().unwrap_or(Vec2::ZERO)).clamp(Vec2::ZERO, maximum);
            let thickness = node.scroll_bar_thickness;
            if thickness > 0.0 && maximum.y > 0.0 {
                let track_bottom = node.rect.bottom() - if maximum.x > 0.0 { thickness } else { 0.0 };
                let (track_left, track_right) = if node.vertical_scroll_bar_left {
                    (node.rect.left(), node.rect.left()+thickness)
                } else {
                    (node.rect.right()-thickness, node.rect.right())
                };
                let track = Rect::from_min_max(Pos2::new(track_left, node.rect.top()), Pos2::new(track_right, track_bottom));
                let thumb_height = (track.height() * node.content_rect.height()/node.canvas_size.y.max(1.0)).max(thickness);
                let travel = (track.height()-thumb_height).max(0.0);
                let top = track.top() + travel * effective.y/maximum.y.max(1.0);
                scroll_bars.push((Rect::from_min_size(Pos2::new(track.left(), top), Vec2::new(thickness, thumb_height)),
                    node.scroll_bar_color, node.referent, false, maximum.y, travel, node.canvas_position.y, node.scrolling_enabled,
                    [node.scroll_top_image.clone(), node.scroll_mid_image.clone(), node.scroll_bottom_image.clone()]));
            }
            if thickness > 0.0 && maximum.x > 0.0 {
                let has_vertical = maximum.y > 0.0;
                let track_left = node.rect.left() + if has_vertical && node.vertical_scroll_bar_left { thickness } else { 0.0 };
                let track_right = node.rect.right() - if has_vertical && !node.vertical_scroll_bar_left { thickness } else { 0.0 };
                let track = Rect::from_min_max(Pos2::new(track_left, node.rect.bottom()-thickness), Pos2::new(track_right, node.rect.bottom()));
                let thumb_width = (track.width() * node.content_rect.width()/node.canvas_size.x.max(1.0)).max(thickness);
                let travel = (track.width()-thumb_width).max(0.0);
                let left = track.left() + travel * effective.x/maximum.x.max(1.0);
                scroll_bars.push((Rect::from_min_size(Pos2::new(left, track.top()), Vec2::new(thumb_width, thickness)),
                    node.scroll_bar_color, node.referent, true, maximum.x, travel, node.canvas_position.x, node.scrolling_enabled,
                    [node.scroll_top_image.clone(), node.scroll_mid_image.clone(), node.scroll_bottom_image.clone()]));
            }
        }
        let painter = ui.painter().with_clip_rect(node.clip);
        let is_button = matches!(node.class.as_str(), "TextButton" | "ImageButton");
        let pressed = is_button && node.interactable && response.is_pointer_button_down_on();
        let hovered = is_button && node.interactable && response.hovered() && pointer_inside;
        let button_factor = if node.auto_button_color && pressed { 0.72 }
            else if node.auto_button_color && hovered { 0.88 } else { 1.0 };
        let background_painter=if node.background_rect!=node.rect{ui.painter().with_clip_rect(viewport)}else{painter.clone()};
        if node.gradient.is_none()&&node.group_gradients.is_empty() {
        if !node.rounded_clips.is_empty() || node.surface_warp.is_some() {
            paint_masked_solid(&background_painter,node.background_rect,shade_color(node.background,button_factor),node.corner_radius,node.rotation,&node.rounded_clips,node.surface_warp);
        } else if node.rotation.abs() < 0.001 {
            background_painter.rect_filled(node.background_rect,node.corner_radius,shade_color(node.background,button_factor));
        } else {
            let radians = node.rotation.to_radians();
            let corners = [node.background_rect.left_top(),node.background_rect.right_top(),node.background_rect.right_bottom(),node.background_rect.left_bottom()]
                .into_iter().map(|point| rotate_point(point,node.background_rect.center(),radians)).collect();
            background_painter.add(egui::Shape::convex_polygon(corners,
                shade_color(node.background,button_factor),Stroke::NONE));
        }}
        if node.gradient.is_some()||!node.group_gradients.is_empty() {
            paint_gradient(&background_painter.with_clip_rect(rotated_bounds(node.background_rect,node.rotation).intersect(if node.background_rect!=node.rect{viewport}else{node.clip})),node.background_rect,node.gradient_rect,
                shade_color(node.background,button_factor),node.gradient.as_ref(),&node.group_gradients,node.rotation,node.corner_radius,&node.rounded_clips,node.surface_warp);
        }
        if node.rotation.abs() < 0.001 {
            if node.border_size > 0.0 {
                painter.rect_stroke(node.rect, node.corner_radius, Stroke::new(node.border_size, node.border), egui::StrokeKind::Inside);
            }
            if let Some((thickness, color, offset, position)) = node.ui_stroke {
                painter.rect_stroke(node.rect.expand(offset), node.corner_radius + offset.max(0.0),
                    Stroke::new(thickness, color), position);
            }
        } else {
            let radians = node.rotation.to_radians();
            let outline = |rect: Rect| {
                let mut corners: Vec<Pos2> = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()]
                    .into_iter().map(|point| {
                        let rotated=rotate_point(point,node.rect.center(),radians);
                        node.surface_warp.map(|warp| warp_surface_point(rotated,warp)).unwrap_or(rotated)
                    }).collect();
                corners.push(corners[0]); corners
            };
            if node.border_size > 0.0 { painter.line(outline(node.rect), Stroke::new(node.border_size, node.border)); }
            if let Some((thickness, color, offset, _)) = node.ui_stroke {
                painter.line(outline(node.rect.expand(offset)), Stroke::new(thickness, color));
            }
        }
        let viewport_hovered = if node.class == "ViewportFrame" {
            paint_viewport_frame(&painter, ui, &node, dom, textures)
        } else { None };
        let displayed_image = if pressed {
            node.pressed_image.as_ref().or(node.hover_image.as_ref()).or(node.image.as_ref())
        } else if hovered {
            node.hover_image.as_ref().or(node.image.as_ref())
        } else {
            node.image.as_ref()
        };
        if let Some(uri) = displayed_image {
            // Sampler state belongs to the egui texture, so retain separate GPU
            // handles when one asset is used by both Default and Pixelated
            // ImageLabels.
            let texture_key = if node.image_pixelated { format!("{uri}#pixelated") } else { uri.clone() };
            if !textures.contains_key(&texture_key) {
                let decoded = crate::asset_downloader::get_cached_image(uri).or_else(|| {
                    crate::asset_downloader::extract_asset_id(uri)
                        .and_then(|id| crate::asset_downloader::get_cached_image(&id))
                });
                if let Some(image) = decoded {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width, image.height], &image.rgba,
                    );
                    let options = if node.image_pixelated { egui::TextureOptions::NEAREST }
                        else { egui::TextureOptions::LINEAR };
                    textures.insert(texture_key.clone(), ui.ctx().load_texture(
                        &texture_key, color_image, options,
                    ));
                }
            }
            if let Some(texture) = textures.get(&texture_key) {
                paint_image(&painter, &node, texture, shade_color(node.image_color, button_factor));
            }
        }
        let editable_textbox = node.class == "TextBox" && node.rotation.abs() < 0.001;
        if editable_textbox {
            let value = text_inputs.entry(node.referent).or_insert_with(|| node.text.clone());
            let hint = egui::RichText::new(node.placeholder_text.clone()).color(node.placeholder_color);
            let textbox_font = FontId::new(node.text_size,roblox_font_family(node.font_monospace,node.font_extended,node.font_source_sans,node.font_montserrat,node.font_arimo,node.font_italic,node.font_weight));
            let widget = if node.multiline {
                egui::TextEdit::multiline(value)
                    .desired_width(node.content_rect.width()).font(textbox_font.clone())
                    .text_color(node.text_color).hint_text(hint).frame(false)
            } else {
                egui::TextEdit::singleline(value)
                    .desired_width(node.content_rect.width()).font(textbox_font)
                    .text_color(node.text_color).hint_text(hint).frame(false)
            };
            let text_response = ui.put(node.content_rect, widget);
            if text_response.gained_focus() {
                if node.clear_text_on_focus { value.clear(); runtime_events.push(GuiRuntimeEvent{referent:node.referent,kind:GuiRuntimeEventKind::TextChanged,position:runtime_position,delta:runtime_delta}); }
                runtime_events.push(GuiRuntimeEvent{referent:node.referent,kind:GuiRuntimeEventKind::Focused,position:runtime_position,delta:runtime_delta});
            }
            if text_response.changed() { runtime_events.push(GuiRuntimeEvent{referent:node.referent,kind:GuiRuntimeEventKind::TextChanged,position:runtime_position,delta:runtime_delta}); }
            if text_response.lost_focus() { runtime_events.push(GuiRuntimeEvent{referent:node.referent,kind:GuiRuntimeEventKind::FocusLost,position:runtime_position,delta:runtime_delta}); }
            if text_response.clicked() { clicked = Some(node.referent); }
        }
        if !node.text.is_empty() && !editable_textbox {
            let text_color = shade_color(node.text_color, button_factor);
            let text_rect = node.content_rect;
            let wrap_width = if node.text_wrapped || node.text_truncate != 0 { text_rect.width() } else { f32::INFINITY };
            let mut font_size = if node.text_scaled { text_rect.height().max(1.0) } else { node.text_size };
            font_size = font_size.clamp(node.text_min_size, node.text_max_size);
            let mut galley = layout_text(&painter, &node, font_size, text_color, wrap_width, text_rect.height());
            if node.text_scaled && (galley.size().x > text_rect.width() || galley.size().y > text_rect.height()) {
                let scale = (text_rect.width() / galley.size().x.max(1.0))
                    .min(text_rect.height() / galley.size().y.max(1.0));
                font_size = (font_size * scale).clamp(node.text_min_size, node.text_max_size);
                galley = layout_text(&painter, &node, font_size, text_color, wrap_width, text_rect.height());
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
                    paint_galley(&painter, pos + offset, galley.clone(), node.text_stroke, true,
                        node.rotation, node.rect.center());
                }
            }
            if let Some((thickness, color)) = node.text_ui_stroke {
                for direction in [Vec2::new(-1.0,0.0), Vec2::new(1.0,0.0), Vec2::new(0.0,-1.0), Vec2::new(0.0,1.0),
                    Vec2::new(-0.707,-0.707), Vec2::new(0.707,-0.707), Vec2::new(-0.707,0.707), Vec2::new(0.707,0.707)] {
                    paint_galley(&painter, pos + direction*thickness, galley.clone(), color, true,
                        node.rotation, node.rect.center());
                }
            }
            let final_galley = if node.gradient.is_some() || !node.group_gradients.is_empty() || !node.rounded_clips.is_empty() || node.surface_warp.is_some() {
                mask_galley(galley.clone(),pos,node.gradient.as_ref(),&node.group_gradients,&node.rounded_clips,node.rect,node.gradient_rect,node.rotation,node.surface_warp)
            } else { galley };
            let render_rotation=if node.surface_warp.is_some() { 0.0 } else { node.rotation };
            paint_galley(&painter, pos, final_galley, text_color, false, render_rotation, node.rect.center());
        }
        if selected == Some(node.referent) {
            let transform_corner=|corner:Pos2|{
                let rotated=rotate_point(corner,node.rect.center(),node.rotation.to_radians());
                node.surface_warp.map(|warp|warp_surface_point(rotated,warp)).unwrap_or(rotated)
            };
            let corners=[transform_corner(node.rect.left_top()),transform_corner(node.rect.right_top()),
                transform_corner(node.rect.right_bottom()),transform_corner(node.rect.left_bottom())];
            painter.line(vec![corners[0],corners[1],corners[2],corners[3],corners[0]],Stroke::new(2.0,Color32::from_rgb(0,162,255)));
            for corner in corners {
                painter.rect_filled(Rect::from_center_size(corner, Vec2::splat(6.0)), 0.0, Color32::WHITE);
                painter.rect_stroke(Rect::from_center_size(corner, Vec2::splat(6.0)), 0.0,
                    Stroke::new(1.0, Color32::from_rgb(0, 110, 220)), egui::StrokeKind::Inside);
            }
        }
        if response.clicked() && pointer_inside { clicked = viewport_hovered.or(Some(node.referent)); }
    }
    let overlay_painter = ui.painter().with_clip_rect(viewport);
    for (rect, color, owner, horizontal, maximum, travel, authored, enabled, images) in scroll_bars {
        let sense = if enabled { egui::Sense::drag() } else { egui::Sense::hover() };
        let response = ui.interact(rect, ui.make_persistent_id(("scroll_thumb", format!("{:?}", owner), horizontal)), sense);
        let shown_color = if response.dragged() { shade_color(color, 0.72) }
            else if response.hovered() { shade_color(color, 0.88) } else { color };
        let main_length = if horizontal { rect.width() } else { rect.height() };
        let cap = (if horizontal { rect.height() } else { rect.width() }).min(main_length*0.5);
        let segments = if horizontal {
            [Rect::from_min_max(rect.min, Pos2::new(rect.left()+cap,rect.bottom())),
             Rect::from_min_max(Pos2::new(rect.left()+cap,rect.top()),Pos2::new(rect.right()-cap,rect.bottom())),
             Rect::from_min_max(Pos2::new(rect.right()-cap,rect.top()),rect.max)]
        } else {
            [Rect::from_min_max(rect.min,Pos2::new(rect.right(),rect.top()+cap)),
             Rect::from_min_max(Pos2::new(rect.left(),rect.top()+cap),Pos2::new(rect.right(),rect.bottom()-cap)),
             Rect::from_min_max(Pos2::new(rect.left(),rect.bottom()-cap),rect.max)]
        };
        let mut used_texture = false;
        for (segment, image) in segments.into_iter().zip(images.iter()) {
            if segment.width() <= 0.0 || segment.height() <= 0.0 { continue; }
            if let Some(texture) = image.as_deref().and_then(|uri| ensure_gui_texture(ui,textures,uri)) {
                overlay_painter.image(texture,segment,Rect::from_min_max(Pos2::ZERO,Pos2::new(1.0,1.0)),shown_color);
                used_texture=true;
            } else if images.iter().any(Option::is_some) {
                overlay_painter.rect_filled(segment,0.0,shown_color);
            }
        }
        if !used_texture && images.iter().all(Option::is_none) { overlay_painter.rect_filled(rect,2.0,shown_color); }
        if response.dragged() && travel > 0.0 {
            let pointer_delta = ui.input(|input| input.pointer.delta());
            let delta = if horizontal { pointer_delta.x } else { pointer_delta.y } * maximum / travel;
            let entry = scroll_offsets.entry(owner).or_insert(Vec2::ZERO);
            if horizontal { entry.x = (entry.x + delta).clamp(-authored, maximum-authored); }
            else { entry.y = (entry.y + delta).clamp(-authored, maximum-authored); }
            ui.ctx().request_repaint();
        }
    }
    if pointer_delta!=Vec2::ZERO { for referent in &current_hovered { runtime_events.push(GuiRuntimeEvent{referent:*referent,kind:GuiRuntimeEventKind::InputChanged,position:runtime_position,delta:runtime_delta}); } }
    for referent in current_hovered.difference(hovered_gui) { runtime_events.push(GuiRuntimeEvent{referent:*referent,kind:GuiRuntimeEventKind::MouseEnter,position:runtime_position,delta:runtime_delta}); }
    for referent in hovered_gui.difference(&current_hovered) { runtime_events.push(GuiRuntimeEvent{referent:*referent,kind:GuiRuntimeEventKind::MouseLeave,position:runtime_position,delta:runtime_delta}); }
    *hovered_gui=current_hovered;
    if clicked.is_some() && clicked!=selected {
        if let Some(referent)=selected { runtime_events.push(GuiRuntimeEvent{referent,kind:GuiRuntimeEventKind::SelectionLost,position:runtime_position,delta:runtime_delta}); }
        if let Some(referent)=clicked { runtime_events.push(GuiRuntimeEvent{referent,kind:GuiRuntimeEventKind::SelectionGained,position:runtime_position,delta:runtime_delta}); }
    }
    clicked
}
