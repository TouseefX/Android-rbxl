// ============================================================================
// viewport3d_bevy.rs  —  egui bridge for the Bevy 3D renderer
// ----------------------------------------------------------------------------
// Drop-in replacement for the CPU software `viewport3d::Viewport3D` that keeps
// the exact same public API (`render`, `set_preset`, `focus_on`, `move_*`,
// `show_grid`/`show_wireframe`/`show_skybox`/`move_speed`) so `app.rs` barely
// changes — but renders with Bevy on the GPU.
//
// How it works
// ------------
// 1. A background Bevy thread owns a headless Bevy app (no window) that
//    renders the scene to an *offscreen texture* and reads the pixels back.
// 2. The egui thread sends scene/camera/settings changes to that thread over an
//    mpsc channel as plain data (`Vec<PartGeo>`), and receives RGBA frames back.
// 3. Each RGBA frame is shown as an `egui::ColorImage` in the Viewport tab.
//
// IMPORTANT / EXPERIMENTAL
// ------------------------
// Embedding Bevy inside an eframe/egui app is the hardest part of this port.
// The pixel readback relies on Bevy's `bevy_render/gpu_readback` API, which
// churns between minor versions and must be validated on a real device/GPU.
// If you just want to SEE the renderer working first, build and run the
// standalone example instead:
//
//     cargo run --release --example render_rbxl -- path/to/place.rbxl
//
// That example runs Bevy in a normal window and needs no egui/readback.
// ============================================================================

use crate::bevy_rbxl::{
    extract_geometry, update_orbit_camera, OrbitCamera, RbxViewportSettings, TextureRegistry,
};
use crate::bevy_rbxl::PartGeo;
use crate::bevy_rbxl as rbx;
use bevy::app::App;
use bevy::ecs::query::With;
use egui::{Color32, ColorImage, Pos2, Rect, Vec2};
use rbx_dom_weak::WeakDom;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

// ----------------------------------------------------------------------------
// Cross-thread messages
// ----------------------------------------------------------------------------

pub enum ViewportMsg {
    /// Replace the whole scene (geometry) and update camera/settings.
    SetScene {
        parts: Vec<PartGeo>,
        camera: OrbitCamera,
        settings: RbxViewportSettings,
    },
    SetCamera(OrbitCamera),
    SetSettings(RbxViewportSettings),
    Stop,
}

pub enum FrameMsg {
    /// A rendered frame ready to display.
    Frame { w: u32, h: u32, rgba: Vec<u8> },
    /// Thread + renderer are up and rendering.
    Ready,
    Error(String),
}

// ----------------------------------------------------------------------------
// egui widget state
// ----------------------------------------------------------------------------

pub struct BevyViewport3D {
    // Mirror the old Viewport3D fields so app.rs keeps compiling.
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: crate::viewport3d::Vec3,
    pub show_grid: bool,
    pub show_wireframe: bool,
    pub show_skybox: bool,
    pub move_speed: f32,
    pub initialized_camera: bool,

    // Bevy thread handles.
    tx: Option<Sender<ViewportMsg>>,
    rx: Option<Receiver<FrameMsg>>,
    thread: Option<JoinHandle<()>>,

    // Latest displayed frame.
    latest_image: Option<ColorImage>,
    latest_tex: Option<egui::TextureHandle>,
    last_frame_w: u32,
    last_frame_h: u32,
    status: String,

}

impl Default for BevyViewport3D {
    fn default() -> Self {
        Self {
            yaw: 0.785,
            pitch: 0.45,
            distance: 65.0,
            target: crate::viewport3d::Vec3::new(0.0, 6.0, 0.0),
            show_grid: false,
            show_wireframe: false,
            show_skybox: true,
            move_speed: 4.0,
            initialized_camera: false,
            tx: None,
            rx: None,
            thread: None,
            latest_image: None,
            latest_tex: None,
            last_frame_w: 0,
            last_frame_h: 0,
            status: "Bevy renderer idle".into(),
        }
    }
}

/// Ensure the background Bevy thread is running (called lazily on first render).
fn start_bevy_thread(widget: &mut BevyViewport3D) {
    if widget.thread.is_some() {
        return;
    }
    let (tx, rx) = mpsc::channel::<ViewportMsg>();
    let (ftx, frx) = mpsc::channel::<FrameMsg>();
    let init_cam = widget.orbit();
    let init_settings = widget.settings();
    let handle = thread::Builder::new()
        .name("bevy-rbxl-renderer".into())
        .spawn(move || bevy_thread_main(rx, ftx, init_cam, init_settings))
        .expect("failed to spawn Bevy render thread");
    widget.tx = Some(tx);
    widget.rx = Some(frx);
    widget.thread = Some(handle);
    widget.status = "starting Bevy renderer...".into();
}

impl Drop for BevyViewport3D {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(ViewportMsg::Stop);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl BevyViewport3D {
    fn orbit(&self) -> OrbitCamera {
        OrbitCamera {
            yaw: self.yaw,
            pitch: self.pitch,
            distance: self.distance,
            target: crate::viewport3d::Vec3::new(self.target.x, self.target.y, self.target.z),
        }
    }

    fn settings(&self) -> RbxViewportSettings {
        RbxViewportSettings {
            show_grid: self.show_grid,
            show_wireframe: self.show_wireframe,
            show_skybox: self.show_skybox,
            move_speed: self.move_speed,
            ..Default::default()
        }
    }

    pub fn set_preset(&mut self, preset: crate::viewport3d::CameraPreset) {
        let mut cam = self.orbit();
        match preset {
            crate::viewport3d::CameraPreset::Isometric => {
                cam.yaw = 0.785;
                cam.pitch = 0.45;
            }
            crate::viewport3d::CameraPreset::Top => {
                cam.yaw = 0.0;
                cam.pitch = 1.54;
            }
            crate::viewport3d::CameraPreset::Front => {
                cam.yaw = 0.0;
                cam.pitch = 0.15;
            }
            crate::viewport3d::CameraPreset::Side => {
                cam.yaw = std::f32::consts::PI * 0.5;
                cam.pitch = 0.15;
            }
        }
        self.yaw = cam.yaw;
        self.pitch = cam.pitch;
    }

    pub fn focus_on(&mut self, pos: [f32; 3]) {
        self.target = crate::viewport3d::Vec3::new(pos[0], pos[1], pos[2]);
    }

    pub fn move_forward(&mut self) {
        let f = crate::viewport3d::Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos()).normalize();
        self.target = self.target.add(&f.mul_scalar(self.move_speed));
    }
    pub fn move_backward(&mut self) {
        let f = crate::viewport3d::Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos()).normalize();
        self.target = self.target.add(&f.mul_scalar(self.move_speed));
    }
    pub fn move_left(&mut self) {
        let l = crate::viewport3d::Vec3::new(-self.yaw.cos(), 0.0, self.yaw.sin()).normalize();
        self.target = self.target.add(&l.mul_scalar(self.move_speed));
    }
    pub fn move_right(&mut self) {
        let r = crate::viewport3d::Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin()).normalize();
        self.target = self.target.add(&r.mul_scalar(self.move_speed));
    }
    pub fn move_up(&mut self) {
        self.target.y += self.move_speed;
    }
    pub fn move_down(&mut self) {
        self.target.y -= self.move_speed;
    }

    /// The main egui widget: renders the latest Bevy frame and forwards input.
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        dom: Option<&WeakDom>,
        _selected: &mut Option<rbx_dom_weak::types::Ref>,
        _cookie_opt: Option<&str>,
    ) {
        start_bevy_thread(self);

        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::new(220.0, 300.0)),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);

        // Touch drag to orbit.
        if response.dragged() {
            let delta = response.drag_delta();
            self.yaw -= delta.x * 0.008;
            self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.54, 1.54);
        }
        // Pinch zoom approximated by scroll.
        if let Some(s) = response.hover_pos() {
            let _ = s;
        }
        if ui.input(|i| i.smooth_scroll_delta.y.abs() > 0.0) {
            let s = ui.input(|i| i.smooth_scroll_delta.y);
            self.distance = (self.distance - s * 0.1).clamp(8.0, 300.0);
        }

        // Extract geometry (plain data) and send to the Bevy thread each frame.
        // TODO: throttle / diff against a dom generation counter to avoid
        // re-sending on every frame for large places.
        if let Some(dom) = dom {
            let parts = extract_geometry(dom, None);
            let camera = self.orbit();
            let settings = self.settings();
            if let Some(tx) = &self.tx {
                let _ = tx.send(ViewportMsg::SetScene {
                    parts,
                    camera,
                    settings,
                });
            }
        }

        // Send camera updates (cheap, throttled by caller's drag rate).
        if let Some(tx) = &self.tx {
            let _ = tx.send(ViewportMsg::SetCamera(self.orbit()));
        }

        // Drain incoming frames.
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    FrameMsg::Frame { w, h, rgba } => {
                        let img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                        self.last_frame_w = w;
                        self.last_frame_h = h;
                        self.latest_image = Some(img);
                        self.status = format!("Bevy · {w}×{h}");
                    }
                    FrameMsg::Ready => self.status = "Bevy renderer ready".into(),
                    FrameMsg::Error(e) => {
                        self.status = format!("Bevy error: {e}");
                    }
                }
            }
        }

        // Upload the latest frame to an egui texture and draw it.
        if let Some(img) = self.latest_image.take() {
            let ctx = ui.ctx().clone();
            let handle = ctx.load_texture("bevy_viewport", img, egui::TextureOptions::LINEAR);
            self.latest_tex = Some(handle);
        }

        if let Some(tex) = &self.latest_tex {
            let size = if self.last_frame_w > 0 && self.last_frame_h > 0 {
                Vec2::new(self.last_frame_w as f32, self.last_frame_h as f32)
            } else {
                rect.size()
            };
            // Fit image into the rect preserving aspect ratio.
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let draw_size = Vec2::new(size.x * scale, size.y * scale);
            let origin = rect.center() - draw_size * 0.5;
            painter.image(
                tex.id(),
                Rect::from_min_size(origin, draw_size),
                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            // Placeholder until first frame.
            painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 22, 26));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &self.status,
                egui::FontId::proportional(16.0),
                Color32::from_rgb(180, 200, 220),
            );
        }
    }
}

// ============================================================================
// Background Bevy thread
// ============================================================================

/// Fixed offscreen render resolution (egui scales it to fit).
const OFF_W: u32 = 640;
const OFF_H: u32 = 480;

/// Resource holding the frame sender so render systems can push frames.
#[derive(bevy::prelude::Resource)]
struct FrameTx(Sender<FrameMsg>);

/// Pending scene data passed to the rebuild system via a resource.
#[derive(bevy::prelude::Resource)]
struct PendingScene {
    parts: Vec<PartGeo>,
    settings: RbxViewportSettings,
}

fn bevy_thread_main(
    rx: Receiver<ViewportMsg>,
    ftx: Sender<FrameMsg>,
    init_camera: OrbitCamera,
    init_settings: RbxViewportSettings,
) {
    use bevy::prelude::*;

    let mut app = bevy::app::App::new();
    // Minimal engine + renderer, no window.
    app.add_plugins(MinimalPlugins);
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.add_plugins(bevy::render::RenderPlugin::default());
    app.add_plugins(bevy::core_pipeline::CorePipelinePlugin);
    app.add_plugins(bevy::pbr::PbrPlugin::default());
    // GPU readback (Bevy 0.17 API): Readback component + GpuReadbackPlugin.
    app.add_plugins(bevy::render::gpu_readback::GpuReadbackPlugin::default());

    app.insert_resource(init_camera);
    app.insert_resource(init_settings.clone());
    app.insert_resource(TextureRegistry::default());
    app.insert_resource(FrameTx(ftx));
    // Without a light, PBR materials render black.
    app.insert_resource(bevy::light::AmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
        ..default()
    });

    // Spawn the offscreen camera that renders into an image and reads it back.
    app.add_systems(Startup, spawn_offscreen_camera);
    // Keep the camera transform in sync with the OrbitCamera resource, and
    // rebuild the scene when a PendingScene is submitted.
    app.add_systems(Update, (update_orbit_camera, rebuild_system));

    let _ = app
        .world_mut()
        .resource_mut::<FrameTx>()
        .0
        .send(FrameMsg::Ready);

    let mut running = true;
    while running {
        let mut pending_scene: Option<Vec<PartGeo>> = None;
        let mut pending_settings = init_settings.clone();
        while let Ok(msg) = rx.try_recv() {
            match msg {
                ViewportMsg::SetScene { parts, camera, settings } => {
                    pending_scene = Some(parts);
                    pending_settings = settings;
                    *app.world_mut().resource_mut::<OrbitCamera>() = camera;
                }
                ViewportMsg::SetCamera(c) => {
                    *app.world_mut().resource_mut::<OrbitCamera>() = c;
                }
                ViewportMsg::SetSettings(s) => pending_settings = s,
                ViewportMsg::Stop => running = false,
            }
        }

        if pending_scene.is_some() {
            rebuild_scene(&mut app, pending_scene.take().unwrap(), pending_settings.clone());
        }

        app.update();

        if !running {
            break;
        }
    }
}

/// Spawn a camera that renders to an offscreen image with a `Readback`
/// component. Every frame the completed RGBA buffer triggers the entity's
/// `ReadbackComplete` observer, which forwards the frame to egui.
fn spawn_offscreen_camera(
    mut commands: bevy::ecs::system::Commands,
    mut images: bevy::ecs::system::ResMut<bevy::asset::Assets<bevy::image::Image>>,
) {
    use bevy::camera::{Camera, Camera3d, ClearColorConfig, PerspectiveProjection, Projection};
    use bevy::render::gpu_readback::{Readback, ReadbackComplete};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    use bevy::prelude::*;
    use bevy::asset::RenderAssetUsages;

    let img = bevy::image::Image::new(
        Extent3d { width: OFF_W, height: OFF_H, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0u8; (OFF_W * OFF_H * 4) as usize],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let handle = images.add(img);
    // Wrap the SAME image the camera renders into, so the readback returns the
    // rendered frame.
    let readback = Readback::texture(handle.clone());

    let cam = OrbitCamera::default();
    let (eye_b, target_b) = rbx::orbit_eye_target_b(&cam);
    commands
        .spawn((
            Camera3d::default(),
            Camera {
                target: bevy::camera::RenderTarget::Image(handle.into()),
                clear_color: ClearColorConfig::Custom(Color::srgb(0.45, 0.66, 0.95)),
                ..default()
            },
            Transform::from_translation(eye_b).looking_at(target_b, Vec3::Y),
            Projection::Perspective(PerspectiveProjection {
                fov: 60f32.to_radians(),
                ..default()
            }),
            rbx::RbxCamera,
            readback,
            Visibility::default(),
        ))
        // Observer fires when a ReadbackComplete event is triggered on this entity.
        .observe(|trigger: bevy::ecs::observer::On<ReadbackComplete>, tx: bevy::ecs::system::Res<FrameTx>| {
            let data = &trigger.event().data;
            let _ = tx.0.send(FrameMsg::Frame {
                w: OFF_W,
                h: OFF_H,
                rgba: data.clone(),
            });
        });
    // Directional sun light so parts are lit.
    commands.spawn((
        bevy::light::DirectionalLight {
            illuminance: 30_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(50.0, 80.0, -40.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Queue a scene rebuild by inserting a `PendingScene` resource; the Update
/// system picks it up on the next frame.
fn rebuild_scene(app: &mut App, parts: Vec<PartGeo>, settings: RbxViewportSettings) {
    app.world_mut().insert_resource(PendingScene { parts, settings });
}

/// Rebuild system: when `PendingScene` has parts, despawn the old scene root
/// and spawn the new one, then clear the pending flag.
fn rebuild_system(
    mut commands: bevy::ecs::system::Commands,
    mut meshes: bevy::ecs::system::ResMut<bevy::asset::Assets<bevy::mesh::Mesh>>,
    mut mats: bevy::ecs::system::ResMut<bevy::asset::Assets<bevy::pbr::StandardMaterial>>,
    mut imgs: bevy::ecs::system::ResMut<bevy::asset::Assets<bevy::image::Image>>,
    mut tex: bevy::ecs::system::ResMut<TextureRegistry>,
    mut pending: bevy::ecs::system::ResMut<PendingScene>,
    old_scene: bevy::ecs::system::Query<bevy::ecs::entity::Entity, With<rbx::RbxSceneRoot>>,
) {
    if pending.parts.is_empty() {
        return;
    }
    for e in old_scene.iter() {
        commands.entity(e).despawn();
    }
    rbx::spawn_scene(
        &mut commands,
        &mut meshes,
        &mut mats,
        &mut imgs,
        &mut tex,
        &pending.parts,
        &pending.settings,
    );
    pending.parts.clear();
}
