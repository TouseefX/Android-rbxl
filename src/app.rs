use crate::jni_bridge::{self, FileEvent};
use crate::{explorer, lua_syntax, rbxl};
use rbx_dom_weak::{
    types::{Color3, Color3uint8, Ref, Variant, Vector3},
    WeakDom,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Explorer,
    ScriptEditor,
    Properties,
    Insert,
}

pub struct EditorApp {
    dom: Option<WeakDom>,
    selected: Option<Ref>,
    buffer: String,
    buffer_original: String,
    current_uri: Option<String>,
    status: String,
    active_tab: ActiveTab,

    // UI state
    explorer_search: String,
    script_search: String,
    font_size: f32,
    show_stats: bool,
    rename_buffer: String,
    new_prop_name: String,
    new_prop_type: String,
    new_prop_val_str: String,
    new_prop_val_num: f64,
    new_prop_val_bool: bool,

    // Maps an opaque id we hand to Java -> which script Ref it belongs to
    pending_external_edits: HashMap<u64, Ref>,
    next_external_id: u64,
}

impl Default for EditorApp {
    fn default() -> Self {
        Self {
            dom: None,
            selected: None,
            buffer: String::new(),
            buffer_original: String::new(),
            current_uri: None,
            status: "No file loaded".into(),
            active_tab: ActiveTab::Explorer,
            explorer_search: String::new(),
            script_search: String::new(),
            font_size: 14.0,
            show_stats: false,
            rename_buffer: String::new(),
            new_prop_name: String::new(),
            new_prop_type: "String".into(),
            new_prop_val_str: String::new(),
            new_prop_val_num: 0.0,
            new_prop_val_bool: false,
            pending_external_edits: HashMap::new(),
            next_external_id: 1,
        }
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();

        let top_frame = egui::Frame::side_top_panel(ui.style())
            .inner_margin(egui::Margin {
                top: 48,
                bottom: 6,
                left: 10,
                right: 10,
            });

        // Top Toolbar
        egui::Panel::top("toolbar")
            .frame(top_frame)
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);

                    if ui.button("📂 Open .rbxl").clicked() {
                        jni_bridge::trigger_open_document();
                    }
                    if ui.button("💾 Save").clicked() {
                        self.save();
                    }
                    if ui.button("💾 Save As...").clicked() {
                        self.save_as();
                    }
                    if ui.button("📊 Stats").clicked() {
                        self.show_stats = !self.show_stats;
                    }
                    ui.separator();
                    ui.label(&self.status);
                });

                if self.show_stats {
                    if let Some(dom) = &self.dom {
                        let stats = rbxl::calculate_stats(dom);
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("📦 Total: {}", stats.total_instances));
                            ui.label(format!("📜 Scripts: {}", stats.scripts_count));
                            ui.label(format!("🧱 Parts: {}", stats.parts_count));
                            ui.label(format!("📦 Models: {}", stats.models_count));
                            ui.label(format!("📱 UI: {}", stats.gui_count));
                        });
                    }
                }
            });

        // Tab Navigation Bar
        let available_width = ui.available_width();
        let is_landscape = available_width > 650.0;

        let nav_frame = egui::Frame::side_top_panel(ui.style())
            .inner_margin(egui::Margin {
                top: 4,
                bottom: 4,
                left: 10,
                right: 10,
            });

        egui::Panel::top("nav_tabs")
            .frame(nav_frame)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(14.0, 8.0);
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);

                    let tab_btn = |ui: &mut egui::Ui, label: &str, tab: ActiveTab, current: ActiveTab| {
                        let is_active = current == tab;
                        let text = if is_active {
                            egui::RichText::new(label).strong().color(egui::Color32::from_rgb(100, 200, 255))
                        } else {
                            egui::RichText::new(label)
                        };
                        ui.selectable_label(is_active, text)
                    };

                    if tab_btn(ui, "📁 Explorer", ActiveTab::Explorer, self.active_tab).clicked() {
                        self.active_tab = ActiveTab::Explorer;
                    }
                    if tab_btn(ui, "📝 Script Editor", ActiveTab::ScriptEditor, self.active_tab).clicked() {
                        self.active_tab = ActiveTab::ScriptEditor;
                    }
                    if tab_btn(ui, "⚙️ Properties", ActiveTab::Properties, self.active_tab).clicked() {
                        self.active_tab = ActiveTab::Properties;
                    }
                    if tab_btn(ui, "➕ Insert", ActiveTab::Insert, self.active_tab).clicked() {
                        self.active_tab = ActiveTab::Insert;
                    }
                });
            });

        // Main Content Area
        if is_landscape {
            // Landscape mode: Two-column split layout
            egui::Panel::left("landscape_left")
                .resizable(true)
                .default_size(280.0)
                .show_inside(ui, |ui| {
                    self.show_explorer_ui(ui);
                });

            egui::CentralPanel::default().show_inside(ui, |ui| {
                match self.active_tab {
                    ActiveTab::Explorer | ActiveTab::ScriptEditor => {
                        self.show_script_editor_ui(ui);
                    }
                    ActiveTab::Properties => {
                        self.show_properties_ui(ui);
                    }
                    ActiveTab::Insert => {
                        self.show_insert_ui(ui);
                    }
                }
            });
        } else {
            // Portrait mode: Full-screen active tab
            egui::CentralPanel::default().show_inside(ui, |ui| {
                match self.active_tab {
                    ActiveTab::Explorer => {
                        self.show_explorer_ui(ui);
                    }
                    ActiveTab::ScriptEditor => {
                        self.show_script_editor_ui(ui);
                    }
                    ActiveTab::Properties => {
                        self.show_properties_ui(ui);
                    }
                    ActiveTab::Insert => {
                        self.show_insert_ui(ui);
                    }
                }
            });
        }
    }
}

impl EditorApp {
    fn show_explorer_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Explorer");
            ui.add(
                egui::TextEdit::singleline(&mut self.explorer_search)
                    .hint_text("🔍 Filter...")
                    .desired_width(120.0),
            );
            if !self.explorer_search.is_empty() && ui.button("✖").clicked() {
                self.explorer_search.clear();
            }
        });

        // Context actions for selected instance
        let selected_info = self.dom.as_ref().and_then(|dom| {
            self.selected.and_then(|r| dom.get_by_ref(r)).map(|inst| inst.name.clone())
        });

        if let Some(name) = selected_info {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(format!("Selected: {name}")).strong());
                
                if self.is_script_selected() && ui.button("📝 Open").clicked() {
                    self.active_tab = ActiveTab::ScriptEditor;
                }
                if ui.button("⚙️ Properties").clicked() {
                    self.active_tab = ActiveTab::Properties;
                }
                if ui.button("📋 Duplicate").clicked() {
                    self.duplicate_selected();
                }
                if ui.button("🗑️ Delete").clicked() {
                    self.delete_selected();
                }
            });
        }

        ui.separator();

        egui::ScrollArea::both()
            .id_salt("explorer_tree_scroll")
            .show(ui, |ui| {
                if let Some(dom) = &self.dom {
                    let root = dom.root_ref();
                    let prev_selected = self.selected;
                    explorer::show_tree_filtered(ui, dom, root, &mut self.selected, &self.explorer_search);
                    if self.selected != prev_selected {
                        if let Some(r) = self.selected {
                            if let Some(src) = rbxl::get_source(dom, r) {
                                self.buffer = src.clone();
                                self.buffer_original = src;
                            } else {
                                self.buffer.clear();
                                self.buffer_original.clear();
                            }
                            if let Some(inst) = dom.get_by_ref(r) {
                                self.rename_buffer = inst.name.clone();
                            }
                        } else {
                            self.buffer.clear();
                            self.buffer_original.clear();
                        }
                    }
                } else {
                    ui.label("Open a place file (.rbxl) using the toolbar above.");
                }
            });
    }

    fn show_script_editor_ui(&mut self, ui: &mut egui::Ui) {
        if !self.is_script_selected() {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.heading("No Script Selected");
                ui.label("Select a Script, LocalScript, or ModuleScript in the Explorer to edit its source.");
                if ui.button("📁 Go to Explorer").clicked() {
                    self.active_tab = ActiveTab::Explorer;
                }
            });
            return;
        }

        let is_dirty = self.buffer != self.buffer_original;

        // Header with Script Name & Action Buttons
        let script_header = self.dom.as_ref().and_then(|dom| {
            self.selected.and_then(|r| dom.get_by_ref(r)).map(|inst| {
                (inst.name.clone(), inst.class.to_string())
            })
        });

        if let Some((name, class)) = script_header {
            ui.horizontal_wrapped(|ui| {
                ui.heading(&name);
                ui.label(egui::RichText::new(format!("({class})")).color(egui::Color32::from_rgb(150, 150, 150)));

                if is_dirty {
                    if ui.button(egui::RichText::new("💾 Apply Changes").color(egui::Color32::from_rgb(120, 255, 120)).strong()).clicked() {
                        self.apply_edit();
                    }
                    if ui.button("↩ Revert").clicked() {
                        self.buffer = self.buffer_original.clone();
                    }
                } else {
                    ui.label(egui::RichText::new("✓ Saved to DOM").color(egui::Color32::from_rgb(120, 200, 120)));
                }

                if ui.button("📱 Edit in External App").clicked() {
                    self.edit_externally();
                }
            });
        }

        // Active External Edit Sync Banner
        if !self.pending_external_edits.is_empty() {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("🟢 External Sync Active").color(egui::Color32::from_rgb(100, 255, 100)).strong());
                if ui.button("🔄 Sync Edits Now").clicked() {
                    jni_bridge::trigger_sync_external_edits();
                }
                if ui.button("✖ Stop External Edit").clicked() {
                    self.pending_external_edits.clear();
                    jni_bridge::trigger_finish_external_edit();
                    self.status = "External editing finished".into();
                }
            });
        }

        ui.separator();

        // Editor Controls Bar (Font Size, Search, Clear)
        ui.horizontal_wrapped(|ui| {
            ui.label("Font:");
            if ui.button("➖").clicked() && self.font_size > 10.0 {
                self.font_size -= 2.0;
            }
            ui.label(format!("{:.0}pt", self.font_size));
            if ui.button("➕").clicked() && self.font_size < 30.0 {
                self.font_size += 2.0;
            }

            ui.separator();

            ui.add(
                egui::TextEdit::singleline(&mut self.script_search)
                    .hint_text("🔍 Search in code...")
                    .desired_width(130.0),
            );
            if !self.script_search.is_empty() {
                let count = self.buffer.to_lowercase().matches(&self.script_search.to_lowercase()).count();
                ui.label(format!("({count} found)"));
                if ui.button("✖").clicked() {
                    self.script_search.clear();
                }
            }

            ui.separator();
            if ui.button("📋 Copy All").clicked() {
                ui.ctx().copy_text(self.buffer.clone());
                self.status = "Copied script to clipboard".into();
            }
        });

        // Quick Lua Symbol Helper Bar (Crucial for mobile touch editing)
        ui.separator();
        egui::ScrollArea::horizontal()
            .id_salt("quick_symbol_scroll")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(6.0, 4.0);
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);

                    let symbols = [
                        ("()", "()"), ("{}", "{}"), ("[]", "[]"), ("\"\"", "\"\""), ("''", "''"),
                        ("=", " = "), ("==", " == "), ("~=", " ~= "), ("<=", " <= "), (">=", " >= "),
                        ("..", " .. "), (":", ":"), (".", "."), (",", ", "), ("->", " -> "), ("::", " :: "),
                        ("local", "local "), ("function", "function "), ("end", "end"),
                        ("then", "then\n\t"), ("do", "do\n\t"), ("return", "return "),
                        ("if", "if "), ("else", "else\n\t"), ("elseif", "elseif "),
                        ("for", "for i, v in pairs() do\n\tend"), ("while", "while true do\n\ttask.wait()\nend"),
                        ("task.wait()", "task.wait()"), ("print()", "print()"),
                        ("game:GetService()", "game:GetService(\"\")"),
                    ];

                    for (label, snippet) in symbols {
                        if ui.button(label).clicked() {
                            self.buffer.push_str(snippet);
                        }
                    }
                });
            });

        ui.separator();

        // Monospace Code Editor Area with Smooth Touch Scroll
        egui::ScrollArea::both()
            .id_salt("code_editor_scroll")
            .show(ui, |ui| {
                let font_size = self.font_size;
                let search_term = if self.script_search.trim().is_empty() {
                    None
                } else {
                    Some(self.script_search.trim())
                };

                let mut layouter = move |ui: &egui::Ui, text_buf: &dyn egui::TextBuffer, _wrap: f32| {
                    let job = lua_syntax::highlight_lua(text_buf.as_str(), font_size, search_term);
                    ui.fonts_mut(|f| f.layout_job(job))
                };

                ui.add(
                    egui::TextEdit::multiline(&mut self.buffer)
                        .id_source("script_multiline_editor")
                        .font(egui::FontId::monospace(self.font_size))
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(25)
                        .lock_focus(true)
                        .layouter(&mut layouter),
                );
            });
    }

    fn show_properties_ui(&mut self, ui: &mut egui::Ui) {
        let (Some(dom), Some(r)) = (&self.dom, self.selected) else {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.heading("No Instance Selected");
                ui.label("Select an object in the Explorer to inspect and edit its properties.");
                if ui.button("📁 Go to Explorer").clicked() {
                    self.active_tab = ActiveTab::Explorer;
                }
            });
            return;
        };

        let Some(inst) = dom.get_by_ref(r) else {
            ui.label("Selected instance no longer exists.");
            return;
        };

        let inst_name = inst.name.clone();
        let inst_class = inst.class.to_string();
        let properties = inst.properties.clone();
        let parent_str = format!("{}", inst.parent());
        let children_count = inst.children().len();

        ui.horizontal(|ui| {
            ui.heading(format!("{} {}", explorer::class_icon(&inst_class), inst_name));
            ui.label(egui::RichText::new(format!("({inst_class})")).color(egui::Color32::from_rgb(150, 150, 150)));
        });

        ui.separator();

        egui::ScrollArea::both()
            .id_salt("properties_scroll")
            .show(ui, |ui| {
                // Rename Section
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.add(egui::TextEdit::singleline(&mut self.rename_buffer).desired_width(150.0));
                    if ui.button("Rename").clicked() {
                        let name = self.rename_buffer.clone();
                        if let Some(dom) = self.dom.as_mut() {
                            let _ = rbxl::rename_instance(dom, r, &name);
                            self.status = format!("Renamed to {name}");
                        }
                    }
                });

                ui.label(format!("Parent: {parent_str}"));
                ui.label(format!("Children: {children_count}"));

                ui.separator();
                ui.heading("Properties");

                // Render properties
                let mut prop_updates = Vec::new();
                let mut prop_deletes = Vec::new();

                for (key, val) in &properties {
                    let key_str = key.as_str();
                    if key_str == "Source" {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Source:").strong());
                            if ui.button("📝 Open in Script Editor").clicked() {
                                self.active_tab = ActiveTab::ScriptEditor;
                            }
                        });
                        continue;
                    }

                    ui.horizontal(|ui| {
                        ui.label(format!("{key_str}:"));

                        match val {
                            Variant::String(s) => {
                                let mut text = s.clone();
                                if ui.text_edit_singleline(&mut text).changed() {
                                    prop_updates.push((key_str.to_string(), Variant::String(text)));
                                }
                            }
                            Variant::Bool(b) => {
                                let mut val_bool = *b;
                                if ui.checkbox(&mut val_bool, "").changed() {
                                    prop_updates.push((key_str.to_string(), Variant::Bool(val_bool)));
                                }
                            }
                            Variant::Float32(f) => {
                                let mut val_f = *f;
                                if ui.add(egui::DragValue::new(&mut val_f).speed(0.1)).changed() {
                                    prop_updates.push((key_str.to_string(), Variant::Float32(val_f)));
                                }
                            }
                            Variant::Float64(f) => {
                                let mut val_f = *f;
                                if ui.add(egui::DragValue::new(&mut val_f).speed(0.1)).changed() {
                                    prop_updates.push((key_str.to_string(), Variant::Float64(val_f)));
                                }
                            }
                            Variant::Int32(i) => {
                                let mut val_i = *i;
                                if ui.add(egui::DragValue::new(&mut val_i).speed(1)).changed() {
                                    prop_updates.push((key_str.to_string(), Variant::Int32(val_i)));
                                }
                            }
                            Variant::Int64(i) => {
                                let mut val_i = *i;
                                if ui.add(egui::DragValue::new(&mut val_i).speed(1)).changed() {
                                    prop_updates.push((key_str.to_string(), Variant::Int64(val_i)));
                                }
                            }
                            Variant::Vector3(v) => {
                                let mut x = v.x;
                                let mut y = v.y;
                                let mut z = v.z;
                                ui.label("X");
                                let cx = ui.add(egui::DragValue::new(&mut x).speed(0.2)).changed();
                                ui.label("Y");
                                let cy = ui.add(egui::DragValue::new(&mut y).speed(0.2)).changed();
                                ui.label("Z");
                                let cz = ui.add(egui::DragValue::new(&mut z).speed(0.2)).changed();
                                if cx || cy || cz {
                                    prop_updates.push((key_str.to_string(), Variant::Vector3(Vector3::new(x, y, z))));
                                }
                            }
                            Variant::Color3(c) => {
                                let mut rgb = [c.r, c.g, c.b];
                                if ui.color_edit_button_rgb(&mut rgb).changed() {
                                    prop_updates.push((key_str.to_string(), Variant::Color3(Color3::new(rgb[0], rgb[1], rgb[2]))));
                                }
                            }
                            Variant::Color3uint8(c) => {
                                let mut srgb = [c.r, c.g, c.b];
                                if ui.color_edit_button_srgb(&mut srgb).changed() {
                                    prop_updates.push((key_str.to_string(), Variant::Color3uint8(Color3uint8::new(srgb[0], srgb[1], srgb[2]))));
                                }
                            }
                            _ => {
                                ui.label(format!("{val:?}"));
                            }
                        }

                        if ui.small_button("🗑").clicked() {
                            prop_deletes.push(key_str.to_string());
                        }
                    });
                }

                // Apply property edits
                if let Some(dom) = self.dom.as_mut() {
                    for (k, v) in prop_updates {
                        let _ = rbxl::set_property(dom, r, &k, v);
                    }
                    for k in prop_deletes {
                        let _ = rbxl::delete_property(dom, r, &k);
                    }
                }

                // Add Property Section
                ui.separator();
                ui.heading("➕ Add Property / Value");
                ui.horizontal_wrapped(|ui| {
                    ui.label("Name:");
                    ui.add(egui::TextEdit::singleline(&mut self.new_prop_name).desired_width(100.0));

                    egui::ComboBox::from_id_salt("prop_type_combo")
                        .selected_text(&self.new_prop_type)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.new_prop_type, "String".into(), "String");
                            ui.selectable_value(&mut self.new_prop_type, "Number".into(), "Number");
                            ui.selectable_value(&mut self.new_prop_type, "Bool".into(), "Bool");
                        });

                    match self.new_prop_type.as_str() {
                        "String" => {
                            ui.add(egui::TextEdit::singleline(&mut self.new_prop_val_str).hint_text("Value").desired_width(100.0));
                        }
                        "Number" => {
                            ui.add(egui::DragValue::new(&mut self.new_prop_val_num).speed(0.1));
                        }
                        "Bool" => {
                            ui.checkbox(&mut self.new_prop_val_bool, "Value");
                        }
                        _ => {}
                    }

                    if ui.button("Add").clicked() && !self.new_prop_name.trim().is_empty() {
                        let name = self.new_prop_name.trim().to_string();
                        let variant = match self.new_prop_type.as_str() {
                            "Number" => Variant::Float64(self.new_prop_val_num),
                            "Bool" => Variant::Bool(self.new_prop_val_bool),
                            _ => Variant::String(self.new_prop_val_str.clone()),
                        };

                        if let Some(dom) = self.dom.as_mut() {
                            let _ = rbxl::set_property(dom, r, &name, variant);
                            self.status = format!("Added property {name}");
                            self.new_prop_name.clear();
                        }
                    }
                });
            });
    }

    fn show_insert_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("➕ Insert Roblox Object");
        ui.label("Inserts the selected object into the currently selected instance (or Workspace).");

        ui.separator();

        let categories = [
            ("🧱 3D World", vec![
                ("Part", "Part"),
                ("Model", "Model"),
                ("Folder", "Folder"),
                ("SpawnLocation", "SpawnLocation"),
                ("TrussPart", "TrussPart"),
                ("WedgePart", "WedgePart"),
                ("CornerWedgePart", "CornerWedgePart"),
            ]),
            ("📜 Scripting", vec![
                ("Script", "Script"),
                ("LocalScript", "LocalScript"),
                ("ModuleScript", "ModuleScript"),
            ]),
            ("📡 Networking", vec![
                ("RemoteEvent", "RemoteEvent"),
                ("RemoteFunction", "RemoteFunction"),
                ("BindableEvent", "BindableEvent"),
                ("BindableFunction", "BindableFunction"),
            ]),
            ("📱 GUI & UI", vec![
                ("ScreenGui", "ScreenGui"),
                ("Frame", "Frame"),
                ("TextLabel", "TextLabel"),
                ("TextButton", "TextButton"),
                ("TextBox", "TextBox"),
                ("ImageLabel", "ImageLabel"),
                ("ImageButton", "ImageButton"),
                ("ScrollingFrame", "ScrollingFrame"),
            ]),
            ("🔢 Values", vec![
                ("StringValue", "StringValue"),
                ("IntValue", "IntValue"),
                ("NumberValue", "NumberValue"),
                ("BoolValue", "BoolValue"),
                ("Color3Value", "Color3Value"),
                ("Vector3Value", "Vector3Value"),
                ("ObjectValue", "ObjectValue"),
            ]),
            ("✨ Effects & Lighting", vec![
                ("PointLight", "PointLight"),
                ("SpotLight", "SpotLight"),
                ("SurfaceLight", "SurfaceLight"),
                ("ParticleEmitter", "ParticleEmitter"),
                ("Highlight", "Highlight"),
                ("ProximityPrompt", "ProximityPrompt"),
                ("Sound", "Sound"),
                ("Attachment", "Attachment"),
            ]),
        ];

        egui::ScrollArea::both()
            .id_salt("insert_objects_scroll")
            .show(ui, |ui| {
                for (cat_name, items) in categories {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(cat_name).heading().color(egui::Color32::from_rgb(100, 200, 255)));

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);

                        for (class, label) in items {
                            let btn_text = format!("{} {}", explorer::class_icon(class), label);
                            if ui.button(btn_text).clicked() {
                                self.insert_class(class, label);
                            }
                        }
                    });
                }
            });
    }

    fn insert_class(&mut self, class: &str, name: &str) {
        let Some(dom) = self.dom.as_mut() else {
            self.status = "Open a place file first".into();
            return;
        };

        let parent = self.selected.unwrap_or_else(|| dom.root_ref());
        match rbxl::add_instance(dom, parent, class, name) {
            Ok(new_ref) => {
                self.selected = Some(new_ref);
                if matches!(class, "Script" | "LocalScript" | "ModuleScript") {
                    self.buffer.clear();
                    self.buffer_original.clear();
                    self.active_tab = ActiveTab::ScriptEditor;
                } else {
                    self.active_tab = ActiveTab::Properties;
                }
                self.status = format!("Inserted {class} '{name}'");
            }
            Err(e) => {
                self.status = format!("Insert failed: {e}");
            }
        }
    }

    fn duplicate_selected(&mut self) {
        let (Some(dom), Some(r)) = (self.dom.as_mut(), self.selected) else {
            return;
        };
        match rbxl::duplicate_instance(dom, r) {
            Ok(new_ref) => {
                self.selected = Some(new_ref);
                self.status = "Duplicated instance".into();
            }
            Err(e) => self.status = format!("Duplicate failed: {e}"),
        }
    }

    fn delete_selected(&mut self) {
        let (Some(dom), Some(r)) = (self.dom.as_mut(), self.selected) else {
            return;
        };
        match rbxl::delete_instance(dom, r) {
            Ok(_) => {
                self.selected = None;
                self.buffer.clear();
                self.buffer_original.clear();
                self.status = "Deleted instance".into();
            }
            Err(e) => self.status = format!("Delete failed: {e}"),
        }
    }

    fn is_script_selected(&self) -> bool {
        let (Some(dom), Some(r)) = (&self.dom, self.selected) else {
            return false;
        };
        dom.get_by_ref(r).is_some_and(|i| {
            matches!(i.class.as_str(), "Script" | "LocalScript" | "ModuleScript")
                || i.properties.contains_key(&rbx_dom_weak::Ustr::from("Source"))
        })
    }

    fn drain_events(&mut self) {
        for event in jni_bridge::try_recv_all() {
            match event {
                FileEvent::Opened { uri, data } => match rbxl::load_place(data) {
                    Ok(dom) => {
                        let count = dom.root().children().len();
                        self.dom = Some(dom);
                        self.current_uri = Some(uri);
                        self.selected = None;
                        self.buffer.clear();
                        self.buffer_original.clear();
                        self.status = format!("Loaded ({count} top-level services)");
                        self.active_tab = ActiveTab::Explorer;
                    }
                    Err(e) => self.status = format!("Failed to parse: {e}"),
                },
                FileEvent::OpenCancelled => self.status = "Open cancelled".into(),
                FileEvent::Created { uri } => {
                    self.current_uri = Some(uri);
                    self.save();
                }
                FileEvent::SaveComplete(ok) => {
                    self.status = if ok { "Saved place successfully".into() } else { "Save failed".into() };
                }
                FileEvent::ExternalEditReturned { script_id, text } => {
                    if let Some(referent) = self.pending_external_edits.get(&script_id).copied() {
                        if let Some(dom) = self.dom.as_mut() {
                            let _ = rbxl::set_source(dom, referent, text.clone());
                            if self.selected == Some(referent) {
                                self.buffer = text.clone();
                                self.buffer_original = text;
                            }
                            self.status = "⚡ Synced changes from external editor".into();
                        }
                    }
                }
            }
        }
    }

    fn apply_edit(&mut self) {
        if let (Some(dom), Some(r)) = (self.dom.as_mut(), self.selected) {
            let _ = rbxl::set_source(dom, r, self.buffer.clone());
            self.buffer_original = self.buffer.clone();
            self.status = "Edit applied to DOM (Save to write to disk)".into();
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
            self.status = "No file open yet - use Open .rbxl first".into();
            return;
        };
        match rbxl::save_place(dom) {
            Ok(bytes) => jni_bridge::trigger_save(&bytes),
            Err(e) => self.status = format!("Serialize failed: {e}"),
        }
    }

    fn save_as(&mut self) {
        if self.dom.is_none() {
            self.status = "No place file open to save".into();
            return;
        }
        jni_bridge::trigger_create_document("place.rbxl");
    }
}
