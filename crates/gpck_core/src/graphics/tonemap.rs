// src/graphics/tonemap.rs
//! # Real-Time HDR Tonemapping & Color Heatmap Diagnostics
//!
//! Implements industry-standard tone mapping operators and false-color luminance heatmaps:
//! - **Khronos PBR Neutral:** Official Khronos 3D standard preserving base albedo colors without hue shifts.
//! - **False Color Heatmap:** Thermal luminance visualizer to inspect RDO (BLER) compression artifacts and clipping.
//! - **ACES Filmic:** High-dynamic-range filmic curve.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum TonemapOperator {
    #[default]
    None = 0,
    PbrNeutral = 1,
    FalseColor = 2,
    AcesFilmic = 3,
}

impl TonemapOperator {
    pub fn from_index(idx: i32) -> Self {
        match idx {
            1 => Self::PbrNeutral,
            2 => Self::FalseColor,
            3 => Self::AcesFilmic,
            _ => Self::None,
        }
    }
}

#[inline(always)]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline(always)]
fn get_luma(color: [f32; 3]) -> f32 {
    color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722
}

#[inline(always)]
fn linear_to_srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

/// Khronos PBR Neutral Tonemapper.
/// Preserves base material albedo without shifting hues or oversaturating midtones.
#[inline]
pub fn pbr_neutral_tonemapping(color_in: [f32; 3]) -> [f32; 3] {
    let mut color = [
        color_in[0].max(0.0),
        color_in[1].max(0.0),
        color_in[2].max(0.0),
    ];

    let start_compression = 0.8 - 0.04;
    let desaturation = 0.15;

    let x = color[0].min(color[1]).min(color[2]);
    let offset = if x < 0.08 { x - 6.25 * x * x } else { 0.04 };

    color[0] -= offset;
    color[1] -= offset;
    color[2] -= offset;

    let peak = color[0].max(color[1]).max(color[2]);
    if peak < start_compression {
        return color;
    }

    let d = 1.0 - start_compression;
    let new_peak = 1.0 - d * d / (peak + d - start_compression);

    let ratio = new_peak / peak;
    color[0] *= ratio;
    color[1] *= ratio;
    color[2] *= ratio;

    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    let mix_factor = g;
    let target = new_peak;

    [
        color[0] * (1.0 - mix_factor) + target * mix_factor,
        color[1] * (1.0 - mix_factor) + target * mix_factor,
        color[2] * (1.0 - mix_factor) + target * mix_factor,
    ]
}

/// False Color / Exposure Heatmap visualizer.
/// Maps linear luminance into a thermal gradient (Blue -> Cyan -> Green -> Yellow -> Red -> White)
/// to instantly reveal RDO (BLER) compression banding and shadow/highlight clipping.
#[inline]
pub fn tonemap_false_color(color: [f32; 3]) -> [f32; 3] {
    let luma = get_luma(color);
    let log_luma = ((luma.max(1e-5).log2() + 10.0) * 0.071428).clamp(0.0, 1.0);

    let r = smoothstep(0.5, 0.8, log_luma) + if log_luma >= 0.9 { 1.0 } else { 0.0 };
    let g = smoothstep(0.2, 0.5, log_luma) - smoothstep(0.7, 0.9, log_luma);
    let b = smoothstep(0.0, 0.2, log_luma) - smoothstep(0.4, 0.5, log_luma);

    [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]
}

/// ACES Filmic curve (Krzysztof Narkowicz approximation).
#[inline]
pub fn aces_tonemap_raw(x: f32) -> f32 {
    let (a, b, c, d, e) = (2.51, 0.03, 2.43, 0.59, 0.14);
    ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
}

/// Applies the selected tonemapper or heatmap directly in-place across an RGBA8 pixel buffer.
pub fn apply_tonemap_to_rgba8(pixels: &mut [u8], operator: TonemapOperator) {
    if operator == TonemapOperator::None {
        return;
    }

    for chunk in pixels.chunks_exact_mut(4) {
        let r_in = chunk[0] as f32 / 255.0;
        let g_in = chunk[1] as f32 / 255.0;
        let b_in = chunk[2] as f32 / 255.0;

        let (mut r, mut g, mut b) = match operator {
            TonemapOperator::None => (r_in, g_in, b_in),
            TonemapOperator::PbrNeutral => {
                let mapped = pbr_neutral_tonemapping([r_in, g_in, b_in]);
                (mapped[0], mapped[1], mapped[2])
            }
            TonemapOperator::FalseColor => {
                let mapped = tonemap_false_color([r_in, g_in, b_in]);
                (mapped[0], mapped[1], mapped[2])
            }
            TonemapOperator::AcesFilmic => (
                aces_tonemap_raw(r_in),
                aces_tonemap_raw(g_in),
                aces_tonemap_raw(b_in),
            ),
        };

        if operator != TonemapOperator::FalseColor {
            r = linear_to_srgb(r);
            g = linear_to_srgb(g);
            b = linear_to_srgb(b);
        }

        chunk[0] = (r.clamp(0.0, 1.0) * 255.0) as u8;
        chunk[1] = (g.clamp(0.0, 1.0) * 255.0) as u8;
        chunk[2] = (b.clamp(0.0, 1.0) * 255.0) as u8;
    }
}
