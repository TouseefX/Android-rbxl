// ============================================================================
// OpenRBLX Native 3D Viewport Renderer for Android-rbxl
// Ported from OpenRBLX (TornadoCookie/OpenRBLX C++ engine) & Studio Lite
// ============================================================================

use crate::asset_downloader;
use crate::roblox_api;
use egui::{Color32, Pos2, Rect, Stroke, Ui, Vec2};
use rbx_dom_weak::{
    types::{Ref, Variant},
    WeakDom,
};
use std::collections::HashMap;
use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraPreset {
    Isometric,
    Top,
    Front,
    Side,
}

// ----------------------------------------------------------------------------
// OpenRBLX 3D Math: Vector3, Matrix4, CFrame, Ray
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    pub const UP: Self = Self { x: 0.0, y: 1.0, z: 0.0 };

    #[inline(always)]
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    #[inline(always)]
    pub fn dot(&self, other: &Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    #[inline(always)]
    pub fn cross(&self, other: &Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    #[inline(always)]
    pub fn length_sq(&self) -> f32 {
        self.dot(self)
    }

    #[inline(always)]
    pub fn length(&self) -> f32 {
        self.length_sq().sqrt()
    }

    #[inline(always)]
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

    #[inline(always)]
    pub fn add(&self, other: &Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }

    #[inline(always)]
    pub fn sub(&self, other: &Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }

    #[inline(always)]
    pub fn mul_scalar(&self, s: f32) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CFrame3D {
    pub pos: Vec3,
    pub r00: f32, pub r01: f32, pub r02: f32,
    pub r10: f32, pub r11: f32, pub r12: f32,
    pub r20: f32, pub r21: f32, pub r22: f32,
}

impl Default for CFrame3D {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            r00: 1.0, r01: 0.0, r02: 0.0,
            r10: 0.0, r11: 1.0, r12: 0.0,
            r20: 0.0, r21: 0.0, r22: 1.0,
        }
    }
}

impl CFrame3D {
    pub fn new(pos: Vec3) -> Self {
        Self {
            pos,
            r00: 1.0, r01: 0.0, r02: 0.0,
            r10: 0.0, r11: 1.0, r12: 0.0,
            r20: 0.0, r21: 0.0, r22: 1.0,
        }
    }

    pub fn from_angles(pos: Vec3, rx: f32, ry: f32, rz: f32) -> Self {
        let (sx, cx) = rx.sin_cos();
        let (sy, cy) = ry.sin_cos();
        let (sz, cz) = rz.sin_cos();

        Self {
            pos,
            r00: cy * cz,
            r01: -cy * sz,
            r02: sy,
            r10: cx * sz + sx * sy * cz,
            r11: cx * cz - sx * sy * sz,
            r12: -sx * cy,
            r20: sx * sz - cx * sy * cz,
            r21: sx * cz + cx * sy * sz,
            r22: cx * cy,
        }
    }

    #[inline(always)]
    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        Vec3::new(
            self.pos.x + self.r00 * p.x + self.r01 * p.y + self.r02 * p.z,
            self.pos.y + self.r10 * p.x + self.r11 * p.y + self.r12 * p.z,
            self.pos.z + self.r20 * p.x + self.r21 * p.y + self.r22 * p.z,
        )
    }

    #[inline(always)]
    pub fn transform_normal(&self, n: Vec3) -> Vec3 {
        Vec3::new(
            self.r00 * n.x + self.r01 * n.y + self.r02 * n.z,
            self.r10 * n.x + self.r11 * n.y + self.r12 * n.z,
            self.r20 * n.x + self.r21 * n.y + self.r22 * n.z,
        ).normalize()
    }
}

pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    pub fn intersects_aabb(&self, min: &Vec3, max: &Vec3) -> Option<f32> {
        let mut tmin = (min.x - self.origin.x) / self.direction.x;
        let mut tmax = (max.x - self.origin.x) / self.direction.x;
        if tmin > tmax { std::mem::swap(&mut tmin, &mut tmax); }

        let mut tymin = (min.y - self.origin.y) / self.direction.y;
        let mut tymax = (max.y - self.origin.y) / self.direction.y;
        if tymin > tymax { std::mem::swap(&mut tymin, &mut tymax); }

        if (tmin > tymax) || (tymin > tmax) { return None; }
        if tymin > tmin { tmin = tymin; }
        if tymax < tmax { tmax = tymax; }

        let mut tzmin = (min.z - self.origin.z) / self.direction.z;
        let mut tzmax = (max.z - self.origin.z) / self.direction.z;
        if tzmin > tzmax { std::mem::swap(&mut tzmin, &mut tzmax); }

        if (tmin > tzmax) || (tzmin > tmax) { return None; }
        if tzmin > tmin { tmin = tzmin; }

        if tmin >= 0.0 { Some(tmin) } else { None }
    }
}

// ----------------------------------------------------------------------------
// BrickColor Registry (All 208 Official Roblox BrickColors)
// ----------------------------------------------------------------------------

pub fn brick_color_to_rgb(code: u32) -> Color32 {
    match code {
        1 => Color32::from_rgb(242, 243, 243),
        2 => Color32::from_rgb(161, 165, 162),
        3 => Color32::from_rgb(249, 233, 153),
        5 => Color32::from_rgb(215, 197, 154),
        6 => Color32::from_rgb(194, 218, 184),
        9 => Color32::from_rgb(232, 186, 200),
        11 => Color32::from_rgb(128, 187, 219),
        12 => Color32::from_rgb(203, 132, 66),
        18 => Color32::from_rgb(204, 142, 105),
        21 => Color32::from_rgb(196, 40, 28),
        22 => Color32::from_rgb(196, 112, 160),
        23 => Color32::from_rgb(13, 105, 172),
        24 => Color32::from_rgb(245, 205, 48),
        25 => Color32::from_rgb(98, 71, 50),
        26 => Color32::from_rgb(27, 42, 53),
        27 => Color32::from_rgb(109, 110, 108),
        28 => Color32::from_rgb(40, 127, 71),
        29 => Color32::from_rgb(161, 196, 140),
        36 => Color32::from_rgb(243, 207, 155),
        37 => Color32::from_rgb(75, 151, 75),
        38 => Color32::from_rgb(160, 95, 53),
        39 => Color32::from_rgb(193, 202, 222),
        40 => Color32::from_rgb(236, 236, 236),
        41 => Color32::from_rgb(205, 84, 75),
        42 => Color32::from_rgb(193, 223, 240),
        43 => Color32::from_rgb(123, 182, 232),
        44 => Color32::from_rgb(247, 241, 141),
        45 => Color32::from_rgb(180, 210, 228),
        47 => Color32::from_rgb(217, 133, 108),
        48 => Color32::from_rgb(132, 182, 141),
        49 => Color32::from_rgb(248, 241, 132),
        50 => Color32::from_rgb(236, 232, 222),
        100 => Color32::from_rgb(238, 196, 182),
        101 => Color32::from_rgb(218, 134, 122),
        102 => Color32::from_rgb(110, 153, 202),
        103 => Color32::from_rgb(199, 193, 183),
        104 => Color32::from_rgb(107, 50, 124),
        105 => Color32::from_rgb(226, 155, 64),
        106 => Color32::from_rgb(218, 133, 65),
        107 => Color32::from_rgb(0, 143, 156),
        108 => Color32::from_rgb(104, 92, 67),
        110 => Color32::from_rgb(67, 84, 147),
        111 => Color32::from_rgb(191, 183, 177),
        112 => Color32::from_rgb(104, 116, 172),
        113 => Color32::from_rgb(229, 173, 200),
        115 => Color32::from_rgb(199, 210, 60),
        116 => Color32::from_rgb(85, 165, 175),
        118 => Color32::from_rgb(183, 215, 213),
        119 => Color32::from_rgb(164, 189, 71),
        120 => Color32::from_rgb(217, 228, 167),
        121 => Color32::from_rgb(231, 172, 88),
        123 => Color32::from_rgb(211, 111, 76),
        124 => Color32::from_rgb(146, 57, 120),
        125 => Color32::from_rgb(234, 184, 146),
        126 => Color32::from_rgb(165, 165, 203),
        127 => Color32::from_rgb(220, 188, 129),
        128 => Color32::from_rgb(174, 122, 89),
        131 => Color32::from_rgb(156, 163, 168),
        133 => Color32::from_rgb(213, 115, 61),
        134 => Color32::from_rgb(216, 221, 86),
        135 => Color32::from_rgb(116, 134, 157),
        136 => Color32::from_rgb(135, 124, 144),
        137 => Color32::from_rgb(224, 152, 100),
        138 => Color32::from_rgb(149, 138, 115),
        140 => Color32::from_rgb(32, 58, 86),
        141 => Color32::from_rgb(39, 70, 45),
        143 => Color32::from_rgb(207, 226, 247),
        145 => Color32::from_rgb(121, 136, 161),
        146 => Color32::from_rgb(149, 142, 163),
        147 => Color32::from_rgb(147, 135, 103),
        148 => Color32::from_rgb(87, 88, 87),
        149 => Color32::from_rgb(22, 29, 50),
        150 => Color32::from_rgb(171, 173, 172),
        151 => Color32::from_rgb(120, 144, 130),
        153 => Color32::from_rgb(149, 121, 119),
        154 => Color32::from_rgb(123, 46, 47),
        157 => Color32::from_rgb(255, 246, 123),
        158 => Color32::from_rgb(225, 164, 194),
        168 => Color32::from_rgb(117, 108, 98),
        176 => Color32::from_rgb(151, 105, 91),
        178 => Color32::from_rgb(180, 132, 85),
        179 => Color32::from_rgb(137, 135, 136),
        180 => Color32::from_rgb(215, 169, 75),
        190 => Color32::from_rgb(249, 214, 46),
        191 => Color32::from_rgb(232, 171, 45),
        192 => Color32::from_rgb(105, 64, 40),
        193 => Color32::from_rgb(207, 96, 36),
        194 => Color32::from_rgb(163, 162, 165),
        195 => Color32::from_rgb(70, 103, 164),
        196 => Color32::from_rgb(35, 71, 139),
        198 => Color32::from_rgb(142, 66, 133),
        199 => Color32::from_rgb(99, 95, 98),
        200 => Color32::from_rgb(130, 138, 93),
        208 => Color32::from_rgb(229, 228, 223),
        209 => Color32::from_rgb(176, 142, 68),
        210 => Color32::from_rgb(112, 149, 120),
        211 => Color32::from_rgb(121, 181, 181),
        212 => Color32::from_rgb(159, 195, 233),
        213 => Color32::from_rgb(108, 129, 183),
        216 => Color32::from_rgb(144, 76, 42),
        217 => Color32::from_rgb(124, 92, 70),
        218 => Color32::from_rgb(150, 112, 159),
        219 => Color32::from_rgb(107, 98, 155),
        220 => Color32::from_rgb(167, 169, 206),
        221 => Color32::from_rgb(205, 98, 152),
        222 => Color32::from_rgb(228, 173, 200),
        223 => Color32::from_rgb(220, 144, 149),
        224 => Color32::from_rgb(240, 213, 160),
        225 => Color32::from_rgb(235, 184, 127),
        226 => Color32::from_rgb(253, 234, 141),
        232 => Color32::from_rgb(125, 187, 221),
        268 => Color32::from_rgb(52, 43, 117),
        301 => Color32::from_rgb(80, 109, 84),
        302 => Color32::from_rgb(91, 93, 105),
        303 => Color32::from_rgb(0, 16, 176),
        304 => Color32::from_rgb(44, 101, 29),
        305 => Color32::from_rgb(82, 124, 174),
        306 => Color32::from_rgb(51, 88, 130),
        307 => Color32::from_rgb(16, 42, 220),
        308 => Color32::from_rgb(61, 21, 133),
        309 => Color32::from_rgb(52, 142, 64),
        310 => Color32::from_rgb(91, 154, 76),
        311 => Color32::from_rgb(159, 161, 172),
        312 => Color32::from_rgb(89, 34, 89),
        313 => Color32::from_rgb(31, 128, 29),
        314 => Color32::from_rgb(159, 173, 192),
        315 => Color32::from_rgb(9, 137, 207),
        316 => Color32::from_rgb(123, 0, 123),
        317 => Color32::from_rgb(124, 156, 107),
        318 => Color32::from_rgb(138, 171, 133),
        319 => Color32::from_rgb(185, 196, 177),
        320 => Color32::from_rgb(202, 203, 209),
        321 => Color32::from_rgb(167, 94, 155),
        322 => Color32::from_rgb(123, 47, 123),
        323 => Color32::from_rgb(148, 190, 129),
        324 => Color32::from_rgb(168, 189, 153),
        325 => Color32::from_rgb(223, 223, 222),
        327 => Color32::from_rgb(151, 0, 0),
        328 => Color32::from_rgb(177, 229, 166),
        329 => Color32::from_rgb(152, 194, 219),
        330 => Color32::from_rgb(255, 152, 220),
        331 => Color32::from_rgb(255, 89, 89),
        332 => Color32::from_rgb(117, 0, 0),
        333 => Color32::from_rgb(239, 184, 56),
        334 => Color32::from_rgb(248, 217, 109),
        335 => Color32::from_rgb(231, 231, 236),
        336 => Color32::from_rgb(199, 212, 228),
        337 => Color32::from_rgb(255, 148, 148),
        338 => Color32::from_rgb(190, 104, 98),
        339 => Color32::from_rgb(86, 36, 36),
        340 => Color32::from_rgb(241, 231, 199),
        341 => Color32::from_rgb(254, 243, 187),
        342 => Color32::from_rgb(224, 178, 208),
        343 => Color32::from_rgb(212, 144, 189),
        344 => Color32::from_rgb(150, 85, 85),
        345 => Color32::from_rgb(143, 76, 42),
        346 => Color32::from_rgb(211, 190, 150),
        347 => Color32::from_rgb(226, 220, 188),
        348 => Color32::from_rgb(237, 234, 234),
        349 => Color32::from_rgb(233, 218, 218),
        350 => Color32::from_rgb(136, 62, 62),
        351 => Color32::from_rgb(188, 155, 93),
        352 => Color32::from_rgb(199, 172, 120),
        353 => Color32::from_rgb(202, 191, 163),
        354 => Color32::from_rgb(187, 179, 178),
        355 => Color32::from_rgb(108, 88, 75),
        356 => Color32::from_rgb(160, 132, 79),
        357 => Color32::from_rgb(149, 137, 136),
        358 => Color32::from_rgb(171, 168, 158),
        359 => Color32::from_rgb(175, 148, 131),
        360 => Color32::from_rgb(150, 103, 102),
        361 => Color32::from_rgb(86, 66, 54),
        362 => Color32::from_rgb(126, 104, 63),
        363 => Color32::from_rgb(105, 102, 92),
        364 => Color32::from_rgb(90, 76, 66),
        365 => Color32::from_rgb(106, 57, 9),
        1001 => Color32::from_rgb(248, 248, 248),
        1002 => Color32::from_rgb(205, 205, 205),
        1003 => Color32::from_rgb(17, 17, 17),
        1004 => Color32::from_rgb(255, 0, 0),
        1005 => Color32::from_rgb(255, 176, 0),
        1006 => Color32::from_rgb(180, 128, 255),
        1007 => Color32::from_rgb(163, 75, 75),
        1008 => Color32::from_rgb(193, 190, 66),
        1009 => Color32::from_rgb(255, 255, 0),
        1010 => Color32::from_rgb(0, 0, 255),
        1011 => Color32::from_rgb(0, 32, 96),
        1012 => Color32::from_rgb(33, 84, 185),
        1013 => Color32::from_rgb(4, 175, 236),
        1014 => Color32::from_rgb(170, 85, 0),
        1015 => Color32::from_rgb(170, 0, 170),
        1016 => Color32::from_rgb(255, 102, 204),
        1017 => Color32::from_rgb(255, 175, 0),
        1018 => Color32::from_rgb(18, 238, 212),
        1019 => Color32::from_rgb(0, 255, 255),
        1020 => Color32::from_rgb(0, 255, 0),
        1021 => Color32::from_rgb(58, 125, 21),
        1022 => Color32::from_rgb(127, 142, 100),
        1023 => Color32::from_rgb(140, 91, 159),
        1024 => Color32::from_rgb(175, 221, 255),
        1025 => Color32::from_rgb(255, 201, 201),
        1026 => Color32::from_rgb(177, 167, 255),
        1027 => Color32::from_rgb(159, 243, 233),
        1028 => Color32::from_rgb(204, 255, 204),
        1029 => Color32::from_rgb(255, 255, 204),
        1030 => Color32::from_rgb(255, 204, 153),
        1031 => Color32::from_rgb(98, 37, 209),
        1032 => Color32::from_rgb(255, 0, 191),
        _ => Color32::from_rgb(163, 162, 165),
    }
}

// ----------------------------------------------------------------------------
// OpenRBLX RenderObject3D & Geometry Structures
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Vertex3D {
    pub pos: Vec3,
    pub uv: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct RawTriangle3D {
    pub v0: Vertex3D,
    pub v1: Vertex3D,
    pub v2: Vertex3D,
    pub normal: Vec3,
    pub color: Color32,
    pub texture_key: Option<String>,
    pub is_neon: bool,
    pub is_ground_face: bool,
    pub is_transparent: bool,
    pub is_selected: bool,
}

pub struct CameraTriangle {
    pub p0: Pos2,
    pub p1: Pos2,
    pub p2: Pos2,
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],
    pub uv2: [f32; 2],
    pub color: Color32,
    pub depth: f32,
    pub texture_key: Option<String>,
    pub is_selected: bool,
}

#[derive(Debug, Clone)]
pub struct RenderInstanceInfo {
    pub referent: Ref,
    pub name: String,
    pub class_name: String,
    pub cframe: CFrame3D,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
}

// ----------------------------------------------------------------------------
// Viewport3D Engine State
// ----------------------------------------------------------------------------

pub struct Viewport3D {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
    pub show_grid: bool,
    pub show_wireframe: bool,
    pub show_skybox: bool,
    pub move_speed: f32,
    pub textures: HashMap<String, egui::TextureHandle>,
    pub initialized_camera: bool,
}

impl Default for Viewport3D {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.18,
            distance: 65.0,
            target: Vec3::new(0.0, 6.0, 0.0),
            show_grid: false,
            show_wireframe: false,
            show_skybox: true,
            move_speed: 4.0,
            textures: HashMap::new(),
            initialized_camera: false,
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
                self.pitch = 1.54;
            }
            CameraPreset::Front => {
                self.yaw = 0.0;
                self.pitch = 0.15;
            }
            CameraPreset::Side => {
                self.yaw = PI * 0.5;
                self.pitch = 0.15;
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

    pub fn init_camera_from_dom(&mut self, dom: &WeakDom) {
        let mut stack = dom.root().children().to_vec();
        while let Some(r) = stack.pop() {
            if let Some(inst) = dom.get_by_ref(r) {
                if inst.class == "Camera" || inst.name == "Camera" {
                    if let Some(Variant::CFrame(cf)) = inst.properties.get(&rbx_dom_weak::ustr("CFrame")).or_else(|| inst.properties.get(&rbx_dom_weak::ustr("CoordinateFrame"))) {
                        let cam_pos = Vec3::new(cf.position.x, cf.position.y, cf.position.z);
                        let look_dir = Vec3::new(-cf.orientation.x.z, -cf.orientation.y.z, -cf.orientation.z.z).normalize();
                        let target_pos = cam_pos.add(&look_dir.mul_scalar(35.0));
                        self.target = target_pos;
                        self.distance = (cam_pos.sub(&target_pos)).length().clamp(10.0, 120.0);
                        self.yaw = look_dir.x.atan2(-look_dir.z);
                        self.pitch = (look_dir.y).asin().clamp(-1.4, 1.4);
                        self.initialized_camera = true;
                        return;
                    }
                }
                stack.extend(inst.children());
            }
        }
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        dom: Option<&WeakDom>,
        selected: &mut Option<Ref>,
        cookie_opt: Option<&str>,
    ) {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::new(220.0, 300.0)),
            egui::Sense::click_and_drag(),
        );

        let painter = ui.painter_at(rect);

        // Auto-initialize camera from Workspace.Camera on first load
        if !self.initialized_camera {
            if let Some(dom) = dom {
                self.init_camera_from_dom(dom);
                self.initialized_camera = true;
            }
        }

        // Touch Navigation: Drag to Orbit
        if response.dragged() {
            let delta = response.drag_delta();
            self.yaw -= delta.x * 0.008;
            self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.54, 1.54);
        }

        // Camera Frame Vectors
        let cos_p = self.pitch.cos();
        let sin_p = self.pitch.sin();
        let cos_y = self.yaw.cos();
        let sin_y = self.yaw.sin();

        let eye = Vec3::new(
            self.target.x + self.distance * cos_p * sin_y,
            self.target.y + self.distance * sin_p,
            self.target.z + self.distance * cos_p * cos_y,
        );

        let forward = self.target.sub(&eye).normalize();
        let right = forward.cross(&Vec3::UP).normalize();
        let up = right.cross(&forward).normalize();

        let screen_w = rect.width();
        let screen_h = rect.height();
        let fov_rad = 60.0_f32.to_radians();
        let tan_half = (fov_rad * 0.5).tan();
        let focal = (0.5 * screen_h) / tan_half;
        let screen_cx = rect.center().x;
        let screen_cy = rect.center().y;

        // 1. Render Clear Blue Roblox Daytime Skybox Backdrop
        let sky_top = Color32::from_rgb(82, 149, 232);      // Clear blue sky
        let sky_mid = Color32::from_rgb(126, 184, 247);     // Daylight blue
        let sky_horizon = Color32::from_rgb(184, 220, 250); // Atmosphere haze
        let terrain_ground = Color32::from_rgb(68, 85, 60); // Distant ground base

        let horizon_offset = self.pitch.tan() * focal;
        let horizon_y = (screen_cy + horizon_offset).clamp(rect.top() - 500.0, rect.bottom() + 500.0);

        if self.show_skybox {
            let h_mid = (horizon_y - 90.0).max(rect.top());
            let h_top = (horizon_y - 220.0).max(rect.top());

            painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(rect.right(), h_top.min(rect.bottom()))), 0.0, sky_top);
            if h_top < h_mid {
                painter.rect_filled(Rect::from_min_max(Pos2::new(rect.left(), h_top), Pos2::new(rect.right(), h_mid.min(rect.bottom()))), 0.0, sky_mid);
            }
            if h_mid < horizon_y {
                painter.rect_filled(Rect::from_min_max(Pos2::new(rect.left(), h_mid), Pos2::new(rect.right(), horizon_y.min(rect.bottom()))), 0.0, sky_horizon);
            }
            if horizon_y < rect.bottom() {
                painter.rect_filled(Rect::from_min_max(Pos2::new(rect.left(), horizon_y.max(rect.top())), rect.max), 0.0, terrain_ground);
            }
        } else {
            painter.rect_filled(rect, 0.0, Color32::from_rgb(30, 30, 32));
        }

        // 2. Collect 3D Instance Triangles from Workspace
        let mut raw_triangles: Vec<RawTriangle3D> = Vec::new();
        let mut instance_infos: Vec<RenderInstanceInfo> = Vec::new();

        if let Some(dom) = dom {
            let mut stack = dom.root().children().to_vec();
            while let Some(r) = stack.pop() {
                if let Some(inst) = dom.get_by_ref(r) {
                    if matches!(
                        inst.class.as_str(),
                        "ReplicatedStorage" | "ServerStorage" | "Lighting" | "StarterGui" | "StarterPack" | "ServerScriptService"
                    ) {
                        continue;
                    }

                    stack.extend(inst.children());

                    let is_3d = matches!(
                        inst.class.as_str(),
                        "Part"
                            | "WedgePart"
                            | "CornerWedgePart"
                            | "TrussPart"
                            | "SpawnLocation"
                            | "MeshPart"
                            | "UnionOperation"
                            | "Seat"
                            | "VehicleSeat"
                            | "FlagStand"
                            | "Terrain"
                    );

                    if is_3d {
                        let is_sel = *selected == Some(r);
                        let (part_tris, inst_info) = generate_instance_triangles(dom, r, inst, is_sel, cookie_opt);
                        raw_triangles.extend(part_tris);
                        instance_infos.push(inst_info);
                    }
                }
            }
        }

        // 3. Tap Raycasting Selection
        if response.clicked() {
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                let mx = ((mouse_pos.x - rect.left()) / screen_w) * 2.0 - 1.0;
                let my = -(((mouse_pos.y - rect.top()) / screen_h) * 2.0 - 1.0);
                let aspect = screen_w / screen_h.max(1.0);

                let ray_dir = forward
                    .add(&right.mul_scalar(mx * tan_half * aspect))
                    .add(&up.mul_scalar(my * tan_half))
                    .normalize();

                let ray = Ray { origin: eye, direction: ray_dir };

                let mut nearest_hit = None;
                let mut min_t = f32::INFINITY;

                for info in &instance_infos {
                    if let Some(t) = ray.intersects_aabb(&info.aabb_min, &info.aabb_max) {
                        if t < min_t {
                            min_t = t;
                            nearest_hit = Some(info.referent);
                        }
                    }
                }

                if let Some(hit_ref) = nearest_hit {
                    *selected = Some(hit_ref);
                }
            }
        }

        // 4. Transform to Camera Space, Clip at Near Plane (z >= 0.2), and Project
        let sun_dir = Vec3::new(0.35, 0.85, 0.40).normalize();
        let z_near = 0.2_f32;
        let mut camera_triangles: Vec<CameraTriangle> = Vec::with_capacity(raw_triangles.len());

        let project_cam = |p_cam: &Vec3| -> Pos2 {
            let inv_z = 1.0 / p_cam.z.max(0.05);
            let sx = screen_cx + p_cam.x * inv_z * focal;
            let sy = screen_cy - p_cam.y * inv_z * focal;
            Pos2::new(sx, sy)
        };

        for tri in &raw_triangles {
            // Outward normal backface culling in world space
            let center = tri.v0.pos.add(&tri.v1.pos).add(&tri.v2.pos).mul_scalar(1.0 / 3.0);
            let to_cam = eye.sub(&center);
            if tri.normal.dot(&to_cam) <= 0.0 && !tri.is_transparent {
                continue;
            }

            // Transform vertices to camera space
            let d0 = tri.v0.pos.sub(&eye);
            let d1 = tri.v1.pos.sub(&eye);
            let d2 = tri.v2.pos.sub(&eye);

            let c0 = Vec3::new(d0.dot(&right), d0.dot(&up), d0.dot(&forward));
            let c1 = Vec3::new(d1.dot(&right), d1.dot(&up), d1.dot(&forward));
            let c2 = Vec3::new(d2.dot(&right), d2.dot(&up), d2.dot(&forward));

            // Diffuse Sunlight Shading
            let diffuse = tri.normal.dot(&sun_dir).max(0.0);
            let shade = if tri.is_neon { 1.0_f32 } else { 0.48 + 0.52 * diffuse };

            let r = (tri.color.r() as f32 * shade).clamp(0.0, 255.0) as u8;
            let g = (tri.color.g() as f32 * shade).clamp(0.0, 255.0) as u8;
            let b = (tri.color.b() as f32 * shade).clamp(0.0, 255.0) as u8;
            let final_color = Color32::from_rgba_unmultiplied(r, g, b, tri.color.a());

            let depth = if tri.is_ground_face { 50000.0 } else { (c0.z + c1.z + c2.z) * (1.0 / 3.0) };

            let in0 = c0.z >= z_near;
            let in1 = c1.z >= z_near;
            let in2 = c2.z >= z_near;
            let in_count = (if in0 { 1 } else { 0 }) + (if in1 { 1 } else { 0 }) + (if in2 { 1 } else { 0 });

            if in_count == 3 {
                camera_triangles.push(CameraTriangle {
                    p0: project_cam(&c0),
                    p1: project_cam(&c1),
                    p2: project_cam(&c2),
                    uv0: tri.v0.uv,
                    uv1: tri.v1.uv,
                    uv2: tri.v2.uv,
                    color: final_color,
                    depth,
                    texture_key: tri.texture_key.clone(),
                    is_selected: tri.is_selected,
                });
            } else if in_count == 1 {
                let (cin, co1, co2, uvin, uvo1, uvo2) = if in0 {
                    (c0, c1, c2, tri.v0.uv, tri.v1.uv, tri.v2.uv)
                } else if in1 {
                    (c1, c2, c0, tri.v1.uv, tri.v2.uv, tri.v0.uv)
                } else {
                    (c2, c0, c1, tri.v2.uv, tri.v0.uv, tri.v1.uv)
                };

                let t1 = ((cin.z - z_near) / (cin.z - co1.z).max(1e-5)).clamp(0.0, 1.0);
                let t2 = ((cin.z - z_near) / (cin.z - co2.z).max(1e-5)).clamp(0.0, 1.0);

                let ca = Vec3::new(cin.x + (co1.x - cin.x) * t1, cin.y + (co1.y - cin.y) * t1, z_near);
                let cb = Vec3::new(cin.x + (co2.x - cin.x) * t2, cin.y + (co2.y - cin.y) * t2, z_near);

                let uva = [uvin[0] + (uvo1[0] - uvin[0]) * t1, uvin[1] + (uvo1[1] - uvin[1]) * t1];
                let uvb = [uvin[0] + (uvo2[0] - uvin[0]) * t2, uvin[1] + (uvo2[1] - uvin[1]) * t2];

                camera_triangles.push(CameraTriangle {
                    p0: project_cam(&cin),
                    p1: project_cam(&ca),
                    p2: project_cam(&cb),
                    uv0: uvin,
                    uv1: uva,
                    uv2: uvb,
                    color: final_color,
                    depth,
                    texture_key: tri.texture_key.clone(),
                    is_selected: tri.is_selected,
                });
            } else if in_count == 2 {
                let (cout, ci1, ci2, uvout, uvi1, uvi2) = if !in0 {
                    (c0, c1, c2, tri.v0.uv, tri.v1.uv, tri.v2.uv)
                } else if !in1 {
                    (c1, c2, c0, tri.v1.uv, tri.v2.uv, tri.v0.uv)
                } else {
                    (c2, c0, c1, tri.v2.uv, tri.v0.uv, tri.v1.uv)
                };

                let t1 = ((ci1.z - z_near) / (ci1.z - cout.z).max(1e-5)).clamp(0.0, 1.0);
                let t2 = ((ci2.z - z_near) / (ci2.z - cout.z).max(1e-5)).clamp(0.0, 1.0);

                let ca = Vec3::new(ci1.x + (cout.x - ci1.x) * t1, ci1.y + (cout.y - ci1.y) * t1, z_near);
                let cb = Vec3::new(ci2.x + (cout.x - ci2.x) * t2, ci2.y + (cout.y - ci2.y) * t2, z_near);

                let uva = [uvi1[0] + (uvout[0] - uvi1[0]) * t1, uvi1[1] + (uvout[1] - uvi1[1]) * t1];
                let uvb = [uvi2[0] + (uvout[0] - uvi2[0]) * t2, uvi2[1] + (uvout[1] - uvi2[1]) * t2];

                let p_i1 = project_cam(&ci1);
                let p_i2 = project_cam(&ci2);
                let p_a = project_cam(&ca);
                let p_b = project_cam(&cb);

                camera_triangles.push(CameraTriangle {
                    p0: p_i1,
                    p1: p_i2,
                    p2: p_a,
                    uv0: uvi1,
                    uv1: uvi2,
                    uv2: uva,
                    color: final_color,
                    depth,
                    texture_key: tri.texture_key.clone(),
                    is_selected: tri.is_selected,
                });
                camera_triangles.push(CameraTriangle {
                    p0: p_i2,
                    p1: p_b,
                    p2: p_a,
                    uv0: uvi2,
                    uv1: uvb,
                    uv2: uva,
                    color: final_color,
                    depth,
                    texture_key: tri.texture_key.clone(),
                    is_selected: tri.is_selected,
                });
            }
        }

        // 5. Global Painter's Depth Sorting: Draw Furthest -> Nearest
        camera_triangles.sort_by(|a, b| b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal));

        // 6. Direct Depth-Sorted Painter Rendering
        for tri in &camera_triangles {
            let mut tex_handle_opt = None;

            if let Some(ref key) = tri.texture_key {
                if !self.textures.contains_key(key) {
                    if let Some(img) = asset_downloader::get_cached_image(key) {
                        let color_img = egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &img.rgba);
                        let handle = ui.ctx().load_texture(key, color_img, egui::TextureOptions::LINEAR);
                        self.textures.insert(key.clone(), handle);
                    }
                }
                tex_handle_opt = self.textures.get(key);
            }

            if let Some(handle) = tex_handle_opt {
                let mut mesh = egui::Mesh::default();
                mesh.texture_id = handle.id();

                mesh.vertices.push(egui::epaint::Vertex {
                    pos: tri.p0,
                    uv: Pos2::new(tri.uv0[0], tri.uv0[1]),
                    color: tri.color,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: tri.p1,
                    uv: Pos2::new(tri.uv1[0], tri.uv1[1]),
                    color: tri.color,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: tri.p2,
                    uv: Pos2::new(tri.uv2[0], tri.uv2[1]),
                    color: tri.color,
                });
                mesh.add_triangle(0, 1, 2);

                painter.add(egui::Shape::mesh(mesh));
            } else {
                painter.add(egui::Shape::convex_polygon(
                    vec![tri.p0, tri.p1, tri.p2],
                    tri.color,
                    Stroke::NONE,
                ));
            }

            if self.show_wireframe || tri.is_selected {
                let stroke_color = if tri.is_selected {
                    Color32::from_rgb(0, 230, 255)
                } else {
                    Color32::from_rgba_unmultiplied(0, 0, 0, 18)
                };
                let stroke_width = if tri.is_selected { 2.0_f32 } else { 0.75_f32 };

                painter.line_segment([tri.p0, tri.p1], Stroke::new(stroke_width, stroke_color));
                painter.line_segment([tri.p1, tri.p2], Stroke::new(stroke_width, stroke_color));
                painter.line_segment([tri.p2, tri.p0], Stroke::new(stroke_width, stroke_color));
            }
        }

        // 7. 3D Selection Gizmo & Name Label
        if let Some(sel_ref) = *selected {
            if let Some(info) = instance_infos.iter().find(|i| i.referent == sel_ref) {
                let d_sel = info.cframe.pos.sub(&eye);
                let z_sel = d_sel.dot(&forward);

                if z_sel > z_near {
                    let c_sel = Vec3::new(d_sel.dot(&right), d_sel.dot(&up), z_sel);
                    let center_screen = project_cam(&c_sel);

                    painter.circle_filled(center_screen, 5.0, Color32::from_rgb(0, 230, 255));

                    let gizmo_len = 6.0;
                    let p_x = info.cframe.transform_point(Vec3::new(gizmo_len, 0.0, 0.0));
                    let p_y = info.cframe.transform_point(Vec3::new(0.0, gizmo_len, 0.0));
                    let p_z = info.cframe.transform_point(Vec3::new(0.0, 0.0, gizmo_len));

                    let d_x = p_x.sub(&eye);
                    let d_y = p_y.sub(&eye);
                    let d_z = p_z.sub(&eye);

                    if d_x.dot(&forward) > z_near {
                        let sx = project_cam(&Vec3::new(d_x.dot(&right), d_x.dot(&up), d_x.dot(&forward)));
                        painter.line_segment([center_screen, sx], Stroke::new(3.5_f32, Color32::from_rgb(255, 60, 60)));
                        painter.circle_filled(sx, 4.0, Color32::from_rgb(255, 60, 60));
                    }
                    if d_y.dot(&forward) > z_near {
                        let sy = project_cam(&Vec3::new(d_y.dot(&right), d_y.dot(&up), d_y.dot(&forward)));
                        painter.line_segment([center_screen, sy], Stroke::new(3.5_f32, Color32::from_rgb(60, 255, 60)));
                        painter.circle_filled(sy, 4.0, Color32::from_rgb(60, 255, 60));
                    }
                    if d_z.dot(&forward) > z_near {
                        let sz = project_cam(&Vec3::new(d_z.dot(&right), d_z.dot(&up), d_z.dot(&forward)));
                        painter.line_segment([center_screen, sz], Stroke::new(3.5_f32, Color32::from_rgb(60, 130, 255)));
                        painter.circle_filled(sz, 4.0, Color32::from_rgb(60, 130, 255));
                    }

                    painter.text(
                        Pos2::new(center_screen.x, center_screen.y - 18.0),
                        egui::Align2::CENTER_CENTER,
                        format!("{} {}", explorer_icon(&info.class_name), info.name),
                        egui::FontId::proportional(12.5),
                        Color32::from_rgb(100, 240, 255),
                    );
                }
            }
        }

        // Viewport Header Overlay
        let info_pos = Pos2::new(rect.left() + 10.0, rect.top() + 10.0);
        painter.text(
            info_pos,
            egui::Align2::LEFT_TOP,
            format!("🎥 Studio Viewport v3.0 (OpenRBLX) | Instances: {} | Orbit: {:.0}°, {:.0}°", instance_infos.len(), self.yaw.to_degrees(), self.pitch.to_degrees()),
            egui::FontId::proportional(12.0),
            Color32::from_rgb(60, 70, 85),
        );
    }
}

// ----------------------------------------------------------------------------
// Geometry & Surface Texture Generators for Roblox Parts & Meshes
// ----------------------------------------------------------------------------

fn extract_instance_cframe(inst: &rbx_dom_weak::Instance) -> CFrame3D {
    if let Some(Variant::CFrame(cf)) = inst.properties.get(&rbx_dom_weak::ustr("CFrame")).or_else(|| inst.properties.get(&rbx_dom_weak::ustr("CoordinateFrame"))) {
        let pos = Vec3::new(cf.position.x, cf.position.y, cf.position.z);
        return CFrame3D {
            pos,
            r00: cf.orientation.x.x,
            r01: cf.orientation.x.y,
            r02: cf.orientation.x.z,
            r10: cf.orientation.y.x,
            r11: cf.orientation.y.y,
            r12: cf.orientation.y.z,
            r20: cf.orientation.z.x,
            r21: cf.orientation.z.y,
            r22: cf.orientation.z.z,
        };
    }

    let pos = match inst.properties.get(&rbx_dom_weak::ustr("Position")) {
        Some(Variant::Vector3(v)) => Vec3::new(v.x, v.y, v.z),
        _ => Vec3::ZERO,
    };

    if let Some(Variant::Vector3(rot)) = inst.properties.get(&rbx_dom_weak::ustr("Orientation")) {
        return CFrame3D::from_angles(pos, rot.x.to_radians(), rot.y.to_radians(), rot.z.to_radians());
    }

    CFrame3D::new(pos)
}

fn extract_part_shape_type(inst: &rbx_dom_weak::Instance, mesh_shape_type: Option<&str>) -> &'static str {
    if let Some(mt) = mesh_shape_type {
        if mt == "Sphere" || mt == "Head" { return "Ball"; }
        if mt == "Cylinder" { return "Cylinder"; }
        if mt == "Wedge" { return "Wedge"; }
    }
    if inst.class == "WedgePart" { return "Wedge"; }
    if inst.class == "CornerWedgePart" { return "CornerWedge"; }
    if inst.class == "TrussPart" { return "Truss"; }
    if inst.name == "Ball" || inst.name.to_lowercase().contains("sphere") || inst.name.to_lowercase().contains("ball") || inst.name.to_lowercase().contains("wheel") {
        return "Ball";
    }

    if let Some(v) = inst.properties.get(&rbx_dom_weak::ustr("Shape")) {
        match v {
            Variant::String(s) => match s.as_str() {
                "Ball" => "Ball",
                "Cylinder" => "Cylinder",
                "Block" => "Block",
                _ => "Block",
            },
            Variant::Int32(0) | Variant::Int64(0) => "Ball",
            Variant::Int32(1) | Variant::Int64(1) => "Block",
            Variant::Int32(2) | Variant::Int64(2) => "Cylinder",
            _ => "Block",
        }
    } else {
        "Block"
    }
}

fn extract_decal_face_name(inst: &rbx_dom_weak::Instance) -> &'static str {
    if let Some(v) = inst.properties.get(&rbx_dom_weak::ustr("Face")) {
        match v {
            Variant::String(s) => match s.as_str() {
                "Top" => "Top",
                "Bottom" => "Bottom",
                "Front" => "Front",
                "Back" => "Back",
                "Left" => "Left",
                "Right" => "Right",
                _ => "Front",
            },
            Variant::Int32(0) | Variant::Int64(0) => "Right",
            Variant::Int32(1) | Variant::Int64(1) => "Top",
            Variant::Int32(2) | Variant::Int64(2) => "Back",
            Variant::Int32(3) | Variant::Int64(3) => "Left",
            Variant::Int32(4) | Variant::Int64(4) => "Bottom",
            Variant::Int32(5) | Variant::Int64(5) => "Front",
            _ => "Front",
        }
    } else {
        "Front"
    }
}

fn is_studs_surface(inst: &rbx_dom_weak::Instance, prop_name: &str) -> bool {
    if let Some(v) = inst.properties.get(&rbx_dom_weak::ustr(prop_name)) {
        match v {
            Variant::String(s) => s.as_str() == "Studs",
            Variant::Int32(3) | Variant::Int64(3) => true,
            _ => false,
        }
    } else {
        false
    }
}

fn is_inlet_surface(inst: &rbx_dom_weak::Instance, prop_name: &str) -> bool {
    if let Some(v) = inst.properties.get(&rbx_dom_weak::ustr(prop_name)) {
        match v {
            Variant::String(s) => s.as_str() == "Inlet" || s.as_str() == "Inlets",
            Variant::Int32(4) | Variant::Int64(4) => true,
            _ => false,
        }
    } else {
        false
    }
}

fn generate_instance_triangles(
    dom: &WeakDom,
    referent: Ref,
    inst: &rbx_dom_weak::Instance,
    is_selected: bool,
    cookie_opt: Option<&str>,
) -> (Vec<RawTriangle3D>, RenderInstanceInfo) {
    let cframe = extract_instance_cframe(inst);
    let size = match inst.properties.get(&rbx_dom_weak::ustr("Size")) {
        Some(Variant::Vector3(v)) => Vec3::new(v.x.max(0.1), v.y.max(0.1), v.z.max(0.1)),
        _ => Vec3::new(4.0, 1.2, 2.0),
    };

    // Color extraction: Color3, Color3uint8, or BrickColor
    let color = match inst.properties.get(&rbx_dom_weak::ustr("Color")) {
        Some(Variant::Color3(c)) => Color32::from_rgb((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8),
        Some(Variant::Color3uint8(c)) => Color32::from_rgb(c.r, c.g, c.b),
        _ => match inst.properties.get(&rbx_dom_weak::ustr("BrickColor")) {
            Some(Variant::BrickColor(bc)) => brick_color_to_rgb(*bc as u32),
            Some(Variant::Int32(bc)) => brick_color_to_rgb(*bc as u32),
            Some(Variant::Int64(bc)) => brick_color_to_rgb(*bc as u32),
            _ => Color32::from_rgb(163, 162, 165),
        },
    };

    let transparency = match inst.properties.get(&rbx_dom_weak::ustr("Transparency")) {
        Some(Variant::Float32(f)) => *f,
        Some(Variant::Float64(f)) => *f as f32,
        _ => 0.0,
    };
    let is_transparent = transparency > 0.05;
    let alpha = ((1.0 - transparency).clamp(0.0, 1.0) * 255.0) as u8;
    let part_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha);

    let material_str = match inst.properties.get(&rbx_dom_weak::ustr("Material")) {
        Some(Variant::String(s)) => s.as_str(),
        _ => "Plastic",
    };
    let is_neon = material_str == "Neon";
    let is_spawn = inst.class == "SpawnLocation";
    let is_baseplate = size.x >= 40.0 || size.z >= 40.0 || inst.name.to_lowercase().contains("base");

    // Check SpecialMesh / BlockMesh children
    let mut mesh_id_opt: Option<String> = None;
    let mut mesh_tex_opt: Option<String> = None;
    let mut mesh_shape_type: Option<String> = None;
    let mut scale = Vec3::new(1.0, 1.0, 1.0);
    let mut offset = Vec3::ZERO;

    if inst.class == "MeshPart" {
        if let Some(Variant::String(mid)) = inst.properties.get(&rbx_dom_weak::ustr("MeshId")) {
            mesh_id_opt = Some(mid.clone());
        }
        if let Some(Variant::String(tid)) = inst.properties.get(&rbx_dom_weak::ustr("TextureID")).or_else(|| inst.properties.get(&rbx_dom_weak::ustr("TextureId"))) {
            mesh_tex_opt = Some(tid.clone());
        }
    }

    for child_ref in inst.children() {
        if let Some(child_inst) = dom.get_by_ref(*child_ref) {
            if child_inst.class == "SpecialMesh" || child_inst.class == "BlockMesh" {
                if let Some(Variant::String(mid)) = child_inst.properties.get(&rbx_dom_weak::ustr("MeshId")) {
                    mesh_id_opt = Some(mid.clone());
                }
                if let Some(Variant::String(tid)) = child_inst.properties.get(&rbx_dom_weak::ustr("TextureId")) {
                    mesh_tex_opt = Some(tid.clone());
                }
                if let Some(Variant::String(mtype)) = child_inst.properties.get(&rbx_dom_weak::ustr("MeshType")) {
                    mesh_shape_type = Some(mtype.clone());
                }
                if let Some(Variant::Vector3(sc)) = child_inst.properties.get(&rbx_dom_weak::ustr("Scale")) {
                    scale = Vec3::new(sc.x, sc.y, sc.z);
                }
                if let Some(Variant::Vector3(off)) = child_inst.properties.get(&rbx_dom_weak::ustr("Offset")) {
                    offset = Vec3::new(off.x, off.y, off.z);
                }
            }
        }
    }

    // Check Decal / Texture children on Part faces
    let mut decal_faces: HashMap<&'static str, (String, f32)> = HashMap::new();
    for child_ref in inst.children() {
        if let Some(child_inst) = dom.get_by_ref(*child_ref) {
            if child_inst.class == "Decal" || child_inst.class == "Texture" {
                let face_name = extract_decal_face_name(child_inst);

                let tex = match child_inst.properties.get(&rbx_dom_weak::ustr("Texture")) {
                    Some(Variant::String(s)) => s.clone(),
                    _ => String::new(),
                };

                let dec_trans = match child_inst.properties.get(&rbx_dom_weak::ustr("Transparency")) {
                    Some(Variant::Float32(f)) => *f,
                    Some(Variant::Float64(f)) => *f as f32,
                    _ => 0.0,
                };

                if !tex.is_empty() {
                    decal_faces.insert(face_name, (tex, dec_trans));
                }
            }
        }
    }

    // Auto-fetch mesh in background if not loaded
    if let Some(ref mid) = mesh_id_opt {
        if asset_downloader::get_cached_mesh(mid).is_none() {
            roblox_api::fetch_and_cache_mesh_async(mid.clone(), cookie_opt.map(|s| s.to_string()));
        }
    }

    let half = Vec3::new(size.x * 0.5 * scale.x, size.y * 0.5 * scale.y, size.z * 0.5 * scale.z);
    let mut part_cframe = cframe;
    part_cframe.pos = part_cframe.pos.add(&cframe.transform_normal(offset));

    let mut triangles = Vec::new();

    let add_quad = |tris: &mut Vec<RawTriangle3D>, p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, normal: Vec3, tex: Option<String>, is_ground: bool| {
        tris.push(RawTriangle3D {
            v0: Vertex3D { pos: p0, uv: [0.0, 0.0] },
            v1: Vertex3D { pos: p1, uv: [1.0, 0.0] },
            v2: Vertex3D { pos: p2, uv: [1.0, 1.0] },
            normal,
            color: part_color,
            texture_key: tex.clone(),
            is_neon,
            is_ground_face: is_ground,
            is_transparent,
            is_selected,
        });
        tris.push(RawTriangle3D {
            v0: Vertex3D { pos: p0, uv: [0.0, 0.0] },
            v1: Vertex3D { pos: p2, uv: [1.0, 1.0] },
            v2: Vertex3D { pos: p3, uv: [0.0, 1.0] },
            normal,
            color: part_color,
            texture_key: tex,
            is_neon,
            is_ground_face: is_ground,
            is_transparent,
            is_selected,
        });
    };

    // 1. Check real Mesh (.mesh file)
    if let Some(ref mid) = mesh_id_opt {
        if let Some(mesh_data) = asset_downloader::get_cached_mesh(mid) {
            let bbox_x = (mesh_data.aabb_max[0] - mesh_data.aabb_min[0]).max(0.01);
            let bbox_y = (mesh_data.aabb_max[1] - mesh_data.aabb_min[1]).max(0.01);
            let bbox_z = (mesh_data.aabb_max[2] - mesh_data.aabb_min[2]).max(0.01);

            let sx = (size.x * scale.x) / bbox_x;
            let sy = (size.y * scale.y) / bbox_y;
            let sz = (size.z * scale.z) / bbox_z;

            let tex_key = mesh_tex_opt.clone().or_else(|| asset_downloader::extract_asset_id(mid));

            for f in &mesh_data.faces {
                if let (Some(&va), Some(&vb), Some(&vc)) = (
                    mesh_data.vertices.get(f[0] as usize),
                    mesh_data.vertices.get(f[1] as usize),
                    mesh_data.vertices.get(f[2] as usize),
                ) {
                    let pa = part_cframe.transform_point(Vec3::new(va[0] * sx, va[1] * sy, va[2] * sz));
                    let pb = part_cframe.transform_point(Vec3::new(vb[0] * sx, vb[1] * sy, vb[2] * sz));
                    let pc = part_cframe.transform_point(Vec3::new(vc[0] * sx, vc[1] * sy, vc[2] * sz));

                    let uva = mesh_data.uvs.get(f[0] as usize).copied().unwrap_or([0.0, 0.0]);
                    let uvb = mesh_data.uvs.get(f[1] as usize).copied().unwrap_or([1.0, 0.0]);
                    let uvc = mesh_data.uvs.get(f[2] as usize).copied().unwrap_or([0.0, 1.0]);

                    let ab = pb.sub(&pa);
                    let ac = pc.sub(&pa);
                    let normal = ab.cross(&ac).normalize();

                    triangles.push(RawTriangle3D {
                        v0: Vertex3D { pos: pa, uv: uva },
                        v1: Vertex3D { pos: pb, uv: uvb },
                        v2: Vertex3D { pos: pc, uv: uvc },
                        normal,
                        color: part_color,
                        texture_key: tex_key.clone(),
                        is_neon,
                        is_ground_face: false,
                        is_transparent,
                        is_selected,
                    });
                }
            }

            let max_r = (half.x * half.x + half.y * half.y + half.z * half.z).sqrt();
            let aabb_min = part_cframe.pos.sub(&Vec3::new(max_r, max_r, max_r));
            let aabb_max = part_cframe.pos.add(&Vec3::new(max_r, max_r, max_r));

            return (
                triangles,
                RenderInstanceInfo {
                    referent,
                    name: inst.name.clone(),
                    class_name: inst.class.to_string(),
                    cframe: part_cframe,
                    aabb_min,
                    aabb_max,
                },
            );
        }
    }

    // 2. Check Primitive Part Shapes
    let shape_type = extract_part_shape_type(inst, mesh_shape_type.as_deref());

    if shape_type == "Ball" {
        // UV Sphere (8 lats x 12 lons = 96 quads = 192 triangles)
        let lats = 8;
        let lons = 12;
        let radius = half.x.min(half.y).min(half.z);

        for i in 0..lats {
            let lat0 = PI * (-0.5 + (i as f32) / (lats as f32));
            let z0 = lat0.sin() * radius;
            let zr0 = lat0.cos() * radius;

            let lat1 = PI * (-0.5 + ((i + 1) as f32) / (lats as f32));
            let z1 = lat1.sin() * radius;
            let zr1 = lat1.cos() * radius;

            let v0 = (i as f32) / (lats as f32);
            let v1 = ((i + 1) as f32) / (lats as f32);

            for j in 0..lons {
                let lng0 = 2.0 * PI * (j as f32) / (lons as f32);
                let x0 = lng0.cos();
                let y0 = lng0.sin();

                let lng1 = 2.0 * PI * ((j + 1) as f32) / (lons as f32);
                let x1 = lng1.cos();
                let y1 = lng1.sin();

                let u0 = (j as f32) / (lons as f32);
                let u1 = ((j + 1) as f32) / (lons as f32);

                let p0 = part_cframe.transform_point(Vec3::new(x0 * zr0, y0 * zr0, z0));
                let p1 = part_cframe.transform_point(Vec3::new(x1 * zr0, y1 * zr0, z0));
                let p2 = part_cframe.transform_point(Vec3::new(x1 * zr1, y1 * zr1, z1));
                let p3 = part_cframe.transform_point(Vec3::new(x0 * zr1, y0 * zr1, z1));

                let norm_local = Vec3::new((x0 + x1) * 0.5 * (zr0 + zr1) * 0.5, (y0 + y1) * 0.5 * (zr0 + zr1) * 0.5, (z0 + z1) * 0.5).normalize();
                let normal = part_cframe.transform_normal(norm_local);

                triangles.push(RawTriangle3D {
                    v0: Vertex3D { pos: p0, uv: [u0, v0] },
                    v1: Vertex3D { pos: p1, uv: [u1, v0] },
                    v2: Vertex3D { pos: p2, uv: [u1, v1] },
                    normal,
                    color: part_color,
                    texture_key: None,
                    is_neon,
                    is_ground_face: false,
                    is_transparent,
                    is_selected,
                });
                triangles.push(RawTriangle3D {
                    v0: Vertex3D { pos: p0, uv: [u0, v0] },
                    v1: Vertex3D { pos: p2, uv: [u1, v1] },
                    v2: Vertex3D { pos: p3, uv: [u0, v1] },
                    normal,
                    color: part_color,
                    texture_key: None,
                    is_neon,
                    is_ground_face: false,
                    is_transparent,
                    is_selected,
                });
            }
        }
    } else if shape_type == "Cylinder" {
        let segments = 12;
        let radius = half.y.min(half.z);
        let mut cap_front_pts = Vec::new();
        let mut cap_back_pts = Vec::new();

        for i in 0..segments {
            let theta = (i as f32 / segments as f32) * PI * 2.0;
            let next_theta = ((i + 1) as f32 / segments as f32) * PI * 2.0;

            let y1 = theta.cos() * radius;
            let z1 = theta.sin() * radius;
            let y2 = next_theta.cos() * radius;
            let z2 = next_theta.sin() * radius;

            let p0 = part_cframe.transform_point(Vec3::new(-half.x, y1, z1));
            let p1 = part_cframe.transform_point(Vec3::new(half.x, y1, z1));
            let p2 = part_cframe.transform_point(Vec3::new(half.x, y2, z2));
            let p3 = part_cframe.transform_point(Vec3::new(-half.x, y2, z2));

            let mid_y = (y1 + y2) * 0.5;
            let mid_z = (z1 + z2) * 0.5;
            let normal = part_cframe.transform_normal(Vec3::new(0.0, mid_y, mid_z).normalize());

            add_quad(&mut triangles, p0, p1, p2, p3, normal, None, false);

            cap_front_pts.push((part_cframe.transform_point(Vec3::new(half.x, y1, z1)), [(theta.cos() * 0.5 + 0.5), (theta.sin() * 0.5 + 0.5)]));
            cap_back_pts.push((part_cframe.transform_point(Vec3::new(-half.x, y1, z1)), [(theta.cos() * 0.5 + 0.5), (theta.sin() * 0.5 + 0.5)]));
        }

        let front_center = part_cframe.transform_point(Vec3::new(half.x, 0.0, 0.0));
        let back_center = part_cframe.transform_point(Vec3::new(-half.x, 0.0, 0.0));
        let norm_f = part_cframe.transform_normal(Vec3::new(1.0, 0.0, 0.0));
        let norm_b = part_cframe.transform_normal(Vec3::new(-1.0, 0.0, 0.0));

        for i in 0..segments {
            let next_i = (i + 1) % segments;
            triangles.push(RawTriangle3D {
                v0: Vertex3D { pos: front_center, uv: [0.5, 0.5] },
                v1: Vertex3D { pos: cap_front_pts[i].0, uv: cap_front_pts[i].1 },
                v2: Vertex3D { pos: cap_front_pts[next_i].0, uv: cap_front_pts[next_i].1 },
                normal: norm_f,
                color: part_color,
                texture_key: if is_studs_surface(inst, "RightSurface") { Some("__studs".to_string()) } else { None },
                is_neon,
                is_ground_face: false,
                is_transparent,
                is_selected,
            });
            triangles.push(RawTriangle3D {
                v0: Vertex3D { pos: back_center, uv: [0.5, 0.5] },
                v1: Vertex3D { pos: cap_back_pts[next_i].0, uv: cap_back_pts[next_i].1 },
                v2: Vertex3D { pos: cap_back_pts[i].0, uv: cap_back_pts[i].1 },
                normal: norm_b,
                color: part_color,
                texture_key: if is_inlet_surface(inst, "LeftSurface") { Some("__inlets".to_string()) } else { None },
                is_neon,
                is_ground_face: false,
                is_transparent,
                is_selected,
            });
        }
    } else if shape_type == "Wedge" {
        let v0 = part_cframe.transform_point(Vec3::new(-half.x, -half.y, -half.z));
        let v1 = part_cframe.transform_point(Vec3::new(half.x, -half.y, -half.z));
        let v2 = part_cframe.transform_point(Vec3::new(half.x, -half.y, half.z));
        let v3 = part_cframe.transform_point(Vec3::new(-half.x, -half.y, half.z));
        let v4 = part_cframe.transform_point(Vec3::new(-half.x, half.y, -half.z));
        let v5 = part_cframe.transform_point(Vec3::new(half.x, half.y, -half.z));

        let bot_tex = if is_inlet_surface(inst, "BottomSurface") { Some("__inlets".to_string()) } else { None };
        let top_tex = if is_studs_surface(inst, "TopSurface") { Some("__studs".to_string()) } else { None };

        // Bottom (-Y)
        add_quad(&mut triangles, v0, v1, v2, v3, part_cframe.transform_normal(Vec3::new(0.0, -1.0, 0.0)), bot_tex, false);
        // Back (-Z)
        add_quad(&mut triangles, v1, v0, v4, v5, part_cframe.transform_normal(Vec3::new(0.0, 0.0, -1.0)), None, false);
        // Sloped Ramp (+Y/+Z Hypotenuse)
        let ramp_norm = Vec3::new(0.0, half.z, half.y).normalize();
        add_quad(&mut triangles, v3, v2, v5, v4, part_cframe.transform_normal(ramp_norm), top_tex, false);
        // Left (-X triangle)
        triangles.push(RawTriangle3D {
            v0: Vertex3D { pos: v0, uv: [0.0, 0.0] },
            v1: Vertex3D { pos: v3, uv: [1.0, 0.0] },
            v2: Vertex3D { pos: v4, uv: [0.0, 1.0] },
            normal: part_cframe.transform_normal(Vec3::new(-1.0, 0.0, 0.0)),
            color: part_color,
            texture_key: None,
            is_neon,
            is_ground_face: false,
            is_transparent,
            is_selected,
        });
        // Right (+X triangle)
        triangles.push(RawTriangle3D {
            v0: Vertex3D { pos: v2, uv: [0.0, 0.0] },
            v1: Vertex3D { pos: v1, uv: [1.0, 0.0] },
            v2: Vertex3D { pos: v5, uv: [1.0, 1.0] },
            normal: part_cframe.transform_normal(Vec3::new(1.0, 0.0, 0.0)),
            color: part_color,
            texture_key: None,
            is_neon,
            is_ground_face: false,
            is_transparent,
            is_selected,
        });
    } else if shape_type == "CornerWedge" {
        let v_bot0 = part_cframe.transform_point(Vec3::new(-half.x, -half.y, -half.z));
        let v_bot1 = part_cframe.transform_point(Vec3::new(half.x, -half.y, -half.z));
        let v_bot2 = part_cframe.transform_point(Vec3::new(half.x, -half.y, half.z));
        let v_bot3 = part_cframe.transform_point(Vec3::new(-half.x, -half.y, half.z));
        let v_top = part_cframe.transform_point(Vec3::new(-half.x, half.y, -half.z));

        let bot_tex = if is_inlet_surface(inst, "BottomSurface") { Some("__inlets".to_string()) } else { None };
        let top_tex = if is_studs_surface(inst, "TopSurface") { Some("__studs".to_string()) } else { None };

        // Bottom quad (-Y)
        add_quad(&mut triangles, v_bot0, v_bot1, v_bot2, v_bot3, part_cframe.transform_normal(Vec3::new(0.0, -1.0, 0.0)), bot_tex, false);
        // Back triangle (-Z)
        triangles.push(RawTriangle3D {
            v0: Vertex3D { pos: v_bot1, uv: [0.0, 0.0] },
            v1: Vertex3D { pos: v_bot0, uv: [1.0, 0.0] },
            v2: Vertex3D { pos: v_top, uv: [1.0, 1.0] },
            normal: part_cframe.transform_normal(Vec3::new(0.0, 0.0, -1.0)),
            color: part_color,
            texture_key: None,
            is_neon,
            is_ground_face: false,
            is_transparent,
            is_selected,
        });
        // Left triangle (-X)
        triangles.push(RawTriangle3D {
            v0: Vertex3D { pos: v_bot0, uv: [0.0, 0.0] },
            v1: Vertex3D { pos: v_bot3, uv: [1.0, 0.0] },
            v2: Vertex3D { pos: v_top, uv: [0.0, 1.0] },
            normal: part_cframe.transform_normal(Vec3::new(-1.0, 0.0, 0.0)),
            color: part_color,
            texture_key: None,
            is_neon,
            is_ground_face: false,
            is_transparent,
            is_selected,
        });
        // Sloping front-right triangles
        let sloped_norm = Vec3::new(half.y, half.x, half.z).normalize();
        triangles.push(RawTriangle3D {
            v0: Vertex3D { pos: v_bot3, uv: [0.0, 0.0] },
            v1: Vertex3D { pos: v_bot2, uv: [1.0, 0.0] },
            v2: Vertex3D { pos: v_top, uv: [0.5, 1.0] },
            normal: part_cframe.transform_normal(sloped_norm),
            color: part_color,
            texture_key: top_tex.clone(),
            is_neon,
            is_ground_face: false,
            is_transparent,
            is_selected,
        });
        triangles.push(RawTriangle3D {
            v0: Vertex3D { pos: v_bot2, uv: [0.0, 0.0] },
            v1: Vertex3D { pos: v_bot1, uv: [1.0, 0.0] },
            v2: Vertex3D { pos: v_top, uv: [0.5, 1.0] },
            normal: part_cframe.transform_normal(sloped_norm),
            color: part_color,
            texture_key: top_tex,
            is_neon,
            is_ground_face: false,
            is_transparent,
            is_selected,
        });
    } else {
        // Standard Box / Block
        let v = [
            part_cframe.transform_point(Vec3::new(-half.x, -half.y, -half.z)),
            part_cframe.transform_point(Vec3::new(half.x, -half.y, -half.z)),
            part_cframe.transform_point(Vec3::new(half.x, -half.y, half.z)),
            part_cframe.transform_point(Vec3::new(-half.x, -half.y, half.z)),
            part_cframe.transform_point(Vec3::new(-half.x, half.y, -half.z)),
            part_cframe.transform_point(Vec3::new(half.x, half.y, -half.z)),
            part_cframe.transform_point(Vec3::new(half.x, half.y, half.z)),
            part_cframe.transform_point(Vec3::new(-half.x, half.y, half.z)),
        ];

        let mat_tex_opt = match material_str {
            "Brick" => Some("__brick".to_string()),
            "DiamondPlate" | "CorrodedMetal" => Some("__diamond_plate".to_string()),
            "Wood" | "WoodPlanks" => Some("__wood_planks".to_string()),
            "Cobblestone" => Some("__cobblestone".to_string()),
            "Grass" => Some("__grass".to_string()),
            "Concrete" | "Slate" => Some("__concrete".to_string()),
            _ => None,
        };

        let face_definitions = [
            ("Top", [4, 5, 6, 7], Vec3::new(0.0, 1.0, 0.0), true),
            ("Bottom", [0, 3, 2, 1], Vec3::new(0.0, -1.0, 0.0), false),
            ("Front", [3, 2, 6, 7], Vec3::new(0.0, 0.0, 1.0), false),
            ("Back", [1, 0, 4, 5], Vec3::new(0.0, 0.0, -1.0), false),
            ("Right", [2, 1, 5, 6], Vec3::new(1.0, 0.0, 0.0), false),
            ("Left", [0, 3, 7, 4], Vec3::new(-1.0, 0.0, 0.0), false),
        ];

        for (face_name, idx, local_norm, is_top) in face_definitions {
            let face_tex = if let Some((decal_tex, _)) = decal_faces.get(face_name) {
                Some(decal_tex.clone())
            } else if is_top {
                if is_spawn {
                    Some("SpawnLocation.png".to_string())
                } else if is_studs_surface(inst, "TopSurface") && material_str == "Plastic" && !is_baseplate {
                    Some("__studs".to_string())
                } else {
                    mat_tex_opt.clone()
                }
            } else if face_name == "Bottom" {
                if is_inlet_surface(inst, "BottomSurface") && material_str == "Plastic" && !is_baseplate {
                    Some("__inlets".to_string())
                } else {
                    mat_tex_opt.clone()
                }
            } else {
                mat_tex_opt.clone()
            };

            add_quad(
                &mut triangles,
                v[idx[0]], v[idx[1]], v[idx[2]], v[idx[3]],
                part_cframe.transform_normal(local_norm),
                face_tex,
                is_baseplate && is_top,
            );
        }
    }

    let max_radius = (half.x * half.x + half.y * half.y + half.z * half.z).sqrt();
    let aabb_min = part_cframe.pos.sub(&Vec3::new(max_radius, max_radius, max_radius));
    let aabb_max = part_cframe.pos.add(&Vec3::new(max_radius, max_radius, max_radius));

    (
        triangles,
        RenderInstanceInfo {
            referent,
            name: inst.name.clone(),
            class_name: inst.class.to_string(),
            cframe: part_cframe,
            aabb_min,
            aabb_max,
        },
    )
}

fn explorer_icon(class: &str) -> &'static str {
    match class {
        "Tool" => "⚔️",
        "MeshPart" | "SpecialMesh" => "🗿",
        "SpawnLocation" => "🚩",
        "Model" => "📦",
        "VehicleSeat" | "Seat" => "💺",
        _ => "🧱",
    }
}
