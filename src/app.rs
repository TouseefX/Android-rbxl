use crate::asset_downloader::{self, DiscoveredAsset};
use crate::bevy_render::OrbitCam;
use crate::jni_bridge::{self, FileEvent};
use crate::live_session;
use crate::lua_runtime;
use crate::plugins;
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
use std::sync::mpsc::{channel, Receiver, Sender};

/// Log line emitted by a background plugin run.
enum PluginLogLine {
    Output(lua_runtime::OutputLine),
    Error(String),
    Done(usize),
}
/// Background plugin run channel. A single mpsc pair, created lazily.
static PLUGIN_LOG: OnceLock<(Sender<PluginLogLine>, Mutex<Receiver<PluginLogLine>>)> = OnceLock::new();
static PLUGIN_RUNNING: OnceLock<(Sender<String>, Mutex<Receiver<String>>)> = OnceLock::new();
fn plugin_log_channel() -> (&'static Sender<PluginLogLine>, &'static Mutex<Receiver<PluginLogLine>>) {
    let (tx, rx) = PLUGIN_LOG.get_or_init(|| {
        let (tx, rx) = channel();
        (tx, Mutex::new(rx))
    });
    (tx, rx)
}
fn plugin_running_channel() -> (&'static Sender<String>, &'static Mutex<Receiver<String>>) {
    let (tx, rx) = PLUGIN_RUNNING.get_or_init(|| {
        let (tx, rx) = channel();
        (tx, Mutex::new(rx))
    });
    (tx, rx)
}
use std::sync::{Mutex, OnceLock};

/// Background channel for catalog search-result thumbnail URLs.
static CATALOG_THUMBNAIL_TX: OnceLock<Sender<HashMap<u64, String>>> = OnceLock::new();
static CATALOG_THUMBNAIL_RX: OnceLock<Mutex<Receiver<HashMap<u64, String>>>> = OnceLock::new();

static PLUGIN_THUMBNAIL: OnceLock<(Sender<HashMap<u64,String>>, Mutex<Receiver<HashMap<u64,String>>>)> = OnceLock::new();
fn plugin_thumb_tx() -> &'static Sender<HashMap<u64,String>> {
    let (tx, _) = PLUGIN_THUMBNAIL.get_or_init(|| { let (t,r) = channel(); (t, Mutex::new(r)) });
    tx
}
fn plugin_thumb_rx() -> &'static Mutex<Receiver<HashMap<u64,String>>> {
    let (_, rx) = PLUGIN_THUMBNAIL.get_or_init(|| { let (t,r) = channel(); (t, Mutex::new(r)) });
    rx
}

fn catalog_thumbnail_channel() -> (&'static Sender<HashMap<u64, String>>, &'static Mutex<Receiver<HashMap<u64, String>>>) {
    let tx = CATALOG_THUMBNAIL_TX.get_or_init(|| {
        let (tx, rx) = channel();
        let _ = CATALOG_THUMBNAIL_RX.set(Mutex::new(rx));
        tx
    });
    (tx, CATALOG_THUMBNAIL_RX.get().unwrap())
}

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
    Plugins,
    Browse,
    Output,
    Command,
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

/// Actions the Plugins tab can request; processed after the UI closes to
/// avoid borrow conflicts.
enum PluginAction {
    Toggle(String),
    Run(String),
    Stop,
    Insert(String),
    Delete(String),
    OpenScript(String, String),
}

#[derive(bevy::prelude::Resource)]
pub struct EditorApp {
    dom: Option<WeakDom>,
    selected: Option<Ref>,
    current_uri: Option<String>,
    /// On-disk format of the currently-open place (binary .rbxl or XML .rbxlx),
    /// so Save preserves the format the user opened.
    place_format: rbxl::PlaceFormat,
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
    /// Thumbnail URLs keyed by catalog item id.
    catalog_thumbnails: HashMap<u64, String>,
    plugin_thumbnails: HashMap<u64, String>,
    catalog_thumbs_fetched_for: String,
    is_searching_live: bool,
    direct_asset_id_input: String,
    /// Place ID typed in the toolbar "🌐 Open from Roblox" field.
    open_place_id_input: String,
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

    // When Some, drain_events() will flip needs_3d_rebuild back on once this
    // deadline passes, so meshes/textures that finished downloading in the
    // background (see auto_download_place_assets) actually get pulled into
    // the scene instead of only ever showing on the NEXT manual reopen.
    pending_asset_refresh_at: Option<std::time::Instant>,

    // Command bar: a one-line Luau prompt that runs against the embedded
    // luaur VM with a tiny `game`/`workspace`/`script`-style surface so users
    // can run quick snippets the way they do in Studio's command bar. When
    // a Live Session is connected, the user can opt to send commands to
    // real Studio instead.
    command_input: String,
    command_history: Vec<String>,
    command_history_idx: usize,
    /// Last system clipboard text we pushed into egui.
    last_clipboard: String,
    command_output: Vec<lua_runtime::OutputLine>,
    command_run_target_studio: bool,

    // Installed plugins (persisted on disk in the app's files dir).
    plugin_index: plugins::PluginIndex,
    // Asset-id input for downloading a plugin from the Creator Store.
    plugin_asset_id_input: String,

    // Browse Roblox tab (groups / universes / places).
    browse_group_id: String,
    browse_universes: Vec<roblox_api::GroupUniverse>,
    browse_thumbnails: HashMap<u64, String>,
    browse_selected_universe: Option<u64>,
    browse_places: Vec<(u64, String)>,
    browse_status: String,

    // Plugin sandbox run state. A running plugin is identified by its
    // record id; the atomic flag lets the Stop button cancel scripts
    // mid-run (checked between scripts in the batch, and exposed to
    // long-running Luau via a hook if needed).
    running_plugin_id: Option<String>,
    plugin_stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,

    // Live Session bridge state.
    live_session: live_session::LiveSessionState,
}

impl Default for EditorApp {
    fn default() -> Self {
        let saved_settings = EditorSettings::load();

        let mut app = Self {
            dom: None,
            selected: None,
            current_uri: None,
            place_format: rbxl::PlaceFormat::Binary,
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
            catalog_thumbnails: HashMap::new(),
            plugin_thumbnails: HashMap::new(),
            catalog_thumbs_fetched_for: String::new(),
            is_searching_live: false,
            direct_asset_id_input: "47433".into(),
            open_place_id_input: String::new(),
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
            pending_asset_refresh_at: None,
            command_input: String::new(),
            command_history: Vec::new(),
            command_history_idx: 0,
            last_clipboard: String::new(),
            command_output: Vec::new(),
            command_run_target_studio: false,
            plugin_index: plugins::load_index(),
            plugin_asset_id_input: String::new(),
            browse_group_id: String::new(),
            browse_universes: Vec::new(),
            browse_thumbnails: HashMap::new(),
            browse_selected_universe: None,
            browse_places: Vec::new(),
            browse_status: String::new(),
            running_plugin_id: None,
            plugin_stop_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            live_session: live_session::LiveSessionState::default(),
        };
        app.log_info("Roblox Studio Lite initialized with persistent settings");
        app.log_info(&format!(
            "Loaded {} installed plugin(s)",
            app.plugin_index.plugins.len()
        ));
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
        self.place_format = rbxl::PlaceFormat::detect(&bytes);
        match rbxl::load_place(bytes) {
            Ok(dom) => {
                self.dom = Some(dom);
                self.selected = None;
                self.needs_3d_rebuild = true;
                self.status = format!("Loaded ({})", self.place_format.label());
                // New document → fresh undo history.
                lua_runtime::reset_command_history();
                self.auto_download_place_assets();
            }
            Err(e) => {
                self.status = format!("Failed to parse: {e}");
            }
        }
    }

    /// Kick off background downloads for every Mesh/Texture asset referenced
    /// by the place, without waiting for the user to find and press the
    /// "Download & Cache All Place Assets" button. Previously a freshly
    /// opened place had NOTHING cached, so every MeshPart and every real
    /// (non-procedural) decal texture silently fell back to invisible /
    /// flat-color on first render — this made the renderer look broken even
    /// though it was just waiting on assets nobody had triggered a fetch for.
    /// Meshes and textures pop in via `needs_3d_rebuild` polling once they land
    /// (see `drain_events`/the 3D tab) rather than all at once.
    fn auto_download_place_assets(&mut self) {
        let Some(dom) = &self.dom else { return };
        let assets = asset_downloader::scan_place_assets(dom);
        if assets.is_empty() {
            return;
        }
        let cookie = if self.roblosecurity_cookie.is_empty() { None } else { Some(self.roblosecurity_cookie.clone()) };
        self.log_info(format!("Auto-downloading {} referenced assets in the background", assets.len()));
        std::thread::spawn(move || {
            for asset in assets {
                let id = format!("rbxassetid://{}", asset.asset_id);
                match asset.asset_type {
                    "Mesh" => roblox_api::fetch_and_cache_mesh_async(id, cookie.clone()),
                    "Texture" => roblox_api::fetch_and_cache_image_async(id, cookie.clone()),
                    "Sound" => roblox_api::fetch_and_cache_audio_async(id, cookie.clone()),
                    // Animations are rbxm model files, not raw binaries;
                    // they'll be fetched on demand when inserted.
                    "Animation" => {}
                    _ => {}
                }
            }
        });
        // fetch_and_cache_*_async are fire-and-forget background threads with
        // no completion signal, so we can't know exactly when they're done.
        // 4s is a rough guess generous enough for a city-sized place over
        // typical mobile data; bump this (or add a real completion channel,
        // like the existing search_channel pattern) if your assets are bigger.
        self.pending_asset_refresh_at = Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
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
        // Keep egui's internal clipboard in sync with the Android system
        // clipboard. arboard (egui's default backend) doesn't work on
        // Android, and there's no native EditText long-press menu, so we
        // copy the system text into egui each frame. egui handles its own
        // copy internally (it sets system clipboard via our JNI when the
        // user copies), so we only need to push here.
        self.sync_clipboard(ctx);
        // Pump any completed thumbnail downloads into GPU textures.
        crate::thumbnails::pump(ctx);

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
                    // Open a place directly from Roblox by place ID using the
                    // cookie-authenticated web client. Downloads the .rbxl then
                    // loads it exactly like a local file open.
                    ui.add(
                        egui::TextEdit::singleline(&mut self.open_place_id_input)
                            .hint_text("place ID")
                            .desired_width(90.0),
                    );
                    if ui.button("🌐 Open from Roblox").clicked() {
                        self.open_place_from_roblox();
                    }
                    if ui.button(RichText::new("📥 Import Local .rbxm").strong().color(Color32::from_rgb(100, 200, 255))).clicked() {
                        self.prompt_import_local_model();
                    }
                    if ui.button(RichText::new("💾 Save").strong().color(Color32::from_rgb(100, 255, 120))).clicked() {
                        self.save();
                    }
                    if ui.button("💾 Save As...").clicked() {
                        self.save_as();
                    }
                    if ui.button(RichText::new("🚀 Publish to Roblox").color(Color32::from_rgb(255, 180, 80))).clicked() {
                        self.publish_place_to_roblox();
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
                            tab_btn(ui, "🧩 Plugins", ActiveTab::Plugins);
                            tab_btn(ui, "🌐 Browse Roblox", ActiveTab::Browse);
                            tab_btn(ui, "🚀 Open Cloud", ActiveTab::OpenCloud);
                            tab_btn(ui, "▶ Command Bar", ActiveTab::Command);
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
                    self.show_viewport_controls(ui, orbit);
                });
            // Transparent central panel: the Bevy 3D scene shows through and
            // this region senses drag/scroll to orbit the camera.
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(egui::Color32::TRANSPARENT))
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
                    ActiveTab::Plugins => self.show_plugins_ui(ui),
                    ActiveTab::Browse => self.show_browse_ui(ui),
                    ActiveTab::Command => self.show_command_ui(ui),
                    ActiveTab::Output => self.show_output_ui(ui),
                    ActiveTab::Settings => self.show_settings_ui(ui),
                }
            });
        }

        // Drain egui output commands. In particular, when the user
        // copies text inside an egui widget (Ctrl+C / selection), egui
        // emits OutputCommand::CopyText; forward it to the Android
        // clipboard so the system paste menu actually has our text.
        ctx.output(|out| {
            for cmd in out.commands.iter() {
                if let egui::OutputCommand::CopyText(text) = cmd {
                    jni_bridge::trigger_copy_to_clipboard(text);
                }
            }
        });
    }
}

impl EditorApp {
    /// Camera control bar (always drawn on a solid panel so it's visible over
    /// the 3D). Steers the Bevy `OrbitCam`.
    fn show_viewport_controls(&mut self, ui: &mut egui::Ui, orbit: &mut crate::bevy_render::OrbitCam) {
        // Row 1 (scrollable): label, presets, focus, reset, distance, speed.
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
        });

        // Row 2 (always visible, NOT scrolled): camera height Up / Down.
        ui.horizontal(|ui| {
            ui.label("Height:");
            if ui.button("⬆️ Up").clicked() { orbit.target[1] += self.cam_move_speed; }
            if ui.button("⬇️ Down").clicked() { orbit.target[1] -= self.cam_move_speed; }
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
        ui.painter().rect_stroke(rect, 0.0_f32, egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(120, 180, 255, 90)), egui::StrokeKind::Inside);

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

});
        });

        ui.add_space(4.0);

        // 1b. Local Model File Inserter — the off-line counterpart to the
        // network inserter above. Picks a .rbxm/.rbxmx (or gzipped/Luau) file
        // from the device and inserts its hierarchy into the active place,
        // under the current Explorer selection (or the place root).
        ui.group(|ui| {
            ui.label(RichText::new("📁 Import Local Model File (.rbxm / .rbxmx)").strong().color(Color32::from_rgb(100, 200, 255)));
            ui.label("Insert a model saved on your device — no download or Creator Store required. Supports binary .rbxm, XML .rbxmx, gzipped, and .lua/.luau source.");
            ui.horizontal_wrapped(|ui| {
                if ui.button(RichText::new("📂 Choose Local .rbxm/.rbxmx & Insert").strong().color(Color32::from_rgb(100, 255, 140))).clicked() {
                    self.prompt_import_local_model();
                }
                if self.dom.is_none() {
                    ui.label(RichText::new("(open a .rbxl place first)").color(Color32::from_rgb(200, 200, 100)));
                } else if let Some(dom) = &self.dom {
                    let parent = self.selected.and_then(|r| dom.get_by_ref(r));
                    match parent {
                        Some(inst) => ui.label(format!("Will insert under: {} ({})", inst.name, inst.class)),
                        None => ui.label("Will insert under: place root (DataModel)"),
                    };
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
                                    if asset.asset_type == "Sound" {
                                        roblox_api::fetch_and_cache_audio_async(
                                            format!("rbxassetid://{}", asset.asset_id),
                                            if cookie_clone.is_empty() { None } else { Some(cookie_clone.clone()) },
                                        );
                                    }
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
                                // Thumbnail (downloaded + uploaded to GPU
                                // asynchronously; spinner until ready).
                                if let Some(url) = self.catalog_thumbnails.get(&item.id) {
                                    match crate::thumbnails::get_or_load(ui.ctx(), url) {
                                        Some(tex) => {
                                            ui.add(
                                                egui::Image::from_texture(&tex)
                                                    .fit_to_exact_size(egui::vec2(64.0, 64.0))
                                                    .corner_radius(egui::CornerRadius::same(4)),
                                            );
                                        }
                                        None => {
                                            ui.add_space(64.0);
                                            ui.spinner();
                                        }
                                    }
                                }
                                ui.vertical(|ui| {
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
                                });
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
});
                    ui.horizontal(|ui| {
                        ui.label("Universe ID:");
                        ui.add(egui::TextEdit::singleline(&mut self.open_cloud_universe_id).hint_text("e.g. 123456789").desired_width(110.0));
ui.label("Place ID:");
                        ui.add(egui::TextEdit::singleline(&mut self.open_cloud_place_id).hint_text("e.g. 987654321").desired_width(110.0));
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
                // Play button for Sounds: looks up the SoundId, fetches
                // if needed, and plays via the platform audio backend.
                if ui.button("🔊 Play").clicked() {
                    self.play_selected_sound();
                }
                if ui.button("⏹ Stop").clicked() {
                    crate::audio::stop();
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

                let mut prop_updates: Vec<(String, Variant)> = Vec::new();
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

                    // If the reflection DB knows this property is a Roblox enum,
                    // render a proper dropdown with all valid item names instead
                    // of an opaque raw integer.
                    let enum_info = schema::resolve_enum(&inst_class, key_str);

                    ui.horizontal(|ui| {
                        ui.label(format!("{key_str}:"));

                        let changed = match val {
                            Variant::Enum(e) => {
                                let current = e.to_u32();
                                let mut new_val = current;
                                let mut did_change = false;
                                if let Some((enum_name, items)) = &enum_info {
                                    let current_name = items
                                        .iter()
                                        .find(|(_, v)| *v == current)
                                        .map(|(n, _)| n.as_str())
                                        .unwrap_or("Unknown");
                                    egui::ComboBox::from_id_salt(format!("enum_{key_str}"))
                                        .selected_text(format!("{current_name} [{current}]"))
                                        .width(180.0)
                                        .show_ui(ui, |ui| {
                                            for (name, value) in items {
                                                if ui.selectable_label(*value == current, format!("{name} [{value}]")).clicked() {
                                                    new_val = *value;
                                                    did_change = true;
                                                }
                                            }
                                        });
                                    ui.label(RichText::new(enum_name).weak());
                                } else {
                                    // Unknown enum: allow editing as a raw u32.
                                    let mut raw = current as i32;
                                    if ui.add(egui::DragValue::new(&mut raw).speed(1).range(0..=i32::MAX)).changed() {
                                        new_val = raw as u32;
                                        did_change = true;
                                    }
                                }
                                did_change.then(|| Variant::Enum(rbx_dom_weak::types::Enum::from_u32(new_val)))
                            }
                            Variant::String(s) => {
                                let mut text = s.clone();
                                ui.add(
                                    egui::TextEdit::singleline(&mut text)
                                        .desired_width(200.0)
                                        .clip_text(true),
                                )
                                .changed()
                                .then(|| Variant::String(text))
                            }
                            Variant::ContentId(c) => {
                                let mut text = c.as_str().to_string();
                                if ui.add(egui::TextEdit::singleline(&mut text).hint_text("rbxassetid://...").desired_width(200.0)).changed() {
                                    Some(Variant::ContentId(text.into()))
                                } else { None }
                            }
                            Variant::Bool(b) => {
                                let mut val_bool = *b;
                                ui.checkbox(&mut val_bool, "").changed().then(|| Variant::Bool(val_bool))
                            }
                            Variant::Float32(f) => {
                                let mut val_f = *f;
                                ui.add(egui::DragValue::new(&mut val_f).speed(0.1)).changed().then(|| Variant::Float32(val_f))
                            }
                            Variant::Float64(f) => {
                                let mut val_f = *f;
                                ui.add(egui::DragValue::new(&mut val_f).speed(0.1)).changed().then(|| Variant::Float64(val_f))
                            }
                            Variant::Int32(i) => {
                                let mut val_i = *i;
                                ui.add(egui::DragValue::new(&mut val_i).speed(1)).changed().then(|| Variant::Int32(val_i))
                            }
                            Variant::Int64(i) => {
                                let mut val_i = *i;
                                ui.add(egui::DragValue::new(&mut val_i).speed(1)).changed().then(|| Variant::Int64(val_i))
                            }
                            Variant::Vector2(v) => {
                                let (mut x, mut y) = (v.x, v.y);
                                ui.label("X"); ui.add(egui::DragValue::new(&mut x).speed(0.2));
                                ui.label("Y"); let cy = ui.add(egui::DragValue::new(&mut y).speed(0.2)).changed();
                                (x != v.x || cy).then(|| Variant::Vector2(rbx_dom_weak::types::Vector2::new(x, y)))
                            }
                            Variant::Vector3(v) => {
                                let (mut x, mut y, mut z) = (v.x, v.y, v.z);
                                ui.label("X"); let cx = ui.add(egui::DragValue::new(&mut x).speed(0.2)).changed();
                                ui.label("Y"); let cy = ui.add(egui::DragValue::new(&mut y).speed(0.2)).changed();
                                ui.label("Z"); let cz = ui.add(egui::DragValue::new(&mut z).speed(0.2)).changed();
                                (cx || cy || cz).then(|| Variant::Vector3(Vector3::new(x, y, z)))
                            }
                            Variant::Color3(c) => {
                                let mut rgb = [c.r, c.g, c.b];
                                ui.color_edit_button_rgb(&mut rgb).changed().then(|| Variant::Color3(Color3::new(rgb[0], rgb[1], rgb[2])))
                            }
                            Variant::Color3uint8(c) => {
                                let mut srgb = [c.r, c.g, c.b];
                                ui.color_edit_button_srgb(&mut srgb).changed().then(|| Variant::Color3uint8(Color3uint8::new(srgb[0], srgb[1], srgb[2])))
                            }
                            Variant::UDim(u) => {
                                let (mut scale, mut offset) = (u.scale, u.offset);
                                ui.label("S"); ui.add(egui::DragValue::new(&mut scale).speed(0.01));
                                ui.label("O"); let co = ui.add(egui::DragValue::new(&mut offset).speed(1)).changed();
                                (scale != u.scale || co).then(|| Variant::UDim(rbx_dom_weak::types::UDim::new(scale, offset)))
                            }
                            Variant::UDim2(u) => {
                                let (mut xs, mut xo, mut ys, mut yo) = (u.x.scale, u.x.offset, u.y.scale, u.y.offset);
                                ui.label("X{S,O}");
                                ui.add(egui::DragValue::new(&mut xs).speed(0.01));
                                let cxo = ui.add(egui::DragValue::new(&mut xo).speed(1)).changed();
                                ui.label("Y{S,O}");
                                ui.add(egui::DragValue::new(&mut ys).speed(0.01));
                                let cyo = ui.add(egui::DragValue::new(&mut yo).speed(1)).changed();
                                (xs != u.x.scale || ys != u.y.scale || cxo || cyo).then(|| {
                                    Variant::UDim2(rbx_dom_weak::types::UDim2::new(
                                        rbx_dom_weak::types::UDim::new(xs, xo),
                                        rbx_dom_weak::types::UDim::new(ys, yo),
                                    ))
                                })
                            }
                            Variant::NumberRange(n) => {
                                let (mut mn, mut mx) = (n.min, n.max);
                                ui.label("min"); ui.add(egui::DragValue::new(&mut mn).speed(0.1));
                                ui.label("max"); let c = ui.add(egui::DragValue::new(&mut mx).speed(0.1)).changed();
                                (mn != n.min || c).then(|| Variant::NumberRange(rbx_dom_weak::types::NumberRange::new(mn, mx)))
                            }
                            Variant::BrickColor(b) => {
                                let mut n = *b as u16;
                                if ui.add(egui::DragValue::new(&mut n).speed(1).range(0..=1032)).changed() {
                                    rbx_dom_weak::types::BrickColor::from_number(n).map(Variant::BrickColor)
                                } else { None }
                            }
                            Variant::Ref(r) => {
                                let is_null = r.is_none();
                                if is_null {
                                    ui.label(RichText::new("(none)").weak());
                                } else {
                                    ui.label(RichText::new(format!("{r}")).weak());
                                }
                                None
                            }
                            Variant::SharedString(s) => {
                                let preview = String::from_utf8_lossy(s.data());
                                ui.label(RichText::new(format!("{} bytes: {}", s.data().len(), &preview[..preview.len().min(40)])).weak());
                                None
                            }
                            Variant::BinaryString(b) => {
                                let bytes: &[u8] = b.as_ref();
                                ui.label(RichText::new(format!("{} binary bytes", bytes.len())).weak());
                                None
                            }
                            _ => {
                                ui.label(RichText::new(format!("{val:?}")).weak());
                                None
                            }
                        };

                        if let Some(new_variant) = changed {
                            prop_updates.push((key_str.to_string(), new_variant));
                        }

                        if ui.small_button("🗑").on_hover_text("Remove this property").clicked() {
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

    /// If the selected instance is a Sound, fetch (if necessary) and
    /// play its SoundId.
    fn play_selected_sound(&mut self) {
        let Some(dom) = &self.dom else { return; };
        let Some(r) = self.selected else { return; };
        let Some(inst) = dom.get_by_ref(r) else { return; };
        if inst.class != "Sound" {
            self.status = "Select a Sound instance to play it".into();
            return;
        }
        // Find the SoundId property (ContentId/Content/String variants).
        let id_str = inst.properties.iter().find_map(|(k, v)| {
            if k.as_str().eq_ignore_ascii_case("SoundId") {
                match v {
                    Variant::String(s) => Some(s.clone()),
                    Variant::ContentId(c) => Some(c.as_str().to_string()),
                    Variant::Content(c) => c.as_uri().map(|s| s.to_string()),
                    _ => None,
                }
            } else { None }
        });
        let Some(raw) = id_str else {
            self.status = "Sound has no SoundId".into();
            return;
        };
        let Some(asset_id) = crate::asset_downloader::extract_asset_id(&raw) else {
            self.status = format!("Can't parse SoundId: {raw}");
            return;
        };
        let key = format!("rbxassetid://{asset_id}");
        let cookie = if self.roblosecurity_cookie.is_empty() { None } else { Some(self.roblosecurity_cookie.clone()) };
        match crate::audio::play_cached_or_fetch(&key, cookie) {
            Ok(()) => self.status = format!("Playing {key}"),
            Err(msg) => self.status = format!("Audio: {msg}"),
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

    /// Copy the Android system clipboard into egui's internal clipboard
    /// so that Ctrl+V (and egui's paste menu) works even though arboard
    /// is unavailable on Android. We only push when the text changed to
    /// avoid fighting egui's own copy handling.
    fn sync_clipboard(&mut self, ctx: &egui::Context) {
        let system = jni_bridge::get_clipboard_text();
        if system != self.last_clipboard {
            self.last_clipboard = system.clone();
            ctx.copy_text(system);
        }
    }

    fn drain_events(&mut self) {
        // Poll the live-session bridge (connected Studio companion plugin).
        self.drain_live_session_events();
        // Drain any background plugin-run log lines / completion.
        self.pump_plugin_logs();
        self.pump_plugin_thumbnails();

        // Any plugin downloads that finished on a background thread.
        for install in jni_bridge::try_recv_plugins() {
            match install.result {
                Ok(bytes) => self.install_plugin_bytes(
                    &install.name_hint,
                    plugins::PluginSource::CreatorStore,
                    // If the hint is asset_<id>, parse the id back out.
                    install
                        .name_hint
                        .strip_prefix("asset_")
                        .and_then(|s| s.parse::<u64>().ok()),
                    &bytes,
                ),
                Err(e) => {
                    self.log_error(format!("Plugin download failed: {e}"));
                    self.status = format!("Plugin download failed: {e}");
                }
            }
        }

        // If a background asset download was started, flip needs_3d_rebuild
        // back on once its rough deadline passes so downloaded meshes/textures
        // actually appear in the viewport without the user having to reopen
        // the file.
        if let Some(at) = self.pending_asset_refresh_at {
            if std::time::Instant::now() >= at {
                self.needs_3d_rebuild = true;
                self.pending_asset_refresh_at = None;
            }
        }

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

        // Pull in any catalog thumbnail URLs that finished loading.
        if let Some(rx) = CATALOG_THUMBNAIL_RX.get() {
            if let Ok(rx) = rx.lock() {
                while let Ok(map) = rx.try_recv() {
                    self.catalog_thumbnails.extend(map);
                }
            }
        }

        // When a new result set arrives, fetch its thumbnails once.
        let query_signature = self
            .live_catalog_items
            .iter()
            .map(|i| i.id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if !query_signature.is_empty() && query_signature != self.catalog_thumbs_fetched_for {
            self.catalog_thumbs_fetched_for = query_signature;
            let ids: Vec<u64> = self.live_catalog_items.iter().map(|i| i.id).collect();
            let tx = catalog_thumbnail_channel().0.clone();
            std::thread::spawn(move || {
                // The public thumbnails endpoint doesn't require a cookie;
                // pass "" so WebClient skips the Cookie header.
                if let Ok(client) = RobloxApiClient::web_client("") {
                    if let Ok(map) = client.thumbnails_batch(&ids, "Asset", "150x150") {
                        let _ = tx.send(map);
                    }
                }
            });
        }

        for event in jni_bridge::try_recv_all() {
            match event {
                FileEvent::Opened { uri, data } => {
                    let format = rbxl::PlaceFormat::detect(&data);
                    match rbxl::load_place(data) {
                        Ok(dom) => {
                            let count = dom.root().children().len();
                            self.place_format = format;
                            self.dom = Some(dom);
                            self.current_uri = Some(uri.clone());
                            self.selected = None;
                            self.open_tabs.clear();
                            self.needs_3d_rebuild = true;
                            self.status =
                                format!("Loaded {} ({count} top-level services)", format.label());
                            self.log_info(format!(
                                "Opened place: {uri} ({} format, {count} services)",
                                format.label()
                            ));
                            self.active_tab = ActiveTab::Explorer;
                            lua_runtime::reset_command_history();
                            self.auto_download_place_assets();
                        }
                        Err(e) => {
                            self.status = format!("Failed to parse: {e}");
                            self.log_error(format!("Failed to parse rbxl/rbxlx: {e}"));
                        }
                    }
                }
                FileEvent::ModelOpened { uri, data } => {
                    if jni_bridge::take_next_is_plugin() {
                        self.install_plugin_bytes(
                            &uri,
                            plugins::PluginSource::Local,
                            None,
                            &data,
                        );
                    } else {
                        self.insert_local_model(uri, data);
                    }
                }
                FileEvent::PlaceBytes { uri, data } => {
                    // A place downloaded from Roblox: replace the open
                    // document exactly as if the user picked it locally.
                    match rbxl::load_place(data) {
                        Ok(dom) => {
                            let count = dom.root().children().len();
                            self.place_format = rbxl::PlaceFormat::Binary;
                            self.dom = Some(dom);
                            self.current_uri = Some(uri.clone());
                            self.selected = None;
                            self.open_tabs.clear();
                            self.needs_3d_rebuild = true;
                            self.status = format!("Opened place from Roblox ({count} services)");
                            self.log_info(format!("Opened downloaded place: {uri} ({count} services)"));
                            self.active_tab = ActiveTab::Explorer;
                            lua_runtime::reset_command_history();
                            self.auto_download_place_assets();
                        }
                        Err(e) => {
                            self.status = format!("Failed to parse downloaded place: {e}");
                            self.log_error(format!("Downloaded place parse error: {e}"));
                        }
                    }
                }
                FileEvent::PlaceError { uri, error } => {
                    self.status = format!("Download failed: {error}");
                    self.log_error(format!("Place download {uri}: {error}"));
                }
                FileEvent::PublishResult { uri, result } => match result {
                    Ok(()) => {
                        self.status = format!("Published {uri} to Roblox");
                        self.log_info(format!("Published {uri} successfully"));
                    }
                    Err(e) => {
                        self.status = format!("Publish failed: {e}");
                        self.log_error(format!("Publish {uri}: {e}"));
                    }
                },
                FileEvent::GroupUniverses { group_id, universes, thumbs } => {
                    self.browse_group_id = group_id.to_string();
                    self.browse_universes = universes;
                    self.browse_thumbnails.extend(thumbs);
                    self.browse_selected_universe = None;
                    self.browse_places.clear();
                    self.browse_status = format!(
                        "Loaded {} experience(s) from group {group_id}",
                        self.browse_universes.len()
                    );
                }
                FileEvent::UniversePlaces { universe_id, places } => {
                    self.browse_selected_universe = Some(universe_id);
                    self.browse_places = places;
                    self.browse_status = format!(
                        "{} place(s) in universe {universe_id}",
                        self.browse_places.len()
                    );
                }
                FileEvent::BrowseError { message } => {
                    self.browse_status = format!("Error: {message}");
                    self.log_error(format!("Browse: {message}"));
                }
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

    // ------------------------------------------------------------------
    // Command bar
    // ------------------------------------------------------------------
    fn show_command_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("▶ Command Bar");
        ui.label(RichText::new(
            "Run Luau snippets in the embedded luaur VM. The command bar has \
             print/warn/pcall, task.*, Vector3/Color3/CFrame/UDim2, Enum.*, and a \
             stubbed plugin/script. It can't touch the live DataModel — for that, \
             connect a Live Session (companion Studio plugin) and the command will \
             execute inside real Studio instead.",
        ).weak());

        ui.separator();

        // Where to run: local VM or connected Studio.
        ui.horizontal(|ui| {
            let connected =
                self.live_session.status == live_session::SessionStatus::Connected;
            ui.label("Target:");
            let mut in_studio = self.command_run_target_studio;
            ui.add_enabled(connected, egui::Checkbox::new(&mut in_studio, "Live Studio session"));
            if !connected && in_studio {
                self.command_run_target_studio = false;
            } else {
                self.command_run_target_studio = in_studio;
            }
            if connected {
                ui.label(RichText::new("● connected").color(Color32::from_rgb(100, 255, 120)));
            } else {
                ui.label(RichText::new("● local VM").color(Color32::from_rgb(160, 160, 160)));
            }
        });

        ui.add_space(4.0);

        // Input line. Use a code font and submit on Enter (Shift+Enter for newline).
        let mut submitted = false;
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.command_input)
                .font(egui::FontId::monospace(14.0))
                .hint_text(">  print('hello')   —   Enter to run, ↑/↓ for history")
                .desired_width(f32::INFINITY)
                .lock_focus(true),
        );
        if response.ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) && !self.command_history.is_empty() {
            if self.command_history_idx > 0 {
                self.command_history_idx -= 1;
            }
            if let Some(s) = self.command_history.get(self.command_history_idx) {
                self.command_input = s.clone();
            }
        }
        if response.ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if self.command_history_idx + 1 < self.command_history.len() {
                self.command_history_idx += 1;
                self.command_input = self.command_history[self.command_history_idx].clone();
            } else {
                self.command_history_idx = self.command_history.len();
                self.command_input.clear();
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.command_input.trim().is_empty() {
            submitted = true;
        }

        ui.horizontal(|ui| {
            if ui.button("▶ Run").clicked() && !self.command_input.trim().is_empty() {
                submitted = true;
            }
            if ui.button("Clear Output").clicked() {
                self.command_output.clear();
            }
        });

        if submitted {
            let src = self.command_input.trim().to_string();
            // Push onto history (dedupe consecutive duplicates).
            if self.command_history.last().map(String::as_str) != Some(src.as_str()) {
                self.command_history.push(src.clone());
            }
            self.command_history_idx = self.command_history.len();

            self.command_output.push(lua_runtime::OutputLine {
                level: lua_runtime::Level::Info,
                text: format!("> {src}"),
            });

            if self.command_run_target_studio {
                self.live_session.run_in_studio(&src);
                self.command_output.push(lua_runtime::OutputLine {
                    level: lua_runtime::Level::Info,
                    text: "(sent to Studio; result will appear in its Output window)".into(),
                });
            } else if self.dom.is_some() {
                // Run against the REAL loaded DataModel.
                use std::cell::RefCell;
                use std::rc::Rc;
                let taken = self.dom.take().unwrap();
                let rc = Rc::new(RefCell::new(taken));
                match lua_runtime::run_command(rc.clone(), &src, "=command") {
                    Ok(outcome) => {
                        for line in lua_runtime::take_command_log() {
                            self.command_output.push(line);
                        }
                        // Always rebuild because even a pure property change
                        // can affect rendering; Undo/Redo definitely do.
                        self.needs_3d_rebuild = true;

                        // Surface selection changes from Selection service.
                        if let Some(first) = outcome.selected.first().copied() {
                            self.selected = Some(first);
                        } else if outcome.undo || outcome.redo {
                            // After undo/redo the old selection may point at
                            // an instance that no longer exists; clear it.
                            self.selected = None;
                        }

                        if outcome.undo {
                            self.command_output.push(lua_runtime::OutputLine {
                                level: lua_runtime::Level::Info,
                                text: "↶ undo".into(),
                            });
                        } else if outcome.redo {
                            self.command_output.push(lua_runtime::OutputLine {
                                level: lua_runtime::Level::Info,
                                text: "↷ redo".into(),
                            });
                        }

                        if outcome.created.is_empty()
                            && outcome.mutated == 0
                            && outcome.selected.is_empty()
                            && !outcome.undo
                            && !outcome.redo
                        {
                            self.command_output.push(lua_runtime::OutputLine {
                                level: lua_runtime::Level::Info,
                                text: "(no changes)".into(),
                            });
                        } else {
                            let mut parts = vec![format!(
                                "created {} instance(s), mutated {} propert(y/ies)",
                                outcome.created.len(),
                                outcome.mutated
                            )];
                            if !outcome.selected.is_empty() {
                                parts.push(format!("selected {}", outcome.selected.len()));
                            }
                            self.command_output.push(lua_runtime::OutputLine {
                                level: lua_runtime::Level::Info,
                                text: parts.join(", "),
                            });
                        }
                        let back = Rc::try_unwrap(rc).ok().expect("rc leaked").into_inner();
                        self.dom = Some(back);
                    }
                    Err(e) => {
                        for line in lua_runtime::take_command_log() {
                            self.command_output.push(line);
                        }
                        self.command_output.push(lua_runtime::OutputLine {
                            level: lua_runtime::Level::Error, text: e,
                        });
                        let back = Rc::try_unwrap(rc).ok().expect("rc leaked").into_inner();
                        self.dom = Some(back);
                    }
                }
            } else {
                let result = lua_runtime::run_source(&src, "=command");
                for line in result.lines { self.command_output.push(line); }
            }
            self.command_input.clear();
        }

        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("command_output_scroll")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.command_output.is_empty() {
                    ui.label(RichText::new("(no output yet)").weak());
                }
                for line in &self.command_output {
                    let (prefix, color) = match line.level {
                        lua_runtime::Level::Print => ("", Color32::from_rgb(220, 220, 220)),
                        lua_runtime::Level::Warn => ("⚠ ", Color32::from_rgb(255, 210, 120)),
                        lua_runtime::Level::Error => ("✗ ", Color32::from_rgb(255, 110, 110)),
                        lua_runtime::Level::Info => ("", Color32::from_rgb(150, 180, 220)),
                    };
                    ui.label(RichText::new(format!("{prefix}{}", line.text)).color(color).monospace());
                }
            });
    }

    // ------------------------------------------------------------------
    // Plugins tab
    // ------------------------------------------------------------------
    fn show_plugins_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("🧩 Plugin Manager");
        ui.label(RichText::new(
            "Plugins are .rbxm/.rbxmx files (the root is typically a Script or \
             ModuleScript). Install them from your device or by Creator Store asset \
             ID. Plugins run in the embedded luaur sandbox with a plugin:CreateToolbar \
             /CreateDockWidgetPluginGui surface; their widgets show up in the GUI tree \
             preview below. For full engine access, connect a Live Session to real \
             Studio.",
        ).weak());

        ui.separator();

        // Toolbar: import local, download by ID, refresh.
        ui.horizontal_wrapped(|ui| {
            if ui.button(RichText::new("📂 Install local .rbxm/.rbxmx").strong()).clicked() {
                self.prompt_import_local_plugin();
            }
            ui.label("Asset ID:");
            ui.add(
                egui::TextEdit::singleline(&mut self.plugin_asset_id_input)
                    .hint_text("e.g. 123456789")
                    .desired_width(140.0),
            );
            if ui.button("⬇️ Download & Install").clicked() {
                self.install_plugin_from_asset_id();
            }
            if ui.button("🔄 Reload list").clicked() {
                self.plugin_index = plugins::load_index();
            }
            // Stop a running sandbox plugin.
            if self.running_plugin_id.is_some() {
                if ui.button(RichText::new("⏹ Stop running plugin").color(Color32::from_rgb(255,120,120))).clicked() {
                    self.stop_running_plugin();
                }
                if let Some(id) = &self.running_plugin_id {
                    ui.label(RichText::new(format!("running: {id}")).weak());
                }
            }
        });

        // Fetch thumbnails for all installed plugins that have an icon
        // asset id, in the background.
        let plugin_icon_ids: Vec<u64> = self.plugin_index.plugins.iter()
            .filter_map(|p| p.icon_asset_id)
            .filter(|id| !self.plugin_thumbnails.contains_key(id))
            .collect();
        if !plugin_icon_ids.is_empty() {
            let ids = plugin_icon_ids.clone();
            std::thread::spawn(move || {
                if let Ok(client) = roblox_api::RobloxApiClient::web_client("") {
                    if let Ok(map) = client.thumbnails_batch(&ids, "Asset", "150x150") {
                        let _ = plugin_thumb_tx().send(map);
                    }
                }
            });
        }

        ui.separator();

        // Live session status/control.
        self.show_live_session_row(ui);

        ui.add_space(8.0);

        // Snapshot running plugin id so the per-card Stop button doesn't
        // take a &self borrow that conflicts with the mutable actions.
        let running_id = self.running_plugin_id.clone();
        // Plugin list.
        let mut action: Option<PluginAction> = None;
        egui::ScrollArea::vertical()
            .id_salt("plugin_list_scroll")
            .show(ui, |ui| {
                if self.plugin_index.plugins.is_empty() {
                    ui.label(RichText::new("No plugins installed yet.").weak());
                }
                for rec in &self.plugin_index.plugins {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            // Plugin icon (from Creator Store; local plugins
                            // show a generic puzzle piece).
                            if let Some(icon_id) = rec.icon_asset_id {
                                if let Some(url) = self.plugin_thumbnails.get(&icon_id) {
                                    match crate::thumbnails::get_or_load(ui.ctx(), url) {
                                        Some(tex) => {
                                            ui.add(
                                                egui::Image::from_texture(&tex)
                                                    .fit_to_exact_size(egui::vec2(40.0, 40.0))
                                                    .corner_radius(egui::CornerRadius::same(4)),
                                            );
                                        }
                                        None => {
                                            ui.add_space(40.0);
                                            ui.spinner();
                                        }
                                    }
                                }
                            } else {
                                ui.label(RichText::new("🧩").size(28.0));
                            }
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let status_icon = if rec.enabled { "🟢" } else { "⚪" };
                                    ui.label(RichText::new(format!("{status_icon} {}", rec.name)).strong().color(Color32::from_rgb(100, 200, 255)));
                                    ui.label(RichText::new(format!("({})", rec.class())).color(Color32::from_rgb(160, 160, 160)));
                                    match rec.source {
                                        plugins::PluginSource::Local => ui.label("📁 local"),
                                        plugins::PluginSource::CreatorStore => ui.label("☁️ store"),
                                    };
                                });
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("{} instance(s), {} script(s)", rec.instances, rec.scripts.len()));
                                    if let Some(id) = rec.asset_id {
                                        ui.label(format!("asset {id}"));
                                    }
                                });
                            });
                        });
                        if !rec.guis.is_empty() {
                            ui.label(RichText::new(format!("🧱 {} GUI element(s):", rec.guis.len())).weak());
                            for g in rec.guis.iter().take(6) {
                                ui.label(format!("   • {} ({}) — {} descendant(s)", g.name, g.class, g.descendants));
                            }
                            if rec.guis.len() > 6 {
                                ui.label(format!("   … and {} more", rec.guis.len() - 6));
                            }
                        }
                        if !rec.scripts.is_empty() {
                            ui.label(RichText::new("📜 scripts:").weak());
                            for (name, class) in rec.scripts.iter().take(8) {
                                ui.horizontal(|ui| {
                                    ui.label(format!("   • {name} ({class})"));
                                    if ui.small_button("Open").clicked() {
                                        action = Some(PluginAction::OpenScript(rec.id.clone(), name.clone()));
                                    }
                                });
                            }
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(if rec.enabled { "Disable" } else { "Enable" }).clicked() {
                                action = Some(PluginAction::Toggle(rec.id.clone()));
                            }
                            let is_running = running_id.as_deref() == Some(rec.id.as_str());
                            if is_running {
                                if ui.button(RichText::new("⏹ Stop").color(Color32::from_rgb(255,120,120))).clicked() {
                                    action = Some(PluginAction::Stop);
                                }
                            } else if ui.button("▶ Run in sandbox").clicked() {
                                action = Some(PluginAction::Run(rec.id.clone()));
                            }
                            if ui.button("📥 Insert into place").clicked() {
                                action = Some(PluginAction::Insert(rec.id.clone()));
                            }
                            if ui.small_button("🗑 Remove").clicked() {
                                action = Some(PluginAction::Delete(rec.id.clone()));
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            });

        match action {
            Some(PluginAction::Toggle(id)) => {
                let new_state = self.plugin_index.get(&id).map(|r| !r.enabled).unwrap_or(false);
                let _ = plugins::set_enabled(&mut self.plugin_index, &id, new_state);
                self.log_info(format!("Plugin '{id}' enabled={new_state}"));
            }
            Some(PluginAction::Run(id)) => {
                self.run_plugin(&id);
            }
            Some(PluginAction::Stop) => {
                self.stop_running_plugin();
            }
            Some(PluginAction::Insert(id)) => {
                self.insert_plugin_into_place(&id);
            }
            Some(PluginAction::Delete(id)) => {
                if plugins::delete_plugin(&mut self.plugin_index, &id).is_ok() {
                    self.status = format!("Removed plugin {id}");
                    self.log_info(format!("Removed plugin {id}"));
                }
            }
            Some(PluginAction::OpenScript(id, name)) => {
                self.open_plugin_script(&id, &name);
            }
            None => {}
        }
    }

    fn show_live_session_row(&mut self, ui: &mut egui::Ui) {
        // Pump server events so the UI reflects connect/disconnect even when
        // the user isn't on the plugins tab.
        self.drain_live_session_events();

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("🔴 Live Session").strong());
                let (dot, label) = match self.live_session.status {
                    live_session::SessionStatus::Stopped => (Color32::from_rgb(180, 180, 180), "stopped"),
                    live_session::SessionStatus::Listening => (Color32::from_rgb(255, 200, 80), "listening…"),
                    live_session::SessionStatus::Connected => (Color32::from_rgb(100, 255, 120), "● connected"),
                };
                ui.label(RichText::new(label).color(dot));
            });

            ui.horizontal(|ui| {
                ui.label("Port:");
                ui.add(egui::DragValue::new(&mut self.live_session.port).range(1024..=65535));
                match self.live_session.status {
                    live_session::SessionStatus::Stopped => {
                        if ui.button("Start").clicked() {
                            self.live_session.start();
                        }
                    }
                    _ => {
                        if ui.button("Stop").clicked() {
                            self.live_session.stop();
                        }
                    }
                }
                if ui.button("📋 Copy companion plugin .lua").clicked() {
                    jni_bridge::trigger_copy_to_clipboard(live_session::COMPANION_PLUGIN_SOURCE);
                    self.status = "Companion plugin source copied to clipboard".into();
                }
            });
            ui.label(
                RichText::new(
                    "Start the server, then install the companion plugin in Studio \
                     (Plugins tab → Plugins Folder; paste the .lua there and restart \
                     Studio). Connected Studio receives run_command / get_selection calls \
                     from the Command Bar.",
                )
                .weak(),
            );

            for line in self.live_session.log.iter().rev().take(6).collect::<Vec<_>>().iter().rev() {
                ui.label(RichText::new(line.as_str()).weak().monospace());
            }
        });
    }

    fn drain_live_session_events(&mut self) {
        let events: Vec<_> = self.live_session.poll_events();
        for ev in events {
            if let live_session::SessionEvent::Message(msg) = ev {
                self.log_info(format!("live: {} {}", msg.method, msg.params));
            }
        }
    }

    fn prompt_import_local_plugin(&mut self) {
        // Reuse the existing model picker; the file is just routed through
        // the plugin install path on the way back. We piggy-back on the
        // model-open picker and tag the URI in jni_bridge so drain_events
        // knows it's a plugin install, not a place/model insert.
        jni_bridge::trigger_open_model_for_plugin();
    }

    fn install_plugin_bytes(&mut self, name_hint: &str, source: plugins::PluginSource, asset_id: Option<u64>, bytes: &[u8]) {
        match plugins::add_plugin_from_bytes(&mut self.plugin_index, name_hint, source, asset_id, bytes) {
            Ok(rec) => {
                self.status = format!("Installed plugin '{}' ({} instances)", rec.name, rec.instances);
                self.log_info(format!(
                    "Installed plugin '{}' ({} scripts, {} GUI elements)",
                    rec.name,
                    rec.scripts.len(),
                    rec.guis.len()
                ));
            }
            Err(e) => {
                self.status = format!("Plugin install failed: {e}");
                self.log_error(format!("Plugin install failed: {e}"));
            }
        }
    }

    fn install_plugin_from_asset_id(&mut self) {
        let input = self.plugin_asset_id_input.trim().to_string();
        let id: u64 = match input.parse() {
            Ok(n) => n,
            Err(_) => {
                self.status = "Enter a numeric asset ID".into();
                return;
            }
        };
        let cookie = if self.roblosecurity_cookie.is_empty() { None } else { Some(self.roblosecurity_cookie.clone()) };
        let name_hint = format!("asset_{id}");
        self.status = format!("Downloading plugin asset {id}...");
        std::thread::spawn(move || {
            match RobloxApiClient::fetch_asset_payload_sync(id, cookie.as_deref()) {
                Ok(bytes) => jni_bridge::queue_plugin_bytes(name_hint, bytes),
                Err(e) => jni_bridge::queue_plugin_error(name_hint, e),
            }
        });
    }

    fn run_plugin(&mut self, id: &str) {
        use std::sync::atomic::Ordering;
        let Some(rec) = self.plugin_index.get(id).cloned() else { return; };
        if !rec.enabled {
            self.status = "Enable the plugin before running it".into();
            return;
        }
        if self.running_plugin_id.is_some() {
            self.status = "A plugin is already running; stop it first".into();
            return;
        }
        // Reset stop flag and mark running.
        self.plugin_stop_flag.store(false, Ordering::SeqCst);
        self.running_plugin_id = Some(rec.id.clone());

        let flag = self.plugin_stop_flag.clone();
        let plugin_id = rec.id.clone();
        std::thread::spawn(move || {
            let dom = match plugins::load_plugin_dom(&rec) {
                Ok(d) => d,
                Err(e) => {
                    let _ = plugin_log_channel().0.send(PluginLogLine::Error(format!("load failed: {e}")));
                    let _ = plugin_running_channel().0.send(plugin_id);
                    return;
                }
            };
            let mut ran = 0usize;
            for &child in dom.root().children() {
                if flag.load(Ordering::SeqCst) { break; }
                if let Some(inst) = dom.get_by_ref(child) {
                    if matches!(inst.class.as_str(), "Script" | "LocalScript" | "ModuleScript") {
                        let src = rbxl::get_source(&dom, child).unwrap_or_default();
                        let result = if inst.class == "ModuleScript" {
                            lua_runtime::run_module(&src, &rec.name)
                        } else {
                            lua_runtime::run_source(&src, &rec.name)
                        };
                        for line in result.lines { let _ = plugin_log_channel().0.send(PluginLogLine::Output(line)); }
                        ran += 1;
                    }
                }
            }
            let _ = plugin_log_channel().0.send(PluginLogLine::Done(ran));
            let _ = plugin_running_channel().0.send(plugin_id);
        });
        self.active_tab = ActiveTab::Command;
    }

    fn stop_running_plugin(&mut self) {
        use std::sync::atomic::Ordering;
        self.plugin_stop_flag.store(true, Ordering::SeqCst);
        self.status = "Stopping plugin...".into();
    }

    fn pump_plugin_thumbnails(&mut self) {
        if let Ok(rx) = plugin_thumb_rx().lock() {
            while let Ok(map) = rx.try_recv() {
                self.plugin_thumbnails.extend(map);
            }
        }
    }

    fn pump_plugin_logs(&mut self) {
        // If a background plugin run finished, clear the running marker.
        while let Some(finished_id) = plugin_running_channel().1.lock().ok().and_then(|r| r.try_recv().ok()) {
            if self.running_plugin_id.as_deref() == Some(&finished_id) {
                self.running_plugin_id = None;
            }
        }
        // Drain log lines into the command output.
        while let Some(line) = plugin_log_channel().1.lock().ok().and_then(|r| r.try_recv().ok()) {
            match line {
                PluginLogLine::Output(l) => self.command_output.push(l),
                PluginLogLine::Error(e) => {
                    self.command_output.push(lua_runtime::OutputLine {
                        level: lua_runtime::Level::Error,
                        text: e,
                    });
                }
                PluginLogLine::Done(n) => {
                    self.log_info(format!("Plugin ran {n} script(s) in sandbox"));
                }
            }
        }
    }

    fn insert_plugin_into_place(&mut self, id: &str) {
        let (Some(rec), Some(dom)) = (self.plugin_index.get(id).cloned(), self.dom.as_mut()) else {
            self.status = "Open a place first".into();
            return;
        };
        let parent = self.selected.unwrap_or_else(|| dom.root_ref());
        match plugins::insert_into_place(&rec, dom, parent) {
            Ok((first, count)) => {
                self.selected = Some(first);
                self.needs_3d_rebuild = true;
                self.status = format!("Inserted plugin '{}' ({} instances) into place", rec.name, count);
                self.log_info(format!("Inserted plugin contents into place: {count} instance(s)"));
            }
            Err(e) => {
                self.status = format!("Insert failed: {e}");
                self.log_error(format!("Insert plugin failed: {e}"));
            }
        }
    }

    fn open_plugin_script(&mut self, plugin_id: &str, script_name: &str) {
        let Some(rec) = self.plugin_index.get(plugin_id).cloned() else { return; };
        let Ok(dom) = plugins::load_plugin_dom(&rec) else { return; };
        // Find the matching script by name in the plugin DOM.
        let mut target = None;
        let mut stack = dom.root().children().to_vec();
        while let Some(r) = stack.pop() {
            if let Some(inst) = dom.get_by_ref(r) {
                if inst.name == script_name {
                    target = Some(r);
                    break;
                }
                stack.extend(inst.children());
            }
        }
        // We can't edit a plugin's script in place (the plugin DOM is a
        // separate file), but we can show its source in a read-only buffer by
        // inserting the script temporarily into the open place. For now just
        // open the source in the command bar output.
        if let Some(r) = target {
            if let Some(src) = rbxl::get_source(&dom, r) {
                self.command_output.push(lua_runtime::OutputLine {
                    level: lua_runtime::Level::Info,
                    text: format!("-- {}.{} --", rec.name, script_name),
                });
                for line in src.lines() {
                    self.command_output.push(lua_runtime::OutputLine {
                        level: lua_runtime::Level::Print,
                        text: line.to_string(),
                    });
                }
                self.active_tab = ActiveTab::Command;
            }
        }
    }

    // ------------------------------------------------------------------
    // Browse Roblox tab: group -> universes -> places, with thumbnails.
    // ------------------------------------------------------------------
    fn browse_load_group(&self, group_id: u64) {
        let cookie = self.roblosecurity_cookie();
        std::thread::spawn(move || {
            let cookie_str = cookie.as_deref().unwrap_or("");
            let client = match roblox_api::RobloxApiClient::web_client(cookie_str) {
                Ok(c) => c,
                Err(e) => {
                    jni_bridge::queue_browse_error(e);
                    return;
                }
            };
            let universes = match client.group_universes(group_id, 2) {
                Ok(u) => u,
                Err(e) => {
                    jni_bridge::queue_browse_error(e);
                    return;
                }
            };
            // Batch-fetch GameIcon thumbnails for all universes.
            let ids: Vec<u64> = universes.iter().map(|u| u.id).collect();
            let thumbs = client
                .thumbnails_batch(&ids, "GameIcon", "150x150")
                .unwrap_or_default();
            jni_bridge::queue_group_universes(group_id, universes, thumbs);
        });
    }

    fn browse_load_universe_places(&self, universe_id: u64) {
        let cookie = self.roblosecurity_cookie();
        std::thread::spawn(move || {
            let cookie_str = cookie.as_deref().unwrap_or("");
            let client = match roblox_api::RobloxApiClient::web_client(cookie_str) {
                Ok(c) => c,
                Err(e) => { jni_bridge::queue_browse_error(e); return; }
            };
            // Use the cookie-auth develop endpoint to list places; fall
            // back to root-place lookup on failure.
            match client.universe_places(universe_id) {
                Ok(v) => {
                    let mut places = Vec::new();
                    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
                        for p in arr {
                            if let (Some(id), Some(name)) = (
                                p.get("id").and_then(|i| i.as_u64()),
                                p.get("name").and_then(|n| n.as_str()),
                            ) {
                                places.push((id, name.to_string()));
                            }
                        }
                    }
                    jni_bridge::queue_universe_places(universe_id, places);
                }
                Err(e) => jni_bridge::queue_browse_error(e),
            }
        });
    }

    /// Build a read-only cookie Option for background threads.
    fn roblosecurity_cookie(&self) -> Option<String> {
        let c = self.roblosecurity_cookie.trim();
        if c.is_empty() { None } else { Some(c.to_string()) }
    }

    fn show_browse_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("🌐 Browse Roblox");
        ui.label(RichText::new(
            "Browse a group's experiences, see their icons, and open any place by downloading its .rbxl.",
        ).weak());
        ui.separator();

        // Group input.
        ui.horizontal(|ui| {
            ui.label("Group ID:");
            ui.add(
                egui::TextEdit::singleline(&mut self.browse_group_id)
                    .hint_text("e.g. 123456")
                    .desired_width(160.0),
            );
            if ui.button("🔍 Load group").clicked() {
                if let Ok(id) = self.browse_group_id.trim().parse::<u64>() {
                    self.browse_status = format!("Loading group {id}...");
                    self.browse_load_group(id);
                } else {
                    self.browse_status = "Enter a numeric group ID".into();
                }
            }
        });
        if !self.browse_status.is_empty() {
            ui.label(RichText::new(&self.browse_status).weak());
        }
        ui.separator();

        if self.browse_universes.is_empty() {
            ui.label(RichText::new("No experiences loaded yet.").weak());
        } else {
            // Snapshot the data we render so clicking "Open" (which mutates
            // self) doesn't conflict with the immutable borrow.
            let universes: Vec<roblox_api::GroupUniverse> = self.browse_universes.clone();
            let thumbs = self.browse_thumbnails.clone();
            let selected_universe = self.browse_selected_universe;
            let places: Vec<(u64, String)> = self.browse_places.clone();
            let mut open_place: Option<u64> = None;
            let mut load_places: Option<u64> = None;
            egui::ScrollArea::vertical()
                .id_salt("browse_universes_scroll")
                .show(ui, |ui| {
                    for univ in &universes {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                // Thumbnail: load/render the image if we
                                // have a URL; the cache downloads and
                                // uploads it to the GPU asynchronously.
                                if let Some(url) = thumbs.get(&univ.id) {
                                    match crate::thumbnails::get_or_load(ui.ctx(), url) {
                                        Some(tex) => {
                                            ui.add(
                                                egui::Image::from_texture(&tex)
                                                    .fit_to_exact_size(egui::vec2(96.0, 96.0))
                                                    .corner_radius(egui::CornerRadius::same(4)),
                                            );
                                        }
                                        None => {
                                            ui.add_space(96.0);
                                            ui.spinner();
                                        }
                                    }
                                }
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&univ.name).strong());
                                    if !univ.description.is_empty() {
                                        ui.label(RichText::new(&univ.description).weak());
                                    }
                                    if let Some(players) = univ.player_count {
                                        ui.label(format!("👥 {players} playing"));
                                    }
                                    ui.horizontal(|ui| {
                                        if ui.button("📂 Places").clicked() {
                                            load_places = Some(univ.id);
                                        }
                                        if univ.root_place_id.is_some() {
                                            if ui.button("🌐 Open root place").clicked() {
                                                if let Some(pid) = univ.root_place_id { open_place = Some(pid); }
                                            }
                                        }
                                    });
                                });
                            });

                            // If this universe is selected, show its places.
                            if selected_universe == Some(univ.id) {
                                ui.separator();
                                if places.is_empty() {
                                    ui.label(RichText::new("Loading places...").weak());
                                } else {
                                    for (pid, pname) in &places {
                                        ui.horizontal(|ui| {
                                            ui.label(format!("• {pname}"));
                                            if ui.button("📂 Open").clicked() {
                                                self.open_place_id_input = pid.to_string();
                                                self.open_place_from_roblox();
                                            }
                                        });
                                    }
                                }
                            }
                        });
                        ui.add_space(4.0);
                    }
                });
            // Apply any deferred click actions (now that the immutable
            // borrow of self.browse_* has been dropped).
            if let Some(uid) = load_places {
                self.browse_status = format!("Loading places...");
                self.browse_load_universe_places(uid);
            }
            if let Some(pid) = open_place {
                self.open_place_id_input = pid.to_string();
                self.open_place_from_roblox();
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
                                .hint_text("Open Cloud API key")
                                .desired_width(240.0),
                        );
                    });

                    ui.horizontal_wrapped(|ui| {
                        ui.label("Universe ID:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.open_cloud_universe_id)
                                .hint_text("e.g. 123456789")
                                .desired_width(120.0),
                        );

                        ui.label("Place ID:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.open_cloud_place_id)
                                .hint_text("e.g. 987654321")
                                .desired_width(120.0),
                        );
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
        match rbxl::save_place_as(dom, self.place_format) {
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
        let name = format!("place.{}", self.place_format.extension());
        jni_bridge::trigger_create_document(&name);
    }

    /// Launch the Android file picker for a local .rbxm/.rbxmx model file. The
    /// picked file is decoded and inserted as a subtree into the active place
    /// under the current selection (or the place root if nothing is selected),
    /// the exact same way a Creator Store download is inserted. This is the
    /// local-file counterpart to the "📥 Download & Insert Full Model" button.
    /// Download a place's `.rbxl`/`.rbxlx` from Roblox by place ID using the
    /// configured `.ROBLOSECURITY` cookie, then open it exactly like a local
    /// file. This works for places you can edit (Team Create or solo); it
    /// does NOT join a live session — it fetches the latest saved version.
    fn open_place_from_roblox(&mut self) {
        let id_str = self.open_place_id_input.trim();
        let place_id: u64 = match id_str.parse() {
            Ok(n) => n,
            Err(_) => {
                self.status = "Enter a numeric place ID".into();
                return;
            }
        };
        if self.roblosecurity_cookie.trim().is_empty() {
            self.status = "Set your .ROBLOSECURITY cookie in Settings first".into();
            self.log_error("Open from Roblox requires a .ROBLOSECURITY cookie");
            return;
        }
        let cookie = self.roblosecurity_cookie.trim().to_string();
        self.status = format!("Downloading place {place_id} from Roblox...");
        self.log_info(format!("Downloading place {place_id} via asset delivery"));
        std::thread::spawn(move || {
            match roblox_api::RobloxApiClient::fetch_asset_payload_sync(
                place_id,
                Some(&cookie),
            ) {
                Ok(bytes) => {
                    jni_bridge::queue_open_place_bytes(
                        format!("roblox-place-{place_id}"),
                        bytes,
                    );
                }
                Err(e) => {
                    jni_bridge::queue_open_place_error(
                        format!("place {place_id}"),
                        e,
                    );
                }
            }
        });
    }

    /// Publish the currently-open place to Roblox using Open Cloud (the
    /// only currently-supported upload path). The legacy ashx cookie
    /// gateway is gone, so this always talks to apis.roblox.com with the
    /// API key from Settings.
    fn publish_place_to_roblox(&mut self) {
        let Some(dom) = &self.dom else {
            self.status = "Open a place first".into();
            return;
        };
        if self.open_cloud_api_key.trim().is_empty() {
            self.status = "Set your Open Cloud API key in the Open Cloud tab".into();
            self.log_error("Publish requires an Open Cloud API key");
            return;
        }
        // Open Cloud only accepts binary .rbxl; force binary serialization
        // regardless of the on-disk format.
        let bytes = match rbxl::save_place(dom) {
            Ok(b) => b,
            Err(e) => {
                self.status = format!("Serialize failed: {e}");
                self.log_error(format!("Publish serialize: {e}"));
                return;
            }
        };
        let api_key = self.open_cloud_api_key.trim().to_string();
        let universe = self.open_cloud_universe_id.trim().to_string();
        let place = self.open_cloud_place_id.trim().to_string();
        let publish_live = self.open_cloud_publish_live;
        self.status = "Publishing place via Open Cloud...".into();
        self.log_info(format!(
            "Publishing place {place} (universe {universe}) via Open Cloud"
        ));
        std::thread::spawn(move || {
            let result = roblox_api::RobloxApiClient::publish_place_open_cloud(
                &api_key,
                &universe,
                &place,
                &bytes,
                publish_live,
            );
            match result {
                Ok(_msg) => jni_bridge::queue_publish_result(format!("place {place}"), Ok(())),
                Err(e) => jni_bridge::queue_publish_result(format!("place {place}"), Err(e)),
            }
        });
    }

    fn prompt_import_local_model(&mut self) {
        if self.dom.is_none() {
            self.status = "Open a .rbxl place first, then import a model into it".into();
            self.log_error("Cannot import local model: no place file open");
            return;
        }
        self.status = "Pick a local .rbxm / .rbxmx file to insert...".into();
        self.log_info("Opening system file picker for local .rbxm/.rbxmx model");
        jni_bridge::trigger_open_model_document();
    }

    /// Decode local model bytes (binary .rbxm, XML .rbxmx, gzipped, or Luau
    /// source — all handled by `rbxl::decode_model_bytes`) and merge every
    /// top-level instance into the active place under the current selection.
    fn insert_local_model(&mut self, uri: String, data: Vec<u8>) {
        let Some(dom) = self.dom.as_mut() else {
            self.status = "Open a .rbxl place first, then import a model into it".into();
            return;
        };

        // Snapshot the selection/root before borrowing dom mutably for decode.
        let parent = self
            .selected
            .and_then(|r| {
                // Only allow insertion under an instance that still exists and
                // can logically contain children (root DataModel always can).
                if dom.get_by_ref(r).is_some() {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| dom.root_ref());

        match rbxl::decode_model_bytes(&data) {
            Ok(source_dom) => {
                let (first_ref, count) = rbxl::insert_all_root_children(dom, parent, &source_dom);
                if count == 0 || first_ref.is_none() {
                    self.status = "Model file contained no insertable instances".into();
                    self.log_error(format!("Local model '{uri}' decoded but had 0 instances"));
                    return;
                }
                let first_ref = first_ref.unwrap();
                let name = dom
                    .get_by_ref(first_ref)
                    .map(|i| i.name.clone())
                    .unwrap_or_else(|| "Model".to_string());
                self.selected = Some(first_ref);
                self.needs_3d_rebuild = true;
                self.status =
                    format!("✅ Inserted local model '{name}' ({count} instances) from {uri}");
                self.log_info(format!(
                    "Imported local model '{name}' ({count} instances) from {uri}"
                ));
            }
            Err(e) => {
                self.status = format!("Failed to decode local model: {e}");
                self.log_error(format!(
                    "Failed to decode local model '{uri}': {e}. \
                     Supported: .rbxm (binary), .rbxmx (XML), gzipped, or .lua/.luau source."
                ));
            }
        }
    }
}
