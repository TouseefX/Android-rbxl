use egui::{Color32, Pos2, Stroke, Ui, Vec2};
use rbx_dom_weak::{
    types::{Ref, Variant},
    WeakDom,
};
use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraPreset {
    Isometric,
    Top,
    Front,
    Side,
}

// ----------------------------------------------------------------------------
// 3D Math: Vec3, Mat4, Ray
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };

    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(&self, other: &Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(&self, other: &Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    pub fn length_sq(&self) -> f32 {
        self.dot(self)
    }

    pub fn length(&self) -> f32 {
        self.length_sq().sqrt()
    }

    pub fn normalize(&self) -> Self {
        let len = self.length();
        if len > 1e-6 {
            Self {
                x: self.x / len,
                y: self.y / len,
                z: self.z / len,
            }
        } else {
            *self
        }
    }

    pub fn add(&self, other: &Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }

    pub fn sub(&self, other: &Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }

    pub fn mul_scalar(&self, s: f32) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Mat4 {
    pub m: [f32; 16], // Column-major
}

impl Mat4 {
    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Self {
        let f = target.sub(&eye).normalize();
        let s = f.cross(&up).normalize();
        let u = s.cross(&f);

        Self {
            m: [
                s.x, u.x, -f.x, 0.0,
                s.y, u.y, -f.y, 0.0,
                s.z, u.z, -f.z, 0.0,
                -s.dot(&eye), -u.dot(&eye), f.dot(&eye), 1.0,
            ],
        }
    }

    pub fn perspective(fov_y_rad: f32, aspect: f32, z_near: f32, z_far: f32) -> Self {
        let f = 1.0 / (fov_y_rad * 0.5).tan();
        let range_inv = 1.0 / (z_near - z_far);

        Self {
            m: [
                f / aspect, 0.0, 0.0, 0.0,
                0.0, f, 0.0, 0.0,
                0.0, 0.0, (z_near + z_far) * range_inv, -1.0,
                0.0, 0.0, 2.0 * z_near * z_far * range_inv, 0.0,
            ],
        }
    }

    pub fn mul(&self, other: &Self) -> Self {
        let mut out = [0.0; 16];
        for col in 0..4 {
            for row in 0..4 {
                out[col * 4 + row] = self.m[row] * other.m[col * 4]
                    + self.m[4 + row] * other.m[col * 4 + 1]
                    + self.m[8 + row] * other.m[col * 4 + 2]
                    + self.m[12 + row] * other.m[col * 4 + 3];
            }
        }
        Self { m: out }
    }

    pub fn transform_point(&self, p: &Vec3) -> (Vec3, f32) {
        let x = self.m[0] * p.x + self.m[4] * p.y + self.m[8] * p.z + self.m[12];
        let y = self.m[1] * p.x + self.m[5] * p.y + self.m[9] * p.z + self.m[13];
        let z = self.m[2] * p.x + self.m[6] * p.y + self.m[10] * p.z + self.m[14];
        let w = self.m[3] * p.x + self.m[7] * p.y + self.m[11] * p.z + self.m[15];

        if w.abs() > 1e-6 {
            let inv_w = 1.0 / w;
            (Vec3::new(x * inv_w, y * inv_w, z * inv_w), w)
        } else {
            (Vec3::new(x, y, z), w)
        }
    }
}

// ----------------------------------------------------------------------------
// Linear Spatial Instance & Ray Picking
// ----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RenderInstance {
    pub referent: Ref,
    pub name: String,
    pub position: Vec3,
    pub size: Vec3,
    pub color: Color32,
    pub transparency: f32,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
}

pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    pub fn intersects_aabb(&self, min: &Vec3, max: &Vec3) -> Option<f32> {
        let mut tmin = (min.x - self.origin.x) / self.direction.x;
        let mut tmax = (max.x - self.origin.x) / self.direction.x;
        if tmin > tmax {
            std::mem::swap(&mut tmin, &mut tmax);
        }

        let mut tymin = (min.y - self.origin.y) / self.direction.y;
        let mut tymax = (max.y - self.origin.y) / self.direction.y;
        if tymin > tymax {
            std::mem::swap(&mut tymin, &mut tymax);
        }

        if (tmin > tymax) || (tymin > tmax) {
            return None;
        }

        if tymin > tmin {
            tmin = tymin;
        }
        if tymax < tmax {
            tmax = tymax;
        }

        let mut tzmin = (min.z - self.origin.z) / self.direction.z;
        let mut tzmax = (max.z - self.origin.z) / self.direction.z;
        if tzmin > tzmax {
            std::mem::swap(&mut tzmin, &mut tzmax);
        }

        if (tmin > tzmax) || (tzmin > tmax) {
            return None;
        }

        if tzmin > tmin {
            tmin = tzmin;
        }

        if tmin >= 0.0 {
            Some(tmin)
        } else {
            None
        }
    }
}

// ----------------------------------------------------------------------------
// Studio Viewport Engine State with Full Camera Navigation
// ----------------------------------------------------------------------------

pub struct Viewport3D {
    pub yaw: f32,          // Horizontal orbit angle
    pub pitch: f32,        // Vertical orbit angle
    pub distance: f32,     // Distance from focus target
    pub target: Vec3,      // Orbit center / focus point
    pub show_grid: bool,
    pub show_wireframe: bool,
    pub move_speed: f32,
}

impl Default for Viewport3D {
    fn default() -> Self {
        Self {
            yaw: 0.785,   // 45 degrees
            pitch: 0.45,  // ~26 degrees isometric tilt
            distance: 40.0,
            target: Vec3::new(0.0, 2.0, 0.0),
            show_grid: true,
            show_wireframe: true,
            move_speed: 4.0,
        }
    }
}

impl Viewport3D {
    pub fn set_preset(&mut self, preset: CameraPreset) {
        match preset {
            CameraPreset::Isometric => {
                self.yaw = 0.785;
                self.pitch = 0.45;
            }
            CameraPreset::Top => {
                self.yaw = 0.0;
                self.pitch = 1.54; // 90 deg down
            }
            CameraPreset::Front => {
                self.yaw = 0.0;
                self.pitch = 0.0;
            }
            CameraPreset::Side => {
                self.yaw = PI * 0.5;
                self.pitch = 0.0;
            }
        }
    }

    pub fn focus_on(&mut self, pos: [f32; 3]) {
        self.target = Vec3::new(pos[0], pos[1], pos[2]);
    }

    pub fn move_forward(&mut self) {
        let forward = Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos()).normalize();
        self.target = self.target.add(&forward.mul_scalar(self.move_speed));
    }

    pub fn move_backward(&mut self) {
        let backward = Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos()).normalize();
        self.target = self.target.add(&backward.mul_scalar(self.move_speed));
    }

    pub fn move_left(&mut self) {
        let left = Vec3::new(-self.yaw.cos(), 0.0, self.yaw.sin()).normalize();
        self.target = self.target.add(&left.mul_scalar(self.move_speed));
    }

    pub fn move_right(&mut self) {
        let right = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin()).normalize();
        self.target = self.target.add(&right.mul_scalar(self.move_speed));
    }

    pub fn move_up(&mut self) {
        self.target.y += self.move_speed;
    }

    pub fn move_down(&mut self) {
        self.target.y -= self.move_speed;
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        dom: Option<&WeakDom>,
        selected: &mut Option<Ref>,
    ) {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::new(220.0, 300.0)),
            egui::Sense::click_and_drag(),
        );

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(26, 26, 30));

        // Touch Navigation: Drag to Orbit
        if response.dragged() {
            let delta = response.drag_delta();
            self.yaw -= delta.x * 0.008;
            self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.54, 1.54);
        }

        // Camera Position from Spherical Coordinates
        let cos_p = self.pitch.cos();
        let sin_p = self.pitch.sin();
        let cos_y = self.yaw.cos();
        let sin_y = self.yaw.sin();

        let eye = Vec3::new(
            self.target.x + self.distance * cos_p * sin_y,
            self.target.y + self.distance * sin_p,
            self.target.z + self.distance * cos_p * cos_y,
        );

        let view = Mat4::look_at(eye, self.target, Vec3::new(0.0, 1.0, 0.0));
        let aspect = rect.width() / rect.height().max(1.0);
        let proj = Mat4::perspective(60.0_f32.to_radians(), aspect, 0.5, 1000.0);
        let view_proj = proj.mul(&view);

        let screen_w = rect.width();
        let screen_h = rect.height();
        let screen_left = rect.left();
        let screen_top = rect.top();

        // Project world Vec3 to screen Pos2 + depth
        let project_to_screen = |world_p: &Vec3| -> Option<(Pos2, f32)> {
            let (clip, w) = view_proj.transform_point(world_p);
            if w <= 0.1 || clip.z < -1.0 || clip.z > 1.0 {
                return None;
            }
            let sx = screen_left + (clip.x * 0.5 + 0.5) * screen_w;
            let sy = screen_top + (-clip.y * 0.5 + 0.5) * screen_h;
            Some((Pos2::new(sx, sy), w))
        };

        // Render Ground Baseplate Grid
        if self.show_grid {
            let grid_half = 64.0;
            let step = 8.0;
            let mut x = -grid_half;
            while x <= grid_half {
                let p1 = Vec3::new(x, 0.0, -grid_half);
                let p2 = Vec3::new(x, 0.0, grid_half);
                if let (Some((s1, _)), Some((s2, _))) = (project_to_screen(&p1), project_to_screen(&p2)) {
                    let stroke_color = if x == 0.0 {
                        Color32::from_rgb(60, 110, 190) // Primary Z axis
                    } else {
                        Color32::from_rgb(45, 45, 50)
                    };
                    painter.line_segment([s1, s2], Stroke::new(1.0_f32, stroke_color));
                }
                x += step;
            }

            let mut z = -grid_half;
            while z <= grid_half {
                let p1 = Vec3::new(-grid_half, 0.0, z);
                let p2 = Vec3::new(grid_half, 0.0, z);
                if let (Some((s1, _)), Some((s2, _))) = (project_to_screen(&p1), project_to_screen(&p2)) {
                    let stroke_color = if z == 0.0 {
                        Color32::from_rgb(190, 60, 60) // Primary X axis
                    } else {
                        Color32::from_rgb(45, 45, 50)
                    };
                    painter.line_segment([s1, s2], Stroke::new(1.0_f32, stroke_color));
                }
                z += step;
            }
        }

        // Flatten DOM into linear array of 3D parts — ONLY FROM WORKSPACE!
        let mut instances: Vec<RenderInstance> = Vec::new();
        if let Some(dom) = dom {
            // Locate the Workspace service in the DOM
            let mut workspace_roots = Vec::new();
            for &child in dom.root().children() {
                if let Some(inst) = dom.get_by_ref(child) {
                    if inst.class == "Workspace" || inst.name == "Workspace" {
                        workspace_roots.push(child);
                    }
                }
            }

            // If no explicit Workspace instance found, exclude storage services
            let mut stack = if !workspace_roots.is_empty() {
                workspace_roots
            } else {
                dom.root()
                    .children()
                    .iter()
                    .copied()
                    .filter(|&r| {
                        if let Some(inst) = dom.get_by_ref(r) {
                            !matches!(
                                inst.class.as_str(),
                                "ReplicatedStorage"
                                    | "ServerStorage"
                                    | "Lighting"
                                    | "StarterGui"
                                    | "StarterPack"
                                    | "StarterPlayer"
                                    | "ServerScriptService"
                                    | "SoundService"
                                    | "Chat"
                                    | "Players"
                            )
                        } else {
                            false
                        }
                    })
                    .collect()
            };

            while let Some(r) = stack.pop() {
                if let Some(inst) = dom.get_by_ref(r) {
                    if matches!(
                        inst.class.as_str(),
                        "ReplicatedStorage" | "ServerStorage" | "Lighting" | "StarterGui" | "StarterPack"
                    ) {
                        continue;
                    }

                    stack.extend(inst.children());

                    let is_3d = matches!(
                        inst.class.as_str(),
                        "Part" | "WedgePart" | "CornerWedgePart" | "TrussPart" | "SpawnLocation"
                    );

                    if is_3d {
                        let pos = match inst.properties.get(&rbx_dom_weak::ustr("Position")) {
                            Some(Variant::Vector3(v)) => Vec3::new(v.x, v.y, v.z),
                            _ => Vec3::ZERO,
                        };
                        let size = match inst.properties.get(&rbx_dom_weak::ustr("Size")) {
                            Some(Variant::Vector3(v)) => Vec3::new(v.x.max(0.2), v.y.max(0.2), v.z.max(0.2)),
                            _ => Vec3::new(4.0, 1.2, 2.0),
                        };
                        let color = match inst.properties.get(&rbx_dom_weak::ustr("Color")) {
                            Some(Variant::Color3(c)) => {
                                Color32::from_rgb((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8)
                            }
                            Some(Variant::Color3uint8(c)) => Color32::from_rgb(c.r, c.g, c.b),
                            _ => Color32::from_rgb(163, 162, 165),
                        };
                        let transparency = match inst.properties.get(&rbx_dom_weak::ustr("Transparency")) {
                            Some(Variant::Float32(f)) => *f,
                            Some(Variant::Float64(f)) => *f as f32,
                            _ => 0.0,
                        };

                        let half_s = size.mul_scalar(0.5);
                        let aabb_min = pos.sub(&half_s);
                        let aabb_max = pos.add(&half_s);

                        instances.push(RenderInstance {
                            referent: r,
                            name: inst.name.clone(),
                            position: pos,
                            size,
                            color,
                            transparency,
                            aabb_min,
                            aabb_max,
                        });
                    }
                }
            }
        }

        // Tap Raycasting: Select 3D parts on screen tap
        if response.clicked() {
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                let mx = ((mouse_pos.x - screen_left) / screen_w) * 2.0 - 1.0;
                let my = -(((mouse_pos.y - screen_top) / screen_h) * 2.0 - 1.0);

                let forward = self.target.sub(&eye).normalize();
                let right = forward.cross(&Vec3::new(0.0, 1.0, 0.0)).normalize();
                let up = right.cross(&forward).normalize();

                let tan_half_fov = (60.0_f32.to_radians() * 0.5).tan();
                let ray_dir = forward
                    .add(&right.mul_scalar(mx * tan_half_fov * aspect))
                    .add(&up.mul_scalar(my * tan_half_fov))
                    .normalize();

                let ray = Ray {
                    origin: eye,
                    direction: ray_dir,
                };

                let mut nearest_hit = None;
                let mut min_t = f32::INFINITY;

                for inst in &instances {
                    if let Some(t) = ray.intersects_aabb(&inst.aabb_min, &inst.aabb_max) {
                        if t < min_t {
                            min_t = t;
                            nearest_hit = Some(inst.referent);
                        }
                    }
                }

                if let Some(hit_ref) = nearest_hit {
                    *selected = Some(hit_ref);
                }
            }
        }

        // Depth-sort visible instances (Painter's algorithm: back-to-front rendering)
        let mut sorted_instances: Vec<(f32, &RenderInstance)> = instances
            .iter()
            .map(|inst| {
                let dist_sq = inst.position.sub(&eye).length_sq();
                (dist_sq, inst)
            })
            .collect();

        sorted_instances.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Sun Direction for Studio Lighting
        let sun_dir = Vec3::new(0.4, 0.8, 0.5).normalize();

        // Draw Shaded 3D Instances with Solid Outward CCW Normals
        for (_, inst) in sorted_instances {
            let is_sel = *selected == Some(inst.referent);
            let pos = inst.position;
            let half = inst.size.mul_scalar(0.5);

            // 8 Bounding Vertices
            // 0: -X -Y -Z, 1: +X -Y -Z, 2: +X +Y -Z, 3: -X +Y -Z
            // 4: -X -Y +Z, 5: +X -Y +Z, 6: +X +Y +Z, 7: -X +Y +Z
            let v = [
                Vec3::new(pos.x - half.x, pos.y - half.y, pos.z - half.z),
                Vec3::new(pos.x + half.x, pos.y - half.y, pos.z - half.z),
                Vec3::new(pos.x + half.x, pos.y + half.y, pos.z - half.z),
                Vec3::new(pos.x - half.x, pos.y + half.y, pos.z - half.z),
                Vec3::new(pos.x - half.x, pos.y - half.y, pos.z + half.z),
                Vec3::new(pos.x + half.x, pos.y - half.y, pos.z + half.z),
                Vec3::new(pos.x + half.x, pos.y + half.y, pos.z + half.z),
                Vec3::new(pos.x - half.x, pos.y + half.y, pos.z + half.z),
            ];

            let mut proj_v = [None; 8];
            for i in 0..8 {
                proj_v[i] = project_to_screen(&v[i]);
            }

            // 6 Faces with strict counter-clockwise (CCW) winding viewed from the outside:
            // Top (+Y): looking down -> [7, 6, 2, 3] (Normal: 0, 1, 0)
            // Bottom (-Y): looking up -> [0, 1, 5, 4] (Normal: 0, -1, 0)
            // Front (+Z): looking from +Z -> [4, 5, 6, 7] (Normal: 0, 0, 1)
            // Back (-Z): looking from -Z -> [1, 0, 3, 2] (Normal: 0, 0, -1)
            // Right (+X): looking from +X -> [5, 1, 2, 6] (Normal: 1, 0, 0)
            // Left (-X): looking from -X -> [0, 4, 7, 3] (Normal: -1, 0, 0)
            let faces: [([usize; 4], Vec3); 6] = [
                ([7, 6, 2, 3], Vec3::new(0.0, 1.0, 0.0)),  // Top (+Y)
                ([0, 1, 5, 4], Vec3::new(0.0, -1.0, 0.0)), // Bottom (-Y)
                ([4, 5, 6, 7], Vec3::new(0.0, 0.0, 1.0)),  // Front (+Z)
                ([1, 0, 3, 2], Vec3::new(0.0, 0.0, -1.0)), // Back (-Z)
                ([5, 1, 2, 6], Vec3::new(1.0, 0.0, 0.0)),  // Right (+X)
                ([0, 4, 7, 3], Vec3::new(-1.0, 0.0, 0.0)), // Left (-X)
            ];

            let base_r = inst.color.r() as f32;
            let base_g = inst.color.g() as f32;
            let base_b = inst.color.b() as f32;

            for (idx_list, normal) in faces {
                let p0 = proj_v[idx_list[0]];
                let p1 = proj_v[idx_list[1]];
                let p2 = proj_v[idx_list[2]];
                let p3 = proj_v[idx_list[3]];

                if let (Some((v0, _)), Some((v1, _)), Some((v2, _)), Some((v3, _))) = (p0, p1, p2, p3) {
                    // Check if face is facing camera in 3D world space
                    let to_cam = eye.sub(&pos).normalize();
                    if normal.dot(&to_cam) > -0.05 {
                        let diffuse = normal.dot(&sun_dir).max(0.0);
                        let shade = 0.45 + 0.55 * diffuse;

                        let r = (base_r * shade).clamp(0.0, 255.0) as u8;
                        let g = (base_g * shade).clamp(0.0, 255.0) as u8;
                        let b = (base_b * shade).clamp(0.0, 255.0) as u8;
                        let face_color = Color32::from_rgb(r, g, b);

                        painter.add(egui::Shape::convex_polygon(
                            vec![v0, v1, v2, v3],
                            face_color,
                            Stroke::NONE,
                        ));

                        if self.show_wireframe {
                            let stroke_color = if is_sel {
                                Color32::from_rgb(0, 220, 255)
                            } else {
                                Color32::from_rgba_unmultiplied(0, 0, 0, 45)
                            };
                            let stroke_width = if is_sel { 2.5_f32 } else { 1.0_f32 };
                            painter.line_segment([v0, v1], Stroke::new(stroke_width, stroke_color));
                            painter.line_segment([v1, v2], Stroke::new(stroke_width, stroke_color));
                            painter.line_segment([v2, v3], Stroke::new(stroke_width, stroke_color));
                            painter.line_segment([v3, v0], Stroke::new(stroke_width, stroke_color));
                        }
                    }
                }
            }

            // Selected Part 3D Transform Gizmo & Name Tag
            if is_sel {
                if let Some((center_screen, _)) = project_to_screen(&inst.position) {
                    painter.circle_filled(center_screen, 4.5, Color32::from_rgb(0, 220, 255));

                    // 3D Axis Gizmo Lines (Red = X, Green = Y, Blue = Z)
                    let gizmo_len = 5.0;
                    let p_x = inst.position.add(&Vec3::new(gizmo_len, 0.0, 0.0));
                    let p_y = inst.position.add(&Vec3::new(0.0, gizmo_len, 0.0));
                    let p_z = inst.position.add(&Vec3::new(0.0, 0.0, gizmo_len));

                    if let Some((sx, _)) = project_to_screen(&p_x) {
                        painter.line_segment([center_screen, sx], Stroke::new(3.0_f32, Color32::from_rgb(255, 60, 60)));
                    }
                    if let Some((sy, _)) = project_to_screen(&p_y) {
                        painter.line_segment([center_screen, sy], Stroke::new(3.0_f32, Color32::from_rgb(60, 255, 60)));
                    }
                    if let Some((sz, _)) = project_to_screen(&p_z) {
                        painter.line_segment([center_screen, sz], Stroke::new(3.0_f32, Color32::from_rgb(60, 120, 255)));
                    }

                    painter.text(
                        Pos2::new(center_screen.x, center_screen.y - 14.0),
                        egui::Align2::CENTER_CENTER,
                        format!("🧱 {}", inst.name),
                        egui::FontId::proportional(12.0),
                        Color32::from_rgb(100, 230, 255),
                    );
                }
            }
        }

        // Viewport Header Overlay
        let info_pos = Pos2::new(rect.left() + 10.0, rect.top() + 10.0);
        painter.text(
            info_pos,
            egui::Align2::LEFT_TOP,
            format!("🎥 Workspace Viewport | Parts: {} | Cam: {:.0}°, {:.0}°", instances.len(), self.yaw.to_degrees(), self.pitch.to_degrees()),
            egui::FontId::proportional(12.0),
            Color32::from_rgb(180, 180, 190),
        );
    }
}
