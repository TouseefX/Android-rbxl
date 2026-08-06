use egui::{CollapsingHeader, Ui};
use rbx_dom_weak::{WeakDom, types::Ref};

/// Renders the full instance hierarchy, not just scripts. `selected` tracks
/// whichever Ref is currently picked, of any class.
pub fn show_tree(ui: &mut Ui, dom: &WeakDom, root: Ref, selected: &mut Option<Ref>) {
    let Some(inst) = dom.get_by_ref(root) else {
        return;
    };
    let label = format!("{} {}", class_icon(&inst.class), inst.name);

    if inst.children().is_empty() {
        let resp = ui.selectable_label(*selected == Some(root), label);
        if resp.clicked() {
            *selected = Some(root);
        }
    } else {
        let header = CollapsingHeader::new(label)
            .id_salt(root)
            .default_open(false)
            .show(ui, |ui| {
                for child in inst.children() {
                    show_tree(ui, dom, *child, selected);
                }
            });
        if header.header_response.clicked() {
            *selected = Some(root);
        }
    }
}

fn class_icon(class: &str) -> &'static str {
    match class {
        "Script" => "\u{1F4DC}",       // scroll
        "LocalScript" => "\u{1F4C4}",  // page
        "ModuleScript" => "\u{1F4E6}", // package
        "Folder" => "\u{1F4C1}",
        "Workspace" => "\u{1F310}",
        "Players" => "\u{1F464}",
        _ => "\u{25FE}",
    }
}
