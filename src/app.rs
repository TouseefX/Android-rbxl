use crate::asset_downloader::{self, DiscoveredAsset};
use crate::jni_bridge::{self, FileEvent};
use crate::{explorer, lua_syntax, rbxl, templates, viewport3d::{CameraPreset, Viewport3D}};
use egui::{Color32, RichText};
use rbx_dom_weak::{
    types::{Color3, Color3uint8, Ref, Variant, Vector3},
    WeakDom,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Explorer,
    Viewport3D,
    ScriptEditor,
    Properties,
    Insert,
    Toolbox,
    Snippets,
    Assets,
    Output,
}

pub struct OpenScriptTab {
    pub referent: Ref,
    pub name: String,
    pub class: String,
    pub buffer: String,
    pub original: String,
}

pub struct OutputLog {
    pub level: &'static str,
    pub message: String,
    pub time: String,
}

pub struct EditorApp {
    dom: Option<WeakDom>,
    selected: Option<Ref>,
    current_uri: Option<String>,
    status: String,
    active_tab: ActiveTab,

    // Multi-tab Script Editor
    open_tabs: Vec<OpenScriptTab>,
    active_script_idx: usize,

    // Find & Replace
    find_term: String,
    replace_term: String,
    show_replace: bool,

    // UI state
    explorer_search: String,
    font_size: f32,
    show_stats: bool,
    rename_buffer: String,

    // 3D Viewport
    viewport: Viewport3D,

    // Asset & Mesh Downloader State
    manual_asset_input: String,
    discovered_assets: Vec<DiscoveredAsset>,

    // Properties UI State
    new_prop_name: String,
    new_prop_type: String,
    new_prop_val_str: String,
    new_prop_val_num: f64,
    new_prop_val_bool: bool,

    // Output Logs
    output_logs: Vec<OutputLog>,

    // External edit mapping
    pending_external_edits: HashMap<u64, Ref>,
    next_external_id: u64,
}

impl Default for EditorApp {
    fn default() -> Self {
        let mut app = Self {
            dom: None,
            selected: None,
            current_uri: None,
            status: "Ready - Open a .rbxl file to begin".into(),
            active_tab: ActiveTab::Explorer,
            open_tabs: Vec::new(),
            active_script_idx: 0,
            find_term: String::new(),
            replace_term: String::new(),
            show_replace: false,
            explorer_search: String::new(),
            font_size: 14.0,
            show_stats: false,
            rename_buffer: String::new(),
            viewport: Viewport3D::default(),
            manual_asset_input: String::new(),
            discovered_assets: Vec::new(),
            new_prop_name: String::new(),
            new_prop_type: "String".into(),
            new_prop_val_str: String::new(),
            new_prop_val_num: 0.0,
            new_prop_val_bool: false,
            output_logs: Vec::new(),
            pending_external_edits: HashMap::new(),
            next_external_id: 1,
        };
        app.log_info("Roblox Studio Lite initialized");
        app
    }
}

impl EditorApp {
    fn log_info(&mut self, msg: impl Into<String>) {
        self.output_logs.push(OutputLog {
            level: "INFO",
            message: msg.into(),
            time: "now".into(),
        });
    }

    fn log_error(&mut self, msg: impl Into<String>) {
        self.output_logs.push(OutputLog {
            level: "ERROR",
            message: msg.into(),
            time: "now".into(),
        });
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();

        // Custom Roblox Studio Lite Theme styling
        ui.style_mut().visuals.panel_fill = Color32::from_rgb(30, 30, 30);
        ui.style_mut().visuals.window_fill = Color32::from_rgb(37, 37, 38);

        let top_frame = egui::Frame::side_top_panel(ui.style())
            .inner_margin(egui::Margin {
                top: 48,
                bottom: 6,
                left: 10,
                right: 10,
            });

        // Top Studio Toolbar
        egui::Panel::top("toolbar")
            .frame(top_frame)
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);

                    if ui.button(RichText::new("📂 Open .rbxl").strong()).clicked() {
                        jni_bridge::trigger_open_document();
                    }
                    if ui.button(RichText::new("💾 Save").strong().color(Color32::from_rgb(100, 255, 120))).clicked() {
                        self.save();
                    }
                    if ui.button("💾 Save As...").clicked() {
                        self.save_as();
                    }
                    if ui.button("📊 Stats").clicked() {
                        self.show_stats = !self.show_stats;
                    }
                    if ui.button(format!("🖥️ Output ({})", self.output_logs.len())).clicked() {
                        self.active_tab = ActiveTab::Output;
                    }
                    ui.separator();
                    ui.label(&self.status);
                });

                if self.show_stats {
                    if let Some(dom) = &self.dom {
                        let stats = rbxl::calculate_stats(dom);
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("📦 Total Instances: {}", stats.total_instances));
                            ui.label(format!("📜 Scripts: {}", stats.scripts_count));
                            ui.label(format!("🧱 Parts: {}", stats.parts_count));
                            ui.label(format!("📦 Models: {}", stats.models_count));
                            ui.label(format!("📱 UI Elements: {}", stats.gui_count));
                        });
                    }
                }
            });

        // Studio Navigation Tabs Bar
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
                egui::ScrollArea::horizontal()
                    .id_salt("nav_tabs_scroll")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().button_padding = egui::vec2(12.0, 7.0);
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);

                            let mut tab_btn = |ui: &mut egui::Ui, label: &str, tab: ActiveTab| {
                                let is_active = self.active_tab == tab;
                                let text = if is_active {
                                    RichText::new(label).strong().color(Color32::from_rgb(100, 200, 255))
                                } else {
                                    RichText::new(label)
                                };
                                if ui.selectable_label(is_active, text).clicked() {
                                    self.active_tab = tab;
                                }
                            };

                            tab_btn(ui, "📁 Explorer", ActiveTab::Explorer);
                            tab_btn(ui, "🌍 3D Viewport", ActiveTab::Viewport3D);
                            tab_btn(ui, &format!("📝 Scripts ({})", self.open_tabs.len()), ActiveTab::ScriptEditor);
                            tab_btn(ui, "⚙️ Properties", ActiveTab::Properties);
                            tab_btn(ui, "➕ Insert", ActiveTab::Insert);
                            tab_btn(ui, "🧰 Toolbox", ActiveTab::Toolbox);
                            tab_btn(ui, "📜 Snippets", ActiveTab::Snippets);
                            tab_btn(ui, "☁️ Cloud Assets", ActiveTab::Assets);
                            tab_btn(ui, "🖥️ Output", ActiveTab::Output);
                        });
                    });
            });

        // Main Studio Work Area
        if is_landscape {
            // Landscape layout: Explorer on left, active view on right
            egui::Panel::left("landscape_left")
                .resizable(true)
                .default_size(280.0)
                .show_inside(ui, |ui| {
                    self.show_explorer_ui(ui);
                });

            egui::CentralPanel::default().show_inside(ui, |ui| {
                match self.active_tab {
                    ActiveTab::Explorer | ActiveTab::ScriptEditor => self.show_script_editor_ui(ui),
                    ActiveTab::Viewport3D => self.show_viewport_ui(ui),
                    ActiveTab::Properties => self.show_properties_ui(ui),
                    ActiveTab::Insert => self.show_insert_ui(ui),
                    ActiveTab::Toolbox => self.show_toolbox_ui(ui),
                    ActiveTab::Snippets => self.show_snippets_ui(ui),
                    ActiveTab::Assets => self.show_assets_ui(ui),
                    ActiveTab::Output => self.show_output_ui(ui),
                }
            });
        } else {
            // Portrait layout: Full-width dedicated tab view
            egui::CentralPanel::default().show_inside(ui, |ui| {
                match self.active_tab {
                    ActiveTab::Explorer => self.show_explorer_ui(ui),
                    ActiveTab::Viewport3D => self.show_viewport_ui(ui),
                    ActiveTab::ScriptEditor => self.show_script_editor_ui(ui),
                    ActiveTab::Properties => self.show_properties_ui(ui),
                    ActiveTab::Insert => self.show_insert_ui(ui),
                    ActiveTab::Toolbox => self.show_toolbox_ui(ui),
                    ActiveTab::Snippets => self.show_snippets_ui(ui),
                    ActiveTab::Assets => self.show_assets_ui(ui),
                    ActiveTab::Output => self.show_output_ui(ui),
                }
            });
        }
    }
}

impl EditorApp {
    fn show_viewport_ui(&mut self, ui: &mut egui::Ui) {
        // Camera Toolbar & Presets
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(8.0, 5.0);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);

            if ui.button("🎯 Focus Selected").clicked() {
                if let (Some(dom), Some(r)) = (&self.dom, self.selected) {
                    if let Some(inst) = dom.get_by_ref(r) {
                        if let Some(Variant::Vector3(v)) = inst.properties.get(&rbx_dom_weak::ustr("Position")) {
                            self.viewport.focus_on([v.x, v.y, v.z]);
                            self.status = format!("Camera focused on {}", inst.name);
                        }
                    }
                }
            }

            if ui.button("📐 Iso").clicked() {
                self.viewport.set_preset(CameraPreset::Isometric);
            }
            if ui.button("📐 Top").clicked() {
                self.viewport.set_preset(CameraPreset::Top);
            }
            if ui.button("📐 Front").clicked() {
                self.viewport.set_preset(CameraPreset::Front);
            }
            if ui.button("📐 Side").clicked() {
                self.viewport.set_preset(CameraPreset::Side);
            }

            ui.separator();

            if ui.button("🔍+ Zoom In").clicked() && self.viewport.distance > 8.0 {
                self.viewport.distance -= 6.0;
            }
            if ui.button("🔍- Zoom Out").clicked() && self.viewport.distance < 300.0 {
                self.viewport.distance += 6.0;
            }
            if ui.button("🔄 Reset Cam").clicked() {
                self.viewport = Viewport3D::default();
            }

            ui.checkbox(&mut self.viewport.show_grid, "Grid");
            ui.checkbox(&mut self.viewport.show_wireframe, "Wireframe");
        });

        // Fly / Pan Movement Controls for Mobile
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Camera Move:").strong().color(Color32::from_rgb(100, 200, 255)));

            if ui.button("⬆️ Forward").clicked() {
                self.viewport.move_forward();
            }
            if ui.button("⬇️ Back").clicked() {
                self.viewport.move_backward();
            }
            if ui.button("⬅️ Left").clicked() {
                self.viewport.move_left();
            }
            if ui.button("➡️ Right").clicked() {
                self.viewport.move_right();
            }
            if ui.button("🔼 Up").clicked() {
                self.viewport.move_up();
            }
            if ui.button("🔽 Down").clicked() {
                self.viewport.move_down();
            }

            ui.label("Speed:");
            ui.add(egui::DragValue::new(&mut self.viewport.move_speed).speed(0.5).range(0.5..=20.0));
        });

        // 3D Transform Gizmo bar for selected part
        let selected_part_data = self.dom.as_ref().and_then(|dom| {
            self.selected.and_then(|r| dom.get_by_ref(r)).and_then(|inst| {
                let is_part = matches!(
                    inst.class.as_str(),
                    "Part" | "WedgePart" | "CornerWedgePart" | "TrussPart" | "SpawnLocation"
                );
                if is_part {
                    let pos = match inst.properties.get(&rbx_dom_weak::ustr("Position")) {
                        Some(Variant::Vector3(v)) => [v.x, v.y, v.z],
                        _ => [0.0, 0.0, 0.0],
                    };
                    let size = match inst.properties.get(&rbx_dom_weak::ustr("Size")) {
                        Some(Variant::Vector3(v)) => [v.x, v.y, v.z],
                        _ => [4.0, 1.2, 2.0],
                    };
                    Some((inst.name.clone(), pos, size))
                } else {
                    None
                }
            })
        });

        if let Some((name, pos, size)) = selected_part_data {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("Selected 3D: {name}")).strong().color(Color32::from_rgb(0, 220, 255)));

                let mut pos_change = None;
                let mut size_change = None;

                let mut x = pos[0];
                let mut y = pos[1];
                let mut z = pos[2];

                ui.label("Pos X:");
                let cx = ui.add(egui::DragValue::new(&mut x).speed(0.5)).changed();
                ui.label("Y:");
                let cy = ui.add(egui::DragValue::new(&mut y).speed(0.5)).changed();
                ui.label("Z:");
                let cz = ui.add(egui::DragValue::new(&mut z).speed(0.5)).changed();

                if cx || cy || cz {
                    pos_change = Some(Vector3::new(x, y, z));
                }

                let mut sx = size[0];
                let mut sy = size[1];
                let mut sz = size[2];

                ui.label("Size X:");
                let csx = ui.add(egui::DragValue::new(&mut sx).speed(0.5).range(0.2..=500.0)).changed();
                ui.label("Y:");
                let csy = ui.add(egui::DragValue::new(&mut sy).speed(0.5).range(0.2..=500.0)).changed();
                ui.label("Z:");
                let csz = ui.add(egui::DragValue::new(&mut sz).speed(0.5).range(0.2..=500.0)).changed();

                if csx || csy || csz {
                    size_change = Some(Vector3::new(sx, sy, sz));
                }

                if let Some(r) = self.selected {
                    if let Some(dom) = self.dom.as_mut() {
                        if let Some(p) = pos_change {
                            let _ = rbxl::set_property(dom, r, "Position", Variant::Vector3(p));
                        }
                        if let Some(s) = size_change {
                            let _ = rbxl::set_property(dom, r, "Size", Variant::Vector3(s));
                        }
                    }
                }
            });
        }

        ui.separator();

        // Render the 3D Interactive Viewport
        self.viewport.render(ui, self.dom.as_ref(), &mut self.selected);
    }

    fn show_assets_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("☁️ Roblox Creator Store & Asset Delivery");
        ui.label("Search creator store models, framework libraries, and extract mesh / texture delivery URLs.");

        ui.separator();

        // Creator Store Search
        ui.label(RichText::new("Creator Store Search:").strong().color(Color32::from_rgb(100, 200, 255)));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.manual_asset_input)
                    .hint_text("Search: Knit, Sword, Roact, ProfileService, Fusion, Coil...")
                    .desired_width(240.0),
            );

            if !self.manual_asset_input.is_empty() && ui.button("✖ Clear").clicked() {
                self.manual_asset_input.clear();
            }
        });

        ui.separator();

        // Creator Store Catalog Grid
        let search_results = crate::roblox_api::RobloxApiClient::search_creator_store(&self.manual_asset_input);

        egui::ScrollArea::both()
            .id_salt("creator_store_scroll")
            .show(ui, |ui| {
                ui.label(RichText::new(format!("Roblox Creator Store Items ({})", search_results.len())).heading());

                for item in &search_results {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&item.name).heading().color(Color32::from_rgb(100, 200, 255)));
                            ui.label(RichText::new(format!("by {}", item.creator_name)).color(Color32::from_rgb(160, 160, 160)));
                            ui.label(RichText::new(format!("[{}]", item.item_type)).color(Color32::from_rgb(200, 200, 100)));
                        });

                        ui.label(&item.description);

                        ui.horizontal_wrapped(|ui| {
                            if ui.button(RichText::new("📥 Insert into Place Workspace").color(Color32::from_rgb(120, 255, 120)).strong()).clicked() {
                                if let Some(dom) = self.dom.as_mut() {
                                    let parent = self.selected.unwrap_or_else(|| dom.root_ref());
                                    match crate::roblox_api::RobloxApiClient::insert_asset_into_place(dom, parent, item) {
                                        Ok(new_ref) => {
                                            self.selected = Some(new_ref);
                                            self.status = format!("Inserted '{}' into place", item.name);
                                            self.log_info(format!("Inserted '{}' (ID: {})", item.name, item.id));
                                        }
                                        Err(e) => {
                                            self.status = format!("Insert error: {e}");
                                            self.log_error(format!("Insert error: {e}"));
                                        }
                                    }
                                } else {
                                    self.status = "Open a place file first".into();
                                }
                            }

                            let delivery_url = crate::roblox_api::RobloxApiClient::get_asset_delivery_url(item.id);
                            if ui.button("📋 Copy Delivery URL").clicked() {
                                jni_bridge::trigger_copy_to_clipboard(&delivery_url);
                                self.status = format!("Copied asset delivery URL for ID {}", item.id);
                            }

                            if ui.button("🌐 Copy Asset ID").clicked() {
                                jni_bridge::trigger_copy_to_clipboard(&item.id.to_string());
                                self.status = format!("Copied ID {}", item.id);
                            }
                        });
                    });
                    ui.add_space(6.0);
                }

                // Place Scanner Section
                ui.separator();
                ui.label(RichText::new("Local Place Asset Scanner").heading().color(Color32::from_rgb(100, 200, 255)));

                if ui.button("🔍 Scan Active Place for MeshId / TextureId").clicked() {
                    if let Some(dom) = &self.dom {
                        self.discovered_assets = asset_downloader::scan_place_assets(dom);
                        self.status = format!("Found {} unique assets in place", self.discovered_assets.len());
                        self.log_info(format!("Discovered {} place assets", self.discovered_assets.len()));
                    } else {
                        self.status = "Open a place file first".into();
                    }
                }

                if !self.discovered_assets.is_empty() {
                    ui.add_space(4.0);
                    for asset in &self.discovered_assets {
                        ui.group(|ui| {
                            let icon = match asset.asset_type {
                                "Mesh" => "🧱",
                                "Texture" => "🖼️",
                                "Sound" => "🔊",
                                "Animation" => "🏃",
                                _ => "📦",
                            };

                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{icon} [{}] ID: {}", asset.asset_type, asset.asset_id)).strong().color(Color32::from_rgb(100, 200, 255)));
                                ui.label(format!("In: {} ({})", asset.instance_name, asset.instance_class));
                            });

                            let download_url = format!("https://assetdelivery.roblox.com/v1/asset/?id={}", asset.asset_id);
                            ui.horizontal(|ui| {
                                if ui.button("📋 Copy Asset ID").clicked() {
                                    jni_bridge::trigger_copy_to_clipboard(&asset.asset_id);
                                }
                                if ui.button("🌐 Copy Delivery URL").clicked() {
                                    jni_bridge::trigger_copy_to_clipboard(&download_url);
                                }
                            });
                        });
                        ui.add_space(4.0);
                    }
                }
            });
    }

    fn show_explorer_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Explorer");
            ui.add(
                egui::TextEdit::singleline(&mut self.explorer_search)
                    .hint_text("🔍 Filter tree...")
                    .desired_width(120.0),
            );
            if !self.explorer_search.is_empty() && ui.button("✖").clicked() {
                self.explorer_search.clear();
            }
        });

        // Context actions for selected instance
        let selected_info = self.dom.as_ref().and_then(|dom| {
            self.selected.and_then(|r| dom.get_by_ref(r)).map(|inst| (inst.name.clone(), inst.class.to_string()))
        });

        if let Some((name, class)) = selected_info {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("Selected: {name}")).strong().color(Color32::from_rgb(100, 200, 255)));
                ui.label(format!("({class})"));

                if self.is_script_selected() && ui.button("📝 Edit Script").clicked() {
                    if let Some(r) = self.selected {
                        self.open_script_tab(r);
                    }
                    self.active_tab = ActiveTab::ScriptEditor;
                }
                if ui.button("⚙️ Properties").clicked() {
                    self.active_tab = ActiveTab::Properties;
                }
                if ui.button("🌍 View 3D").clicked() {
                    self.active_tab = ActiveTab::Viewport3D;
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
                            if let Some(inst) = dom.get_by_ref(r) {
                                self.rename_buffer = inst.name.clone();
                                if matches!(inst.class.as_str(), "Script" | "LocalScript" | "ModuleScript")
                                    || inst.properties.contains_key(&rbx_dom_weak::Ustr::from("Source"))
                                {
                                    self.open_script_tab(r);
                                }
                            }
                        }
                    }
                } else {
                    ui.label("Open a place file (.rbxl) using the toolbar above.");
                }
            });
    }

    fn open_script_tab(&mut self, referent: Ref) {
        let Some(dom) = &self.dom else { return };
        let Some(inst) = dom.get_by_ref(referent) else { return };

        // Check if tab already open
        if let Some(pos) = self.open_tabs.iter().position(|t| t.referent == referent) {
            self.active_script_idx = pos;
            return;
        }

        let source = rbxl::get_source(dom, referent).unwrap_or_default();
        self.open_tabs.push(OpenScriptTab {
            referent,
            name: inst.name.clone(),
            class: inst.class.to_string(),
            buffer: source.clone(),
            original: source,
        });
        self.active_script_idx = self.open_tabs.len() - 1;
    }

    fn show_script_editor_ui(&mut self, ui: &mut egui::Ui) {
        if self.open_tabs.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.heading("No Scripts Open");
                ui.label("Select a Script, LocalScript, or ModuleScript in the Explorer to edit.");
                if ui.button("📁 Go to Explorer").clicked() {
                    self.active_tab = ActiveTab::Explorer;
                }
            });
            return;
        }

        // Script Tabs Header
        let mut close_tab_idx = None;
        egui::ScrollArea::horizontal()
            .id_salt("script_tabs_scroll")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);

                    for (idx, tab) in self.open_tabs.iter().enumerate() {
                        let is_active = idx == self.active_script_idx;
                        let is_dirty = tab.buffer != tab.original;
                        let icon = explorer::class_icon(&tab.class);
                        let title = if is_dirty {
                            format!("{icon} {} *", tab.name)
                        } else {
                            format!("{icon} {}", tab.name)
                        };

                        let text = if is_active {
                            RichText::new(title).strong().color(Color32::from_rgb(100, 200, 255))
                        } else {
                            RichText::new(title)
                        };

                        if ui.selectable_label(is_active, text).clicked() {
                            self.active_script_idx = idx;
                            self.selected = Some(tab.referent);
                        }

                        if ui.small_button("✖").clicked() {
                            close_tab_idx = Some(idx);
                        }
                    }
                });
            });

        if let Some(idx) = close_tab_idx {
            self.open_tabs.remove(idx);
            if self.active_script_idx >= self.open_tabs.len() && !self.open_tabs.is_empty() {
                self.active_script_idx = self.open_tabs.len() - 1;
            }
            if self.open_tabs.is_empty() {
                return;
            }
        }

        let tab = &mut self.open_tabs[self.active_script_idx];
        let is_dirty = tab.buffer != tab.original;
        let tab_ref = tab.referent;
        let tab_name = tab.name.clone();

        ui.separator();

        // Action Toolbar
        ui.horizontal_wrapped(|ui| {
            ui.heading(&tab_name);
            ui.label(RichText::new(format!("({})", tab.class)).color(Color32::from_rgb(150, 150, 150)));

            if is_dirty {
                if ui.button(RichText::new("💾 Apply Changes").color(Color32::from_rgb(120, 255, 120)).strong()).clicked() {
                    if let Some(dom) = self.dom.as_mut() {
                        let _ = rbxl::set_source(dom, tab_ref, tab.buffer.clone());
                        tab.original = tab.buffer.clone();
                        self.status = format!("Applied edits to {}", tab_name);
                    }
                }
                if ui.button("↩ Revert").clicked() {
                    tab.buffer = tab.original.clone();
                }
            } else {
                ui.label(RichText::new("✓ Up to date").color(Color32::from_rgb(120, 200, 120)));
            }

            if ui.button("📱 Edit in External App").clicked() {
                let id = self.next_external_id;
                self.next_external_id += 1;
                self.pending_external_edits.insert(id, tab_ref);
                jni_bridge::trigger_edit_externally(id, &tab_name, &tab.buffer);
            }

            if ui.button("🔍 Find & Replace").clicked() {
                self.show_replace = !self.show_replace;
            }
        });

        // External Edit Sync Banner
        if self.pending_external_edits.values().any(|&r| r == tab_ref) {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("🟢 External Sync Active").color(Color32::from_rgb(100, 255, 100)).strong());
                if ui.button("🔄 Sync Edits Now").clicked() {
                    jni_bridge::trigger_sync_external_edits();
                }
                if ui.button("✖ Finish External Edit").clicked() {
                    self.pending_external_edits.retain(|_, &mut r| r != tab_ref);
                    jni_bridge::trigger_finish_external_edit();
                    self.status = "External edit finished".into();
                }
            });
        }

        // Find & Replace Panel
        if self.show_replace {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label("Find:");
                ui.add(egui::TextEdit::singleline(&mut self.find_term).desired_width(120.0));
                ui.label("Replace:");
                ui.add(egui::TextEdit::singleline(&mut self.replace_term).desired_width(120.0));

                if !self.find_term.is_empty() {
                    let count = tab.buffer.matches(&self.find_term).count();
                    ui.label(format!("({count} matches)"));

                    if ui.button("Replace All").clicked() {
                        tab.buffer = tab.buffer.replace(&self.find_term, &self.replace_term);
                    }
                }
            });
        }

        // Font Size & Quick Copy
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label("Font:");
            if ui.button("➖").clicked() && self.font_size > 10.0 {
                self.font_size -= 2.0;
            }
            ui.label(format!("{:.0}pt", self.font_size));
            if ui.button("➕").clicked() && self.font_size < 32.0 {
                self.font_size += 2.0;
            }

            ui.separator();
            if ui.button("📋 Copy Script").clicked() {
                jni_bridge::trigger_copy_to_clipboard(&tab.buffer);
                self.status = "Copied script to Android clipboard".into();
            }
        });

        // Quick Lua Symbol Bar
        ui.separator();
        egui::ScrollArea::horizontal()
            .id_salt("quick_symbols_editor")
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
                            tab.buffer.push_str(snippet);
                        }
                    }
                });
            });

        ui.separator();

        // Monospace Code Editor Area
        let font_size = self.font_size;
        let search_term = if self.find_term.trim().is_empty() {
            None
        } else {
            Some(self.find_term.trim().to_string())
        };

        egui::ScrollArea::both()
            .id_salt("code_scroll_area")
            .show(ui, |ui| {
                let search_ref = search_term.as_deref();
                let mut layouter = move |ui: &egui::Ui, text_buf: &dyn egui::TextBuffer, _wrap: f32| {
                    let job = lua_syntax::highlight_lua(text_buf.as_str(), font_size, search_ref);
                    ui.fonts_mut(|f| f.layout_job(job))
                };

                ui.add(
                    egui::TextEdit::multiline(&mut tab.buffer)
                        .id_source("script_multiline_view")
                        .font(egui::FontId::monospace(self.font_size))
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(28)
                        .lock_focus(true)
                        .layouter(&mut layouter),
                );
            });
    }

    fn show_properties_ui(&mut self, ui: &mut egui::Ui) {
        let (Some(dom), Some(r)) = (&self.dom, self.selected) else {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.heading("No Object Selected");
                ui.label("Select any instance in the Explorer to inspect and edit its properties.");
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
            ui.label(RichText::new(format!("({inst_class})")).color(Color32::from_rgb(150, 150, 150)));
        });

        ui.separator();

        egui::ScrollArea::both()
            .id_salt("properties_scroll_view")
            .show(ui, |ui| {
                // Section: Instance Data
                ui.label(RichText::new("Data").heading().color(Color32::from_rgb(100, 200, 255)));
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.add(egui::TextEdit::singleline(&mut self.rename_buffer).desired_width(140.0));
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
                ui.label(RichText::new("Properties").heading().color(Color32::from_rgb(100, 200, 255)));

                let mut prop_updates = Vec::new();
                let mut prop_deletes = Vec::new();

                for (key, val) in &properties {
                    let key_str = key.as_str();
                    if key_str == "Source" {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Source:").strong());
                            if ui.button("📝 Open in Script Editor").clicked() {
                                self.open_script_tab(r);
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
                ui.label(RichText::new("➕ Add Property / Value").heading().color(Color32::from_rgb(100, 200, 255)));
                ui.horizontal_wrapped(|ui| {
                    ui.label("Name:");
                    ui.add(egui::TextEdit::singleline(&mut self.new_prop_name).desired_width(100.0));

                    egui::ComboBox::from_id_salt("prop_type_select")
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
                    ui.label(RichText::new(cat_name).heading().color(Color32::from_rgb(100, 200, 255)));

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

    fn show_toolbox_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("🧰 Roblox Toolbox & Game Systems");
        ui.label("Insert pre-built game systems and boilerplate models directly into your place.");

        ui.separator();

        egui::ScrollArea::both()
            .id_salt("toolbox_scroll_view")
            .show(ui, |ui| {
                for preset in templates::TOOLBOX_PRESETS {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{} {}", preset.icon, preset.name)).heading().color(Color32::from_rgb(100, 200, 255)));
                            ui.label(RichText::new(format!("({})", preset.category)).color(Color32::from_rgb(150, 150, 150)));
                        });
                        ui.label(preset.description);

                        if ui.button("➕ Insert System").clicked() {
                            self.insert_toolbox_preset(preset);
                        }
                    });
                    ui.add_space(6.0);
                }
            });
    }

    fn insert_toolbox_preset(&mut self, preset: &templates::ToolboxPreset) {
        let Some(dom) = self.dom.as_mut() else {
            self.status = "Open a place file first".into();
            return;
        };

        let parent = self.selected.unwrap_or_else(|| dom.root_ref());
        match rbxl::add_instance(dom, parent, preset.class, preset.name) {
            Ok(new_ref) => {
                if let Some((script_name, script_code)) = preset.default_script {
                    let _ = rbxl::add_instance(dom, new_ref, "Script", script_name);
                    let _ = rbxl::set_source(dom, new_ref, script_code.to_string());
                }
                self.selected = Some(new_ref);
                self.status = format!("Inserted toolbox system '{}'", preset.name);
                self.log_info(format!("Inserted preset {}", preset.name));
            }
            Err(e) => {
                self.status = format!("Insert preset failed: {e}");
            }
        }
    }

    fn show_snippets_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("📜 Luau Script Snippets");
        ui.label("Insert boilerplate code and industry standard systems into your active script.");

        ui.separator();

        egui::ScrollArea::both()
            .id_salt("snippets_scroll_view")
            .show(ui, |ui| {
                for snippet in templates::SNIPPETS {
                    ui.group(|ui| {
                        ui.label(RichText::new(snippet.name).heading().color(Color32::from_rgb(100, 200, 255)));
                        ui.label(snippet.description);

                        ui.horizontal(|ui| {
                            if ui.button("📥 Insert into Active Script").clicked() {
                                if !self.open_tabs.is_empty() {
                                    let tab = &mut self.open_tabs[self.active_script_idx];
                                    if !tab.buffer.ends_with('\n') && !tab.buffer.is_empty() {
                                        tab.buffer.push('\n');
                                    }
                                    tab.buffer.push_str(snippet.code);
                                    self.active_tab = ActiveTab::ScriptEditor;
                                    self.status = format!("Inserted snippet {}", snippet.name);
                                } else {
                                    self.status = "Open a script first".into();
                                }
                            }
                            if ui.button("📋 Copy Snippet").clicked() {
                                jni_bridge::trigger_copy_to_clipboard(snippet.code);
                                self.status = format!("Copied {} to Android clipboard", snippet.name);
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
            });
    }

    fn show_output_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("🖥️ Studio Output Console");
            if ui.button("Clear Output").clicked() {
                self.output_logs.clear();
            }
        });

        ui.separator();

        egui::ScrollArea::both()
            .id_salt("output_console_scroll")
            .show(ui, |ui| {
                if self.output_logs.is_empty() {
                    ui.label("No output logs yet. Errors, warnings, and place operations will appear here.");
                }

                for log in &self.output_logs {
                    let color = match log.level {
                        "ERROR" => Color32::from_rgb(255, 100, 100),
                        "WARN" => Color32::from_rgb(255, 200, 100),
                        _ => Color32::from_rgb(200, 220, 240),
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("[{}]", log.level)).color(color).strong());
                        ui.label(&log.message);
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
                    self.open_script_tab(new_ref);
                    self.active_tab = ActiveTab::ScriptEditor;
                } else if matches!(class, "Part" | "WedgePart" | "CornerWedgePart" | "TrussPart" | "SpawnLocation") {
                    self.active_tab = ActiveTab::Viewport3D;
                } else {
                    self.active_tab = ActiveTab::Properties;
                }
                self.status = format!("Inserted {class} '{name}'");
                self.log_info(format!("Inserted {class} '{name}'"));
            }
            Err(e) => {
                self.status = format!("Insert failed: {e}");
                self.log_error(format!("Insert failed: {e}"));
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
                self.log_info("Duplicated instance");
            }
            Err(e) => {
                self.status = format!("Duplicate failed: {e}");
                self.log_error(format!("Duplicate failed: {e}"));
            }
        }
    }

    fn delete_selected(&mut self) {
        let (Some(dom), Some(r)) = (self.dom.as_mut(), self.selected) else {
            return;
        };
        match rbxl::delete_instance(dom, r) {
            Ok(_) => {
                self.open_tabs.retain(|t| t.referent != r);
                self.selected = None;
                self.status = "Deleted instance".into();
                self.log_info("Deleted instance");
            }
            Err(e) => {
                self.status = format!("Delete failed: {e}");
                self.log_error(format!("Delete failed: {e}"));
            }
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
                        self.current_uri = Some(uri.clone());
                        self.selected = None;
                        self.open_tabs.clear();
                        self.status = format!("Loaded ({count} top-level services)");
                        self.log_info(format!("Opened place: {uri} ({count} services)"));
                        self.active_tab = ActiveTab::Explorer;
                    }
                    Err(e) => {
                        self.status = format!("Failed to parse: {e}");
                        self.log_error(format!("Failed to parse rbxl: {e}"));
                    }
                },
                FileEvent::OpenCancelled => {
                    self.status = "Open cancelled".into();
                }
                FileEvent::Created { uri } => {
                    self.current_uri = Some(uri);
                    self.save();
                }
                FileEvent::SaveComplete(ok) => {
                    self.status = if ok { "Saved place successfully".into() } else { "Save failed".into() };
                    if ok {
                        self.log_info("Saved place successfully");
                    } else {
                        self.log_error("Save failed");
                    }
                }
                FileEvent::ExternalEditReturned { script_id, text } => {
                    if let Some(referent) = self.pending_external_edits.get(&script_id).copied() {
                        if let Some(dom) = self.dom.as_mut() {
                            let _ = rbxl::set_source(dom, referent, text.clone());
                            if let Some(tab) = self.open_tabs.iter_mut().find(|t| t.referent == referent) {
                                tab.buffer = text.clone();
                                tab.original = text;
                            }
                            self.status = "⚡ Synced edits from external app".into();
                            self.log_info("Synced script from external editor");
                        }
                    }
                }
            }
        }
    }

    fn save(&mut self) {
        let (Some(dom), Some(_uri)) = (&self.dom, &self.current_uri) else {
            self.status = "No file open yet - use Open .rbxl first".into();
            return;
        };
        match rbxl::save_place(dom) {
            Ok(bytes) => jni_bridge::trigger_save(&bytes),
            Err(e) => {
                self.status = format!("Serialize failed: {e}");
                self.log_error(format!("Serialize failed: {e}"));
            }
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
