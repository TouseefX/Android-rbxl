use crate::asset_downloader::{self, DiscoveredAsset};
use crate::bevy_render::OrbitCam;
use crate::jni_bridge::{self, FileEvent};
use crate::roblox_api::{self, LiveCatalogItem, RobloxApiClient};
use crate::{explorer, lua_syntax, rbxl, schema, templates};
use bevy_egui::egui;
use bevy_egui::egui::{Color32, RichText};
use rbx_dom_weak::{
    types::{Color3, Color3uint8, Ref, Variant, Vector3},
    WeakDom,
};
use crate::settings::EditorSettings;
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
    Settings,
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

#[derive(bevy::prelude::Resource)]
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

    // Live Roblox Catalog & Creator Store State
    live_search_input: String,
    live_catalog_items: Vec<LiveCatalogItem>,
    is_searching_live: bool,
    direct_asset_id_input: String,
    roblosecurity_cookie: String,
    discovered_assets: Vec<DiscoveredAsset>,

    // Open Cloud State
    open_cloud_api_key: String,
    open_cloud_universe_id: String,
    open_cloud_place_id: String,
    open_cloud_publish_live: bool,
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

    // Bevy 3D scene rebuild flag: set when the opened place changes, cleared
    // by the Bevy system that (re)builds the meshes.
    needs_3d_rebuild: bool,

    // 3D viewport camera move speed (studs/step for Up/Down/pan).
    cam_move_speed: f32,
}

impl Default for EditorApp {
    fn default() -> Self {
        let saved_settings = EditorSettings::load();

        let mut app = Self {
            dom: None,
            selected: None,
            current_uri: None,
            status: "Ready - Open a .rbxl file to begin".into(),
            // Open straight to the 3D viewport so the scene + camera controls
            // are visible immediately.
            active_tab: ActiveTab::Viewport3D,
            open_tabs: Vec::new(),
            active_script_idx: 0,
            find_term: String::new(),
            replace_term: String::new(),
            show_replace: false,
            explorer_search: String::new(),
            font_size: 14.0,
            show_stats: false,
            rename_buffer: String::new(),
            live_search_input: "sword".into(),
            live_catalog_items: Vec::new(),
            is_searching_live: false,
            direct_asset_id_input: "47433".into(),
            roblosecurity_cookie: saved_settings.roblosecurity_cookie,
            discovered_assets: Vec::new(),
            open_cloud_api_key: saved_settings.open_cloud_api_key,
            open_cloud_universe_id: saved_settings.open_cloud_universe_id,
            open_cloud_place_id: saved_settings.open_cloud_place_id,
            open_cloud_publish_live: true,
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
            needs_3d_rebuild: false,
            cam_move_speed: 4.0,
        };
        app.log_info("Roblox Studio Lite initialized with persistent settings");
        RobloxApiClient::fetch_live_catalog_async("sword".into());
        app.is_searching_live = true;
        app
    }
}

impl EditorApp {
    /// Whether the Bevy 3D scene needs rebuilding (a place was just opened).
    pub fn take_3d_rebuild(&mut self) -> bool {
        let v = self.needs_3d_rebuild;
        self.needs_3d_rebuild = false;
        v
    }

    /// The currently loaded place (if any), for the Bevy scene builder.
    pub fn dom(&self) -> Option<&WeakDom> {
        self.dom.as_ref()
    }

    /// Load a place from raw file bytes (used at startup / desktop validation).
    pub fn load_from_bytes(&mut self, bytes: Vec<u8>) {
        match rbxl::load_place(bytes) {
            Ok(dom) => {
                self.dom = Some(dom);
                self.selected = None;
                self.needs_3d_rebuild = true;
                self.status = "Loaded".into();
            }
            Err(e) => {
                self.status = format!("Failed to parse: {e}");
            }
        }
    }

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

impl EditorApp {
    /// Render the whole editor UI into the bevy_egui `egui::Context`. `orbit`
    /// is the Bevy viewport camera, steered by the 3D tab. Runs each frame from
    /// a Bevy system.
    pub fn draw_editor(&mut self, ctx: &egui::Context, orbit: &mut OrbitCam) {
        self.drain_events();

        // Custom Roblox Studio Lite Theme styling
        ctx.style_mut(|style| {
            style.visuals.panel_fill = Color32::from_rgb(30, 30, 30);
            style.visuals.window_fill = Color32::from_rgb(37, 37, 38);
        });

        let style = ctx.style();

        let top_frame = egui::Frame::side_top_panel(&style).inner_margin(egui::Margin {
            top: 48,
            bottom: 6,
            left: 10,
            right: 10,
        });

        // Top Studio Toolbar
        egui::TopBottomPanel::top("toolbar")
            .frame(top_frame)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);

                    ui.label(RichText::new("v3.2-B8 • OpenRBLX • Bevy").strong().color(Color32::from_rgb(0, 230, 255)));

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
        let is_landscape = ctx.available_rect().width() > 650.0;

        let nav_frame = egui::Frame::side_top_panel(&style).inner_margin(egui::Margin {
            top: 4,
            bottom: 4,
            left: 10,
            right: 10,
        });

        egui::TopBottomPanel::top("nav_tabs")
            .frame(nav_frame)
            .show(ctx, |ui| {
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
                            tab_btn(ui, "⚙️ Settings", ActiveTab::Settings);
                        });
                    });
            });

        // Landscape: Explorer on the left.
        if is_landscape {
            egui::SidePanel::left("landscape_left")
                .resizable(true)
                .default_width(280.0)
                .show(ctx, |ui| {
                    self.show_explorer_ui(ui);
                });
        }

        // Main work area.
        if self.active_tab == ActiveTab::Viewport3D {
            // SOLID top control bar (guaranteed to render over the 3D): camera
            // presets, distance/zoom, speed, up/down.
            egui::TopBottomPanel::top("viewport_controls")
                .show(ctx, |ui| {
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        self.show_viewport_controls(ui, orbit);
                    });
                });
            // Transparent central panel: the Bevy 3D scene shows through and
            // this region senses drag/scroll to orbit the camera.
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
                .show(ctx, |ui| {
                    self.show_viewport_drag(ui, orbit);
                });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                match self.active_tab {
                    ActiveTab::Explorer => self.show_explorer_ui(ui),
                    ActiveTab::Viewport3D => {
                        self.show_viewport_controls(ui, orbit);
                        self.show_viewport_drag(ui, orbit);
                    }
                    ActiveTab::ScriptEditor => self.show_script_editor_ui(ui),
                    ActiveTab::Properties => self.show_properties_ui(ui),
                    ActiveTab::Insert => self.show_insert_ui(ui),
                    ActiveTab::Toolbox => self.show_toolbox_ui(ui),
                    ActiveTab::Snippets => self.show_snippets_ui(ui),
                    ActiveTab::Assets => self.show_assets_ui(ui),
                    ActiveTab::OpenCloud => self.show_open_cloud_ui(ui),
                    ActiveTab::Output => self.show_output_ui(ui),
                    ActiveTab::Settings => self.show_settings_ui(ui),
                }
            });
        }
    }
}

impl EditorApp {
    /// Camera control bar (always drawn on a solid panel so it's visible over
    /// the 3D). Steers the Bevy `OrbitCam`.
    fn show_viewport_controls(&mut self, ui: &mut egui::Ui, orbit: &mut crate::bevy_render::OrbitCam) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(8.0, 5.0);
            ui.label(RichText::new("🧊 3D (Bevy)").strong().color(Color32::from_rgb(0, 230, 255)));

            if ui.button("📐 Iso").clicked() { orbit.yaw = 0.785; orbit.pitch = 0.45; }
            if ui.button("📐 Top").clicked() { orbit.yaw = 0.0; orbit.pitch = 1.54; }
            if ui.button("📐 Front").clicked() { orbit.yaw = 0.0; orbit.pitch = 0.15; }
            if ui.button("📐 Side").clicked() { orbit.yaw = std::f32::consts::PI * 0.5; orbit.pitch = 0.15; }
            if ui.button("🎯 Focus Sel").clicked() {
                if let (Some(dom), Some(r)) = (&self.dom, self.selected) {
                    if let Some(inst) = dom.get_by_ref(r) {
                        if let Some(Variant::Vector3(v)) = inst.properties.get(&rbx_dom_weak::ustr("Position")) {
                            orbit.target = [v.x, v.y, v.z];
                        }
                    }
                }
            }
            if ui.button("🔄 Reset").clicked() { *orbit = crate::bevy_render::OrbitCam::default(); }

            ui.separator();

            ui.label("📏 Dist:");
            if ui.button("−").clicked() { orbit.dist = (orbit.dist * 0.85).max(2.0); }
            ui.add(egui::Slider::new(&mut orbit.dist, 2.0..=2000.0).show_value(false));
            if ui.button("+").clicked() { orbit.dist = (orbit.dist * 1.15).min(2000.0); }

            ui.separator();

            let mut speed = self.cam_move_speed;
            ui.label("Speed:");
            ui.add(egui::Slider::new(&mut speed, 1.0..=50.0).show_value(false));
            self.cam_move_speed = speed;

            if ui.button("⬆️ Up").clicked() { orbit.target[1] += speed; }
            if ui.button("⬇️ Down").clicked() { orbit.target[1] -= speed; }
        });
    }

    /// Transparent drag area over the Bevy 3D scene: drag to orbit, scroll to
    /// zoom.
    fn show_viewport_drag(&mut self, ui: &mut egui::Ui, orbit: &mut crate::bevy_render::OrbitCam) {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(220.0, 300.0)),
            egui::Sense::drag(),
        );
        if response.dragged() {
            let d = response.drag_delta();
            orbit.yaw -= d.x * 0.008;
            orbit.pitch = (orbit.pitch + d.y * 0.008).clamp(-1.5, 1.5);
        }
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            orbit.dist = (orbit.dist - scroll * 0.1).clamp(2.0, 2000.0);
        }
        // Subtle border so the user can see the drag area.
        ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(120, 180, 255, 90)), egui::StrokeKind::Inside);

        if self.dom.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Open a .rbxl/.rbxmx file to render its parts here.").color(Color32::WHITE));
            });
        }
    }

    fn show_assets_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("☁️ Roblox Creator Store Live Search Engine");
        ui.label("Live real-time query against official Roblox Catalog & Creator Store marketplace.");

        ui.separator();

        // 1. Direct Asset ID or URL Inserter Bar
        ui.group(|ui| {
            ui.label(RichText::new("📥 Direct Asset ID / URL Inserter").strong().color(Color32::from_rgb(100, 200, 255)));
            ui.horizontal_wrapped(|ui| {
                ui.label("Asset ID / URL:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.direct_asset_id_input)
                        .hint_text("e.g. 47433, 142785488, 4842207161, or catalog URL...")
                        .desired_width(220.0),
                );

                if ui.button(RichText::new("📥 Download & Insert Full Model (.rbxm / .rbxmx)").strong().color(Color32::from_rgb(100, 255, 140))).clicked() {
                    let input = self.direct_asset_id_input.trim().to_string();
                    if !input.is_empty() {
                        if let Some(dom) = self.dom.as_mut() {
                            let parent = self.selected.unwrap_or_else(|| dom.root_ref());
                            match RobloxApiClient::insert_by_asset_id_or_url(dom, parent, &input, Some(&self.roblosecurity_cookie)) {
                                Ok((new_ref, count, name)) => {
                                    self.selected = Some(new_ref);
                                    self.status = format!("✅ Inserted Model '{}' ({} instances) into Workspace", name, count);
                                    self.log_info(format!("Successfully downloaded and inserted Model '{}' ({} instances: parts, scripts, meshes, sounds) into Workspace!", name, count));
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

                if ui.button("📋 Paste from Clipboard").clicked() {
                    let clip = jni_bridge::get_clipboard_text();
                    if !clip.trim().is_empty() {
                        self.direct_asset_id_input = clip.trim().to_string();
                        self.status = format!("Pasted asset string: {}", self.direct_asset_id_input);
                    }
                }
            });
        });

        ui.add_space(4.0);

        // 2. Roblox Session Cookie Authentication (.ROBLOSECURITY)
        ui.collapsing("🔑 Optional Roblox Account Authentication (.ROBLOSECURITY)", |ui| {
            ui.label("Providing your .ROBLOSECURITY cookie allows downloading 100% exact raw .rbxm binaries directly from Roblox servers for any asset.");
            ui.horizontal_wrapped(|ui| {
                ui.label("Cookie:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.roblosecurity_cookie)
                        .password(true)
                        .hint_text("Paste .ROBLOSECURITY cookie...")
                        .desired_width(240.0),
                );
                if ui.button("📋 Paste Cookie").clicked() {
                    let clip = jni_bridge::get_clipboard_text();
                    if !clip.trim().is_empty() {
                        self.roblosecurity_cookie = clip.trim().to_string();
                        self.status = "Configured .ROBLOSECURITY cookie".into();
                    }
                }
                if !self.roblosecurity_cookie.is_empty() && ui.button("Clear").clicked() {
                    self.roblosecurity_cookie.clear();
                }
            });
        });

        ui.add_space(4.0);

        // 3. Place Asset Scanner & Batch Downloader (like studio-lite AssetManager)
        if let Some(dom) = &self.dom {
            let place_assets = asset_downloader::scan_place_assets(dom);
            ui.collapsing(format!("📦 Place Assets Inspector ({} Found)", place_assets.len()), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(RichText::new("⬇️ Download & Cache All Place Assets (.mesh & .png)").strong().color(Color32::from_rgb(100, 255, 120))).clicked() {
                        let cookie_clone = self.roblosecurity_cookie.clone();
                        let assets_clone = place_assets.clone();
                        std::thread::spawn(move || {
                            for asset in assets_clone {
                                if asset.asset_type == "Mesh" {
                                    roblox_api::fetch_and_cache_mesh_async(format!("rbxassetid://{}", asset.asset_id), if cookie_clone.is_empty() { None } else { Some(cookie_clone.clone()) });
                                }
                            }
                        });
                        self.status = format!("Initiated batch download for {} place assets", place_assets.len());
                        self.log_info(format!("Downloading {} meshes/textures in background", place_assets.len()));
                    }
                });

                ui.add_space(4.0);
                for asset in place_assets.iter().take(15) {
                    let is_cached = asset_downloader::get_cached_mesh(&format!("rbxassetid://{}", asset.asset_id)).is_some()
                        || asset_downloader::get_builtin_asset(&asset.asset_id).is_some();
                    ui.horizontal(|ui| {
                        let status_icon = if is_cached { "✅ Loaded" } else { "⏳ Needs Fetch" };
                        ui.label(format!("• {} '{}' (ID: {}) [{}]", asset.asset_type, asset.instance_name, asset.asset_id, status_icon));
                    });
                }
            });
            ui.add_space(4.0);
        }

        // 4. Live Creator Store Search Input Bar
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

        // 3. Live Creator Store Results List
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
                                if ui.button(RichText::new("📥 Insert Full Model into Workspace").color(Color32::from_rgb(120, 255, 120)).strong()).clicked() {
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
                match RobloxApiClient::insert_live_item_into_place(dom, parent, &item, Some(&self.roblosecurity_cookie)) {
                    Ok((new_ref, count)) => {
                        self.selected = Some(new_ref);
                        self.status = format!("✅ Inserted '{}' ({} instances) into Workspace", item.name, count);
                        self.log_info(format!("Successfully downloaded and inserted Model '{}' (Asset ID: {}, {} instances: parts, scripts, meshes, sounds) into Workspace!", item.name, item.id, count));
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
                    ui.label("Serializes the active .rbxl in memory and streams it directly to Roblox Open Cloud API via memory pipe (no /tmp file).");

                    ui.checkbox(&mut self.open_cloud_publish_live, "Publish Live to Players (versionType=Published)");

                    if ui.button(RichText::new("⚡ Publish Place Now").strong().color(Color32::from_rgb(100, 255, 120))).clicked() {
                        if let Some(dom) = &self.dom {
                            match rbxl::save_place(dom) {
                                Ok(bytes) => {
                                    self.log_info("Serializing place in memory for Open Cloud publish...");
                                    match RobloxApiClient::publish_place_open_cloud(
                                        &self.open_cloud_api_key,
                                        &self.open_cloud_universe_id,
                                        &self.open_cloud_place_id,
                                        &bytes,
                                        self.open_cloud_publish_live,
                                    ) {
                                        Ok(res) => {
                                            self.status = "✅ Place published successfully via Open Cloud!".into();
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
        ui.label("Search any official class from Roblox Engine Reflection or insert live assets into Workspace.");

        ui.separator();

        // Direct Asset Inserter Quick Bar
        ui.group(|ui| {
            ui.label(RichText::new("🌐 Insert by Asset ID or Catalog URL").strong().color(Color32::from_rgb(100, 200, 255)));
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.direct_asset_id_input)
                        .hint_text("Asset ID e.g. 47433, 142785488, 4842207161...")
                        .desired_width(180.0),
                );

                if ui.button(RichText::new("📥 Insert Full Model").strong().color(Color32::from_rgb(100, 255, 140))).clicked() {
                    let input = self.direct_asset_id_input.trim().to_string();
                    if !input.is_empty() {
                        if let Some(dom) = self.dom.as_mut() {
                            let parent = self.selected.unwrap_or_else(|| dom.root_ref());
                            match RobloxApiClient::insert_by_asset_id_or_url(dom, parent, &input, Some(&self.roblosecurity_cookie)) {
                                Ok((new_ref, count, name)) => {
                                    self.selected = Some(new_ref);
                                    self.status = format!("✅ Inserted Model '{}' ({} instances) into Workspace", name, count);
                                    self.log_info(format!("Successfully inserted Model '{}' (Asset ID: {}, {} instances) into Workspace!", name, input, count));
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

                if ui.button("📋 Paste").clicked() {
                    let clip = jni_bridge::get_clipboard_text();
                    if !clip.trim().is_empty() {
                        self.direct_asset_id_input = clip.trim().to_string();
                    }
                }
            });
        });

        ui.add_space(4.0);

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
        match preset.name {
            "Suphis Signal Module" => {
                match roblox_api::RobloxApiClient::insert_live_item_into_place(dom, parent, &roblox_api::LiveCatalogItem {
                    id: 11670710927,
                    name: "Suphis Signal Module".into(),
                    description: "Suphis signal and connection data types that works a lot like RBXScriptSignal and RBXScriptConnection".into(),
                    creator_name: "5uphi".into(),
                    asset_type_id: 38,
                    price_robux: Some(0),
                    upvote_percent: 100,
                    upvotes: 9800,
                    script_count: 5,
                    mesh_part_count: 0,
                    audio_count: 0,
                    animation_count: 0,
                    decal_count: 0,
                    tool_count: 0,
                    triangle_count: 0,
                }, Some(&self.roblosecurity_cookie)) {
                    Ok((new_ref, count)) => {
                        self.selected = Some(new_ref);
                        self.status = format!("✅ Inserted 'Suphis Signal Module' ({} instances) into Workspace", count);
                        self.log_info(format!("Inserted 'Suphis Signal Module' ({} instances: Connection, GoodSignal, Signal, Types, Demo)", count));
                    }
                    Err(e) => {
                        self.status = format!("Insert failed: {e}");
                    }
                }
            }
            "KillBrick Hazard" => {
                let part_builder = rbx_dom_weak::InstanceBuilder::new("Part")
                    .with_name("KillBrick")
                    .with_property("Size", Variant::Vector3(Vector3::new(8.0, 1.0, 8.0)))
                    .with_property("Position", Variant::Vector3(Vector3::new(0.0, 0.5, 0.0)))
                    .with_property("Color", Variant::Color3(Color3::new(1.0, 0.1, 0.1)))
                    .with_property("Material", Variant::String("Neon".into()))
                    .with_property("Anchored", Variant::Bool(true))
                    .with_property("CanCollide", Variant::Bool(true));
                let part_ref = dom.insert(parent, part_builder);
                if let Some((script_name, script_code)) = preset.default_script {
                    let script_builder = rbx_dom_weak::InstanceBuilder::new("Script")
                        .with_name(script_name)
                        .with_property("Source", Variant::String(script_code.into()));
                    dom.insert(part_ref, script_builder);
                }
                self.selected = Some(part_ref);
                self.active_tab = ActiveTab::Viewport3D;
                self.status = format!("✅ Inserted '{}' hazard into Workspace", preset.name);
                self.log_info(format!("Inserted '{}' with neon part & damage script", preset.name));
            }
            "Interactive Door" => {
                let model_builder = rbx_dom_weak::InstanceBuilder::new("Model").with_name("InteractiveDoor");
                let model_ref = dom.insert(parent, model_builder);

                let frame_builder = rbx_dom_weak::InstanceBuilder::new("Part")
                    .with_name("DoorFrame")
                    .with_property("Size", Variant::Vector3(Vector3::new(1.0, 8.0, 1.0)))
                    .with_property("Position", Variant::Vector3(Vector3::new(-2.5, 4.0, 0.0)))
                    .with_property("Color", Variant::Color3(Color3::new(0.3, 0.2, 0.1)))
                    .with_property("Anchored", Variant::Bool(true));
                dom.insert(model_ref, frame_builder);

                let door_builder = rbx_dom_weak::InstanceBuilder::new("Part")
                    .with_name("Door")
                    .with_property("Size", Variant::Vector3(Vector3::new(4.0, 7.8, 0.6)))
                    .with_property("Position", Variant::Vector3(Vector3::new(0.0, 4.0, 0.0)))
                    .with_property("Color", Variant::Color3(Color3::new(0.55, 0.35, 0.2)))
                    .with_property("Anchored", Variant::Bool(true));
                let door_ref = dom.insert(model_ref, door_builder);

                let prompt_builder = rbx_dom_weak::InstanceBuilder::new("ProximityPrompt")
                    .with_name("DoorPrompt")
                    .with_property("ActionText", Variant::String("Open Door".into()))
                    .with_property("ObjectText", Variant::String("Wooden Door".into()))
                    .with_property("HoldDuration", Variant::Float32(0.5));
                dom.insert(door_ref, prompt_builder);

                if let Some((script_name, script_code)) = preset.default_script {
                    let script_builder = rbx_dom_weak::InstanceBuilder::new("Script")
                        .with_name(script_name)
                        .with_property("Source", Variant::String(script_code.into()));
                    dom.insert(model_ref, script_builder);
                }
                self.selected = Some(model_ref);
                self.active_tab = ActiveTab::Viewport3D;
                self.status = format!("✅ Inserted '{}' model into Workspace", preset.name);
                self.log_info(format!("Inserted '{}' with frame, door, prompt & tween script", preset.name));
            }
            "Main GUI Framework" => {
                let gui_builder = rbx_dom_weak::InstanceBuilder::new("ScreenGui").with_name("MainGui");
                let gui_ref = dom.insert(parent, gui_builder);

                let frame_builder = rbx_dom_weak::InstanceBuilder::new("Frame")
                    .with_name("MainContainer")
                    .with_property("BackgroundColor3", Variant::Color3uint8(Color3uint8::new(30, 30, 35)));
                let frame_ref = dom.insert(gui_ref, frame_builder);

                let label_builder = rbx_dom_weak::InstanceBuilder::new("TextLabel")
                    .with_name("TitleLabel")
                    .with_property("Text", Variant::String("Game Menu".into()))
                    .with_property("TextColor3", Variant::Color3uint8(Color3uint8::new(255, 255, 255)))
                    .with_property("TextScaled", Variant::Bool(true));
                dom.insert(frame_ref, label_builder);

                let btn_builder = rbx_dom_weak::InstanceBuilder::new("TextButton")
                    .with_name("PlayButton")
                    .with_property("Text", Variant::String("▶ Play Game".into()))
                    .with_property("BackgroundColor3", Variant::Color3uint8(Color3uint8::new(0, 180, 255)))
                    .with_property("TextColor3", Variant::Color3uint8(Color3uint8::new(255, 255, 255)))
                    .with_property("TextScaled", Variant::Bool(true));
                dom.insert(frame_ref, btn_builder);

                self.selected = Some(gui_ref);
                self.status = format!("✅ Inserted '{}' into StarterGui", preset.name);
                self.log_info(format!("Inserted '{}' UI hierarchy", preset.name));
            }
            _ => {
                match rbxl::add_instance(dom, parent, preset.class, preset.name) {
                    Ok(new_ref) => {
                        if let Some((script_name, script_code)) = preset.default_script {
                            let script_builder = rbx_dom_weak::InstanceBuilder::new("Script")
                                .with_name(script_name)
                                .with_property("Source", Variant::String(script_code.into()));
                            dom.insert(new_ref, script_builder);
                        }
                        self.selected = Some(new_ref);
                        self.status = format!("✅ Inserted toolbox system '{}'", preset.name);
                        self.log_info(format!("Inserted preset {}", preset.name));
                    }
                    Err(e) => {
                        self.status = format!("Insert preset failed: {e}");
                    }
                }
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
                        self.needs_3d_rebuild = true;
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

    fn show_settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("⚙️ Studio Preferences & Saved Credentials");
        ui.label("Saved credentials persist across app restarts to automate Open Cloud publishing and raw .rbxm asset downloading.");

        ui.separator();

        egui::ScrollArea::both()
            .id_salt("settings_scroll_area")
            .show(ui, |ui| {
                // 1. Roblox Account Authentication (.ROBLOSECURITY)
                ui.group(|ui| {
                    ui.label(RichText::new("🔑 Roblox Account Credentials (.ROBLOSECURITY)").heading().color(Color32::from_rgb(100, 200, 255)));
                    ui.label("Enables direct downloading of 100% exact live .rbxm binary models, meshes, and textures from Roblox AssetDelivery CDN.");

                    ui.horizontal_wrapped(|ui| {
                        ui.label("Cookie:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.roblosecurity_cookie)
                                .password(true)
                                .hint_text("Paste your .ROBLOSECURITY cookie...")
                                .desired_width(260.0),
                        );

                        if ui.button("📋 Paste").clicked() {
                            let text = jni_bridge::get_clipboard_text();
                            if !text.trim().is_empty() {
                                self.roblosecurity_cookie = text.trim().to_string();
                                self.status = "Pasted .ROBLOSECURITY cookie".into();
                            }
                        }

                        if !self.roblosecurity_cookie.is_empty() && ui.button("Clear").clicked() {
                            self.roblosecurity_cookie.clear();
                        }
                    });

                    if !self.roblosecurity_cookie.is_empty() {
                        ui.label(RichText::new("✓ Cookie configured: Direct 100% raw .rbxm download enabled").color(Color32::from_rgb(120, 255, 120)));
                    } else {
                        ui.label(RichText::new("ℹ️ No cookie set: Using high-fidelity asset synthesizers and unauthenticated endpoints").color(Color32::from_rgb(200, 200, 100)));
                    }
                });

                ui.add_space(8.0);

                // 2. Open Cloud API Credentials
                ui.group(|ui| {
                    ui.label(RichText::new("🚀 Roblox Open Cloud API Suite").heading().color(Color32::from_rgb(120, 255, 120)));
                    ui.label("API credentials for Experience Place Publishing, DataStore inspection, and MessagingService.");

                    ui.horizontal_wrapped(|ui| {
                        ui.label("API Key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.open_cloud_api_key)
                                .password(true)
                                .hint_text("Paste Open Cloud API key...")
                                .desired_width(240.0),
                        );

                        if ui.button("📋 Paste").clicked() {
                            let text = jni_bridge::get_clipboard_text();
                            if !text.trim().is_empty() {
                                self.open_cloud_api_key = text.trim().to_string();
                            }
                        }
                    });

                    ui.horizontal_wrapped(|ui| {
                        ui.label("Universe ID:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.open_cloud_universe_id)
                                .hint_text("e.g. 123456789")
                                .desired_width(120.0),
                        );
                        if ui.button("📋").clicked() {
                            let text = jni_bridge::get_clipboard_text();
                            if !text.trim().is_empty() {
                                self.open_cloud_universe_id = text.trim().to_string();
                            }
                        }

                        ui.label("Place ID:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.open_cloud_place_id)
                                .hint_text("e.g. 987654321")
                                .desired_width(120.0),
                        );
                        if ui.button("📋").clicked() {
                            let text = jni_bridge::get_clipboard_text();
                            if !text.trim().is_empty() {
                                self.open_cloud_place_id = text.trim().to_string();
                            }
                        }
                    });

                    ui.checkbox(&mut self.open_cloud_publish_live, "Publish Live to Players by Default (versionType=Published)");
                });

                ui.add_space(8.0);

                ui.label(RichText::new("🌍 3D Viewing: opens in the separate \"rbxl Viewer\" GPU app (Bevy / OpenRBLX renderer) via the View tab.").weak());

                ui.add_space(12.0);

                // Save & Action Buttons
                ui.horizontal_wrapped(|ui| {
                    if ui.button(RichText::new("💾 Save Settings & Credentials").heading().color(Color32::from_rgb(100, 255, 120))).clicked() {
                        let settings_to_save = EditorSettings {
                            roblosecurity_cookie: self.roblosecurity_cookie.clone(),
                            open_cloud_api_key: self.open_cloud_api_key.clone(),
                            open_cloud_universe_id: self.open_cloud_universe_id.clone(),
                            open_cloud_place_id: self.open_cloud_place_id.clone(),
                            auto_download_meshes: true,
                            show_skybox: true,
                        };

                        match settings_to_save.save() {
                            Ok(_) => {
                                self.status = "✅ Settings saved successfully to persistent storage!".into();
                                self.log_info("Saved credentials and viewport preferences");
                            }
                            Err(e) => {
                                self.status = format!("Save error: {e}");
                                self.log_error(format!("Failed to save settings: {e}"));
                            }
                        }
                    }

                    if ui.button(RichText::new("🗑️ Clear All Credentials").color(Color32::from_rgb(255, 100, 100))).clicked() {
                        self.roblosecurity_cookie.clear();
                        self.open_cloud_api_key.clear();
                        self.open_cloud_universe_id.clear();
                        self.open_cloud_place_id.clear();
                        let _ = EditorSettings::default().save();
                        self.status = "Cleared saved credentials".into();
                    }
                });
            });
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
