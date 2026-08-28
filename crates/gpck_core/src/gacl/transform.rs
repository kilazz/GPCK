// crates/gpck_core/src/gacl/transform.rs
//! # Strongly-Typed GACL Transform Matrix
//!
//! Replaces raw magic numbers with a type-safe enum representing official
//! Microsoft DirectStorage 1.4 GACL transformations, desktop BCn layouts,
//! and mobile ASTC/ETC2 hardware conditioning metadata.

use crate::graphics::dxgi_format::dxgi;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum GaclTransform {
    #[default]
    None = 0,
    Bc1Linear = 1,
    Bc3Linear = 2,
    Bc4Linear = 3,
    Bc5DualChannel = 4,
    Bc2AlphaNibble = 6,
    Bc6hHeaderJoin = 7,
    Bc7ModeSplit = 10,
    Bc7ModeJoin = 11,
    Bc1LinearSpaceCurve = 17,
    Bc3LinearSpaceCurve = 18,
    Bc4LinearSpaceCurve = 19,
    Bc5SpaceCurve = 20,
    CurveOnly16B = 23,
    Bc1V2BitInterleaved = 32,
    Bc1V2SpaceCurve = 33,
    Bc3V2BitInterleaved = 34,
    Bc3V2SpaceCurve = 35,

    // Android & Mobile Texture Transforms
    Astc4x4Linear = 40,
    Astc4x4SpaceCurve = 41,
    Astc6x6Linear = 42,
    Astc6x6SpaceCurve = 43,
    Astc8x8Linear = 44,
    Astc8x8SpaceCurve = 45,
    Etc2RgbLinear = 50,
    Etc2RgbaLinear = 51,
}

impl GaclTransform {
    pub const fn to_u32(self) -> u32 {
        self as u32
    }

    pub const fn from_u32(val: u32) -> Self {
        match val {
            1 => Self::Bc1Linear,
            2 => Self::Bc3Linear,
            3 => Self::Bc4Linear,
            4 => Self::Bc5DualChannel,
            6 => Self::Bc2AlphaNibble,
            7 => Self::Bc6hHeaderJoin,
            10 => Self::Bc7ModeSplit,
            11 => Self::Bc7ModeJoin,
            17 => Self::Bc1LinearSpaceCurve,
            18 => Self::Bc3LinearSpaceCurve,
            19 => Self::Bc4LinearSpaceCurve,
            20 => Self::Bc5SpaceCurve,
            23 => Self::CurveOnly16B,
            32 => Self::Bc1V2BitInterleaved,
            33 => Self::Bc1V2SpaceCurve,
            34 => Self::Bc3V2BitInterleaved,
            35 => Self::Bc3V2SpaceCurve,
            40 => Self::Astc4x4Linear,
            41 => Self::Astc4x4SpaceCurve,
            42 => Self::Astc6x6Linear,
            43 => Self::Astc6x6SpaceCurve,
            44 => Self::Astc8x8Linear,
            45 => Self::Astc8x8SpaceCurve,
            50 => Self::Etc2RgbLinear,
            51 => Self::Etc2RgbaLinear,
            _ => Self::None,
        }
    }

    /// Resolves the corresponding base DXGI format for this GACL transformation.
    #[inline(always)]
    pub fn to_dxgi_format(self) -> u32 {
        match self {
            Self::Bc1Linear
            | Self::Bc1LinearSpaceCurve
            | Self::Bc1V2BitInterleaved
            | Self::Bc1V2SpaceCurve => dxgi::BC1_UNORM,
            Self::Bc2AlphaNibble => dxgi::BC2_UNORM,
            Self::Bc3Linear
            | Self::Bc3LinearSpaceCurve
            | Self::Bc3V2BitInterleaved
            | Self::Bc3V2SpaceCurve => dxgi::BC3_UNORM,
            Self::Bc4Linear | Self::Bc4LinearSpaceCurve => dxgi::BC4_UNORM,
            Self::Bc5DualChannel | Self::Bc5SpaceCurve => dxgi::BC5_UNORM,
            Self::Bc6hHeaderJoin => dxgi::BC6H_UF16,
            Self::Bc7ModeSplit | Self::Bc7ModeJoin => dxgi::BC7_UNORM,
            _ => dxgi::BC7_UNORM,
        }
    }

    #[inline(always)]
    pub fn block_size(self) -> usize {
        match self {
            Self::Bc1Linear
            | Self::Bc1LinearSpaceCurve
            | Self::Bc1V2BitInterleaved
            | Self::Bc1V2SpaceCurve
            | Self::Bc4Linear
            | Self::Bc4LinearSpaceCurve
            | Self::Etc2RgbLinear => 8,
            _ => 16,
        }
    }

    #[inline(always)]
    pub fn has_space_curve(self) -> bool {
        matches!(
            self,
            Self::Bc1LinearSpaceCurve
                | Self::Bc1V2SpaceCurve
                | Self::Bc3LinearSpaceCurve
                | Self::Bc3V2SpaceCurve
                | Self::Bc4LinearSpaceCurve
                | Self::Bc5SpaceCurve
                | Self::CurveOnly16B
                | Self::Astc4x4SpaceCurve
                | Self::Astc6x6SpaceCurve
                | Self::Astc8x8SpaceCurve
        )
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::None => "Raw (None)",
            Self::Bc1Linear => "BC1 Linear (v1)",
            Self::Bc1LinearSpaceCurve => "BC1 Linear + Z-Curve",
            Self::Bc1V2BitInterleaved => "BC1 5:6:5 Split (v2)",
            Self::Bc1V2SpaceCurve => "BC1 5:6:5 + Z-Curve",
            Self::Bc2AlphaNibble => "BC2 Alpha Nibble Split",
            Self::Bc3Linear => "BC3 Linear (v1)",
            Self::Bc3LinearSpaceCurve => "BC3 Linear + Z-Curve",
            Self::Bc3V2BitInterleaved => "BC3 6:6:4 Split (v2)",
            Self::Bc3V2SpaceCurve => "BC3 6:6:4 + Z-Curve",
            Self::Bc4Linear => "BC4 Linear",
            Self::Bc4LinearSpaceCurve => "BC4 Linear + Z-Curve",
            Self::Bc5DualChannel => "BC5 Dual Channel",
            Self::Bc5SpaceCurve => "BC5 Dual Channel + Z-Curve",
            Self::Bc6hHeaderJoin => "BC6H Header/Index Join",
            Self::Bc7ModeSplit => "BC7 Mode-Split (3-Stream)",
            Self::Bc7ModeJoin => "BC7 Mode-Join (24-bit)",
            Self::CurveOnly16B => "Morton Z-Curve Only (16B)",
            Self::Astc4x4Linear => "ASTC 4x4 Linear Split",
            Self::Astc4x4SpaceCurve => "ASTC 4x4 + Z-Curve",
            Self::Astc6x6Linear => "ASTC 6x6 Linear Split",
            Self::Astc6x6SpaceCurve => "ASTC 6x6 + Z-Curve",
            Self::Astc8x8Linear => "ASTC 8x8 Linear Split",
            Self::Astc8x8SpaceCurve => "ASTC 8x8 + Z-Curve",
            Self::Etc2RgbLinear => "ETC2 RGB Linear",
            Self::Etc2RgbaLinear => "ETC2 RGBA Linear",
        }
    }

    pub fn gpu_shader_name(self) -> Option<&'static str> {
        match self {
            Self::Bc1Linear
            | Self::Bc1LinearSpaceCurve
            | Self::Bc1V2BitInterleaved
            | Self::Bc1V2SpaceCurve => Some("UnshuffleBC1x.spv"),
            Self::Bc2AlphaNibble => Some("UnshuffleBC2.spv"),
            Self::Bc3Linear
            | Self::Bc3LinearSpaceCurve
            | Self::Bc3V2BitInterleaved
            | Self::Bc3V2SpaceCurve => Some("UnshuffleBC3x.spv"),
            Self::Bc4Linear | Self::Bc4LinearSpaceCurve => Some("UnshuffleBC4x.spv"),
            Self::Bc5DualChannel | Self::Bc5SpaceCurve => Some("UnshuffleBC5x.spv"),
            Self::Bc6hHeaderJoin => Some("UnshuffleBC6h.spv"),
            Self::Bc7ModeSplit | Self::Bc7ModeJoin => Some("UnshuffleBC7.spv"),
            Self::CurveOnly16B => Some("UnshuffleCurveOnly.spv"),
            _ => None,
        }
    }

    pub fn from_str_label(label: &str) -> Option<Self> {
        match label {
            "None (Raw)" | "Raw (None)" => Some(Self::None),
            "Linear Split (v1)" | "Linear Split" | "BC1 Linear (v1)" => Some(Self::Bc1Linear),
            "Linear + Z-Curve" | "BC1 Linear + Z-Curve" => Some(Self::Bc1LinearSpaceCurve),
            "5:6:5 Split (v2)" | "BC1 5:6:5 Split (v2)" => Some(Self::Bc1V2BitInterleaved),
            "5:6:5 + Z-Curve" | "BC1 5:6:5 + Z-Curve" => Some(Self::Bc1V2SpaceCurve),
            "Alpha Nibble Split" | "BC2 Alpha Nibble Split" => Some(Self::Bc2AlphaNibble),
            "6:6:4 Split (v2)" | "BC3 6:6:4 Split (v2)" => Some(Self::Bc3V2BitInterleaved),
            "6:6:4 + Z-Curve" | "BC3 6:6:4 + Z-Curve" => Some(Self::Bc3V2SpaceCurve),
            "Dual Channel Split" | "BC5 Dual Channel" => Some(Self::Bc5DualChannel),
            "Dual Channel + Z-Curve" | "BC5 Dual Channel + Z-Curve" => Some(Self::Bc5SpaceCurve),
            "Header/Index Join" | "BC6H Header/Index Join" => Some(Self::Bc6hHeaderJoin),
            "Mode-Split (3-Stream)" | "BC7 Mode-Split (3-Stream)" => Some(Self::Bc7ModeSplit),
            "Mode-Join (24-bit)" | "BC7 Mode-Join (24-bit)" => Some(Self::Bc7ModeJoin),
            "ASTC 4x4 Linear" | "ASTC 4x4 Linear Split" => Some(Self::Astc4x4Linear),
            "ASTC 4x4 + Z-Curve" => Some(Self::Astc4x4SpaceCurve),
            "ASTC 6x6 Linear" | "ASTC 6x6 Linear Split" => Some(Self::Astc6x6Linear),
            "ASTC 6x6 + Z-Curve" => Some(Self::Astc6x6SpaceCurve),
            "ASTC 8x8 Linear" | "ASTC 8x8 Linear Split" => Some(Self::Astc8x8Linear),
            "ASTC 8x8 + Z-Curve" => Some(Self::Astc8x8SpaceCurve),
            "ETC2 RGB Linear" => Some(Self::Etc2RgbLinear),
            "ETC2 RGBA Linear" => Some(Self::Etc2RgbaLinear),
            _ => None,
        }
    }
}
