// ============================================================================
// viewport3d.rs  —  shared 3D math & helpers for the Bevy renderer
// ----------------------------------------------------------------------------
// This module keeps only the shared helpers that the Bevy viewport
// (`bevy_rbxl.rs` / `viewport3d_bevy.rs`) depends on: the orbit-camera preset
// enum, the left-handed "S space" vector/matrix math, the Roblox BrickColor
// registry, and CFrame extraction from the instance tree.
//
// NOTE: The old CPU software rasterizer (`Viewport3D` + its triangle
// generator) that lived here has been removed — the Bevy engine is now the
// only 3D renderer.
// ============================================================================

use egui::Color32;
use rbx_dom_weak::types::Variant;

pub enum CameraPreset {
    Isometric,
    Top,
    Front,
    Side,
}


// ----------------------------------------------------------------------------
// S space math (matches the coordinates OpenRBLX / Roblox use)
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
// CFrame extraction
// ----------------------------------------------------------------------------

pub fn extract_instance_cframe(inst: &rbx_dom_weak::Instance) -> CFrame3D {
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
