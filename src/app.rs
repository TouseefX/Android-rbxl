use crate::jni_bridge::{self, FileEvent};
use crate::{explorer, lua_syntax, rbxl};
use egui_code_editor::CodeEditor;
use rbx_dom_weak::{types::Ref, WeakDom};
use std::collections::HashMap;

pub struct EditorApp {
    dom: Option<WeakDom>,
    selected: Option<Ref>,
    buffer: String,
    current_uri: Option<String>,
    status: String,

    // Maps an opaque id we hand to Java -> which script Ref it belongs to,
    // since Java has no concept of a rbx_dom_weak::Ref.
    pending_external_edits: HashMap<u64, Ref>,
    next_external_id: u64,
}

impl Default for EditorApp {
    fn default() -> Self {
        Self {
            dom: None,
            selected: None,
            buffer: String::new(),
            current_uri: None,
            status: "No file loaded".into(),
            pending_external_edits: HashMap::new(),
            next_external_id: 1,
        }
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();

        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open .rbxl").clicked() {
                    jni_bridge::trigger_open_document();
                }
                if ui.button("Save").clicked() {
                    self.save();
                }
                if ui.button("+ New Script").clicked() {
                    self.add_new_script();
                }
                if self.is_script_selected() && ui.button("Edit externally").clicked() {
                    self.edit_externally();
                }
                ui.label(&self.status);
            });
        });

        egui::Panel::left("explorer").show_inside(ui, |ui| {
            ui.heading("Explorer");
            if let Some(dom) = &self.dom {
                let root = dom.root_ref();
                explorer::show_tree(ui, dom, root, &mut self.selected);
            } else {
                ui.label("Open a place file to browse it.");
            }
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.is_script_selected() {
                CodeEditor::default()
                    .id_source("script_editor")
                    .with_rows(30)
                    .with_fontsize(14.0)
                    .with_syntax(lua_syntax::luau_syntax())
                    .with_numlines(true)
                    .vscroll(true)
                    .show(ui, &mut self.buffer);

                if ui.button("Apply to script").clicked() {
                    self.apply_edit();
                }
            } else {
                ui.label("Select a script in the Explorer, or open a place file.");
            }
        });
    }
}

impl EditorApp {
    fn is_script_selected(&self) -> bool {
        let (Some(dom), Some(r)) = (&self.dom, self.selected) else {
            return false;
        };
        dom.get_by_ref(r)
            .is_some_and(|i| matches!(i.class.as_str(), "Script" | "LocalScript" | "ModuleScript"))
    }

    fn drain_events(&mut self) {
        for event in jni_bridge::try_recv_all() {
            match event {
                FileEvent::Opened { uri, data } => match rbxl::load_place(data) {
                    Ok(dom) => {
                        self.dom = Some(dom);
                        self.current_uri = Some(uri);
                        self.selected = None;
                        self.status = "Loaded".into();
                    }
                    Err(e) => self.status = format!("Failed to parse: {e}"),
                },
                FileEvent::OpenCancelled => self.status = "Open cancelled".into(),
                FileEvent::Created { uri } => {
                    self.current_uri = Some(uri);
                    self.save();
                }
                FileEvent::SaveComplete(ok) => {
                    self.status = if ok { "Saved".into() } else { "Save failed".into() };
                }
                FileEvent::ExternalEditReturned { script_id, text } => {
                    if let Some(referent) = self.pending_external_edits.remove(&script_id) {
                        if let Some(dom) = self.dom.as_mut() {
                            let _ = rbxl::set_source(dom, referent, text.clone());
                            if self.selected == Some(referent) {
                                self.buffer = text;
                            }
                            self.status = "Updated from external edit".into();
                        }
                    }
                }
            }
        }
    }

    fn apply_edit(&mut self) {
        if let (Some(dom), Some(r)) = (self.dom.as_mut(), self.selected) {
            let _ = rbxl::set_source(dom, r, self.buffer.clone());
            self.status = "Edit applied (not yet saved to disk)".into();
        }
    }

    fn add_new_script(&mut self) {
        if let Some(dom) = self.dom.as_mut() {
            let root = dom.root_ref();
            if let Ok(new_ref) = rbxl::add_script(dom, root, "Script", "NewScript", String::new()) {
                self.selected = Some(new_ref);
                self.buffer.clear();
            }
        }
    }

    fn edit_externally(&mut self) {
        let (Some(dom), Some(r)) = (&self.dom, self.selected) else {
            return;
        };
        let Some(inst) = dom.get_by_ref(r) else { return };
        let id = self.next_external_id;
        self.next_external_id += 1;
        self.pending_external_edits.insert(id, r);
        jni_bridge::trigger_edit_externally(id, &inst.name, &self.buffer);
    }

    fn save(&mut self) {
        let (Some(dom), Some(_uri)) = (&self.dom, &self.current_uri) else {
            self.status = "No file open yet — use Open or + New Script first".into();
            return;
        };
        match rbxl::save_place(dom) {
            Ok(bytes) => jni_bridge::trigger_save(&bytes),
            Err(e) => self.status = format!("Serialize failed: {e}"),
        }
    }
}
