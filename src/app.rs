use crate::asset_downloader::{self, DiscoveredAsset};
use crate::jni_bridge::{self, FileEvent};
use crate::roblox_api::{self, LiveCatalogItem, RobloxApiClient};
use crate::{explorer, lua_syntax, rbxl, schema, templates, viewport3d::{CameraPreset, Viewport3D}};
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
    OpenCloud,
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

    // Live Roblox Catalog & Creator Store State
    live_search_input: String,
    live_catalog_items: Vec<LiveCatalogItem>,
    is_searching_live: bool,
    discovered_assets: Vec<DiscoveredAsset>,

    // Open Cloud State
    open_cloud_api_key: String,
    open_cloud_universe_id: String,
    open_cloud_place_id: String,
    datastore_name_input: String,
    datastore_key_input: String,
    datastore_val_input: String,
    datastore_response_text: String,
    messaging_topic_input: String,
    messaging_msg_input: String,

    // Schema Class Search
    schema_search_input: String,

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
            live_search_input: "sword".into(),
            live_catalog_items: Vec::new(),
            is_searching_live: false,
            discovered_assets: Vec::new(),
            open_cloud_api_key: String::new(),
            open_cloud_universe_id: String::new(),
            open_cloud_place_id: String::new(),
            datastore_name_input: "PlayerData".into(),
            datastore_key_input: "Player_12345".into(),
            datastore_val_input: "{\"Coins\": 1000, \"Level\": 10}".into(),
            datastore_response_text: String::new(),
            messaging_topic_input: "ServerAlerts".into(),
            messaging_msg_input: "Server updating in 5 minutes".into(),
            schema_search_input: String::new(),
            new_prop_name: String::new(),
            new_prop_type: "String".into(),
            new_prop_val_str: String::new(),
            new_prop_val_num: 0.0,
            new_prop_val_bool: false,
            output_logs: Vec::new(),
            pending_external_edits: HashMap::new(),
            next_external_id: 1,
        };
        app.log_info("Roblox Studio Lite initialized with Open Cloud & Live Creator Store");
        RobloxApiClient::fetch_live_catalog_async("sword".into());
        app.is_searching_live = true;
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
                            tab_btn(ui, "☁️ Creator Store", ActiveTab::Assets);
                            tab_btn(ui, "🚀 Open Cloud", ActiveTab::OpenCloud);
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
                    ActiveTab::OpenCloud => self.show_open_cloud_ui(ui),
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
                    ActiveTab::OpenCloud => self.show_open_cloud_ui(ui),
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
        ui.heading("☁️ Roblox Creator Store Live Search Engine");
        ui.label("Live real-time query against official Roblox Catalog & Creator Store marketplace.");

        ui.separator();

        // Live Search Input Bar
        ui.horizontal(|ui| {
            ui.label(RichText::new("Search:").strong());
            ui.add(
                egui::TextEdit::singleline(&mut self.live_search_input)
                    .hint_text("Enter query e.g. sword, car, gun, tree, door, knit, fusion...")
                    .desired_width(200.0),
            );

            if ui.button(RichText::new("🔍 Search Live Store").strong().color(Color32::from_rgb(100, 200, 255))).clicked() {
                if !self.live_search_input.trim().is_empty() {
                    RobloxApiClient::fetch_live_catalog_async(self.live_search_input.trim().to_string());
                    self.is_searching_live = true;
                    self.status = format!("Searching Roblox Catalog for '{}'...", self.live_search_input);
                    self.log_info(format!("Querying Roblox Catalog API: '{}'", self.live_search_input));
                }
            }

            if self.is_searching_live {
                ui.spinner();
            }
        });

        ui.separator();

        let items = self.live_catalog_items.clone();
        let mut insert_item: Option<LiveCatalogItem> = None;

        // Live Creator Store Results List
        egui::ScrollArea::both()
            .id_salt("creator_store_results_scroll")
            .show(ui, |ui| {
                if items.is_empty() {
                    ui.label("Type a keyword above and tap '🔍 Search Live Store' to query millions of Roblox assets.");
                } else {
                    ui.label(RichText::new(format!("Marketplace Items Found ({})", items.len())).heading());

                    for item in &items {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&item.name).heading().color(Color32::from_rgb(100, 200, 255)));
                                ui.label(RichText::new(format!("by {}", item.creator_name)).color(Color32::from_rgb(160, 160, 160)));
                                if item.upvotes > 0 {
                                    ui.label(format!("👍 {} ({}%)", item.upvotes, item.upvote_percent));
                                }
                            });

                            if !item.description.is_empty() {
                                ui.label(&item.description);
                            }

                            ui.horizontal_wrapped(|ui| {
                                if ui.button(RichText::new("📥 Insert into Place Workspace").color(Color32::from_rgb(120, 255, 120)).strong()).clicked() {
                                    insert_item = Some(item.clone());
                                }

                                let delivery_url = RobloxApiClient::get_asset_delivery_url(item.id);
                                if ui.button("📋 Copy Delivery URL").clicked() {
                                    jni_bridge::trigger_copy_to_clipboard(&delivery_url);
                                }

                                if ui.button("🌐 Copy Asset ID").clicked() {
                                    jni_bridge::trigger_copy_to_clipboard(&item.id.to_string());
                                }
                            });
                        });
                        ui.add_space(4.0);
                    }
                }
            });

        if let Some(item) = insert_item {
            if let Some(dom) = self.dom.as_mut() {
                let parent = self.selected.unwrap_or_else(|| dom.root_ref());
                match RobloxApiClient::insert_live_item_into_place(dom, parent, &item) {
                    Ok(new_ref) => {
                        self.selected = Some(new_ref);
                        self.status = format!("Inserted '{}' into Workspace", item.name);
                        self.log_info(format!("Inserted live asset '{}' (ID: {})", item.name, item.id));
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
    }

    fn show_open_cloud_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("🚀 Roblox Open Cloud Suite");
        ui.label("Publish active places directly to live Roblox Universe servers, inspect DataStores, and send Messages.");

        ui.separator();

        egui::ScrollArea::both()
            .id_salt("open_cloud_scroll_view")
            .show(ui, |ui| {
                // Section 1: Authentication & Universe Config
                ui.group(|ui| {
                    ui.label(RichText::new("🔑 Open Cloud Authentication & Target").heading().color(Color32::from_rgb(100, 200, 255)));
                    ui.horizontal(|ui| {
                        ui.label("API Key:");
                        ui.add(egui::TextEdit::singleline(&mut self.open_cloud_api_key).password(true).desired_width(180.0));
                        if ui.button("📥 Paste").clicked() {
                            let text = jni_bridge::get_clipboard_text();
                            if !text.is_empty() {
                                self.open_cloud_api_key = text;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Universe ID:");
                        ui.add(egui::TextEdit::singleline(&mut self.open_cloud_universe_id).hint_text("e.g. 123456789").desired_width(110.0));
                        if ui.button("📥").clicked() {
                            let text = jni_bridge::get_clipboard_text();
                            if !text.is_empty() {
                                self.open_cloud_universe_id = text;
                            }
                        }
                        ui.label("Place ID:");
                        ui.add(egui::TextEdit::singleline(&mut self.open_cloud_place_id).hint_text("e.g. 987654321").desired_width(110.0));
                        if ui.button("📥").clicked() {
                            let text = jni_bridge::get_clipboard_text();
                            if !text.is_empty() {
                                self.open_cloud_place_id = text;
                            }
                        }
                    });
                });

                ui.add_space(8.0);

                // Section 2: Direct Place Publishing
                ui.group(|ui| {
                    ui.label(RichText::new("🚀 Publish Active Place to Live Universe").heading().color(Color32::from_rgb(120, 255, 120)));
                    ui.label("Serializes the active .rbxl DOM in memory and POSTs it directly to the Open Cloud Publishing API.");

                    if ui.button(RichText::new("⚡ Publish Place Now").strong().color(Color32::from_rgb(100, 255, 120))).clicked() {
                        if let Some(dom) = &self.dom {
                            match rbxl::save_place(dom) {
                                Ok(bytes) => {
                                    self.log_info("Serializing place for Open Cloud publish...");
                                    match RobloxApiClient::publish_place_open_cloud(
                                        &self.open_cloud_api_key,
                                        &self.open_cloud_universe_id,
                                        &self.open_cloud_place_id,
                                        &bytes,
                                    ) {
                                        Ok(res) => {
                                            self.status = "Place published successfully via Open Cloud!".into();
                                            self.log_info(format!("Publish success: {res}"));
                                        }
                                        Err(e) => {
                                            self.status = format!("Publish error: {e}");
                                            self.log_error(format!("Open Cloud publish error: {e}"));
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.status = format!("Serialization error: {e}");
                                    self.log_error(format!("Serialization error: {e}"));
                                }
                            }
                        } else {
                            self.status = "Open a place file first".into();
                        }
                    }
                });

                ui.add_space(8.0);

                // Section 3: Open Cloud DataStore Inspector
                ui.group(|ui| {
                    ui.label(RichText::new("🗄️ Live DataStore Explorer").heading().color(Color32::from_rgb(255, 200, 100)));
                    ui.horizontal(|ui| {
                        ui.label("DataStore Name:");
                        ui.add(egui::TextEdit::singleline(&mut self.datastore_name_input).desired_width(120.0));
                        ui.label("Key:");
                        ui.add(egui::TextEdit::singleline(&mut self.datastore_key_input).desired_width(120.0));
                    });

                    ui.horizontal(|ui| {
                        if ui.button("🔍 Read Key").clicked() {
                            match RobloxApiClient::get_datastore_entry(
                                &self.open_cloud_api_key,
                                &self.open_cloud_universe_id,
                                &self.datastore_name_input,
                                &self.datastore_key_input,
                            ) {
                                Ok(res) => {
                                    self.datastore_response_text = res;
                                    self.status = "DataStore key retrieved".into();
                                    self.log_info("Retrieved DataStore key");
                                }
                                Err(e) => {
                                    self.datastore_response_text = format!("Error: {e}");
                                    self.log_error(format!("DataStore error: {e}"));
                                }
                            }
                        }

                        if ui.button("💾 Set / Write Key").clicked() {
                            match RobloxApiClient::set_datastore_entry(
                                &self.open_cloud_api_key,
                                &self.open_cloud_universe_id,
                                &self.datastore_name_input,
                                &self.datastore_key_input,
                                &self.datastore_val_input,
                            ) {
                                Ok(res) => {
                                    self.datastore_response_text = res;
                                    self.status = "DataStore key updated".into();
                                    self.log_info("Updated DataStore key");
                                }
                                Err(e) => {
                                    self.datastore_response_text = format!("Error: {e}");
                                    self.log_error(format!("DataStore write error: {e}"));
                                }
                            }
                        }
                    });

                    ui.label("JSON Payload / Value:");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.datastore_val_input)
                            .desired_width(f32::INFINITY)
                            .desired_rows(3),
                    );

                    if !self.datastore_response_text.is_empty() {
                        ui.label("Response Output:");
                        ui.label(RichText::new(&self.datastore_response_text).monospace().color(Color32::from_rgb(180, 220, 255)));
                    }
                });

                ui.add_space(8.0);

                // Section 4: MessagingService Live Dispatcher
                ui.group(|ui| {
                    ui.label(RichText::new("📡 MessagingService Live Dispatcher").heading().color(Color32::from_rgb(180, 100, 255)));
                    ui.horizontal(|ui| {
                        ui.label("Topic:");
                        ui.add(egui::TextEdit::singleline(&mut self.messaging_topic_input).desired_width(140.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Message:");
                        ui.add(egui::TextEdit::singleline(&mut self.messaging_msg_input).desired_width(180.0));
                    });

                    if ui.button("📤 Send Cross-Server Message").clicked() {
                        match RobloxApiClient::publish_message_topic(
                            &self.open_cloud_api_key,
                            &self.open_cloud_universe_id,
                            &self.messaging_topic_input,
                            &self.messaging_msg_input,
                        ) {
                            Ok(res) => {
                                self.status = "Message dispatched to Roblox servers".into();
                                self.log_info(format!("Dispatched topic message: {res}"));
                            }
                            Err(e) => {
                                self.status = format!("Message error: {e}");
                                self.log_error(format!("Message error: {e}"));
                            }
                        }
                    }
                });
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
            if ui.button("📥 Paste from Clipboard").clicked() {
                let text = jni_bridge::get_clipboard_text();
                if !text.is_empty() {
                    tab.buffer.push_str(&text);
                    self.status = format!("Pasted {} characters from Android clipboard", text.len());
                } else {
                    self.status = "Clipboard is empty".into();
                }
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

                // Add Property Section with Reflection Schema Types
                ui.separator();
                ui.label(RichText::new("➕ Add Schema Property / Value").heading().color(Color32::from_rgb(100, 200, 255)));
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
        ui.heading("➕ Insert Roblox Object (1,000+ Engine Classes)");
        ui.label("Search any official class from Roblox Engine Reflection and insert into Workspace.");

        ui.separator();

        // Engine Class Search Input
        ui.horizontal(|ui| {
            ui.label(RichText::new("Search Engine Schema:").strong());
            ui.add(
                egui::TextEdit::singleline(&mut self.schema_search_input)
                    .hint_text("e.g. Highlight, Beam, ParticleEmitter, Atmosphere, Sound, Part...")
                    .desired_width(200.0),
            );

            if !self.schema_search_input.is_empty() && ui.button("✖").clicked() {
                self.schema_search_input.clear();
            }
        });

        ui.separator();

        let schema_results = schema::search_engine_classes(&self.schema_search_input);

        egui::ScrollArea::both()
            .id_salt("schema_insert_scroll")
            .show(ui, |ui| {
                ui.label(RichText::new(format!("Roblox Engine Classes ({})", schema_results.len())).heading().color(Color32::from_rgb(100, 200, 255)));

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);

                    for cls in schema_results.iter().take(40) {
                        let btn_text = format!("{} {}", explorer::class_icon(&cls.name), cls.name);
                        if ui.button(btn_text).clicked() {
                            self.insert_class(&cls.name, &cls.name);
                        }
                    }
                });

                if schema_results.len() > 40 {
                    ui.add_space(8.0);
                    ui.label(format!("... and {} more classes. Use search above to narrow down.", schema_results.len() - 40));
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
        match schema::create_instance_from_schema(dom, parent, class, name) {
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
        // Check for live search results from background thread
        if let Some(resp) = roblox_api::try_recv_search_results() {
            self.is_searching_live = false;
            let count = resp.items.len();
            self.live_catalog_items = resp.items;
            if let Some(err) = resp.error {
                self.log_error(err);
            } else {
                self.status = format!("Found {count} live items for '{}'", resp.query);
                self.log_info(format!("Received {count} live catalog items for '{}'", resp.query));
            }
        }

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
