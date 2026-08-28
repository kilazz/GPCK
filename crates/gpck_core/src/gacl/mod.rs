//! # Game Asset Conditioning Library (GACL)
//!
//! Dedicated Texture Conditioning Subsystem for Desktop (BC1–BC7) and Mobile (ASTC/ETC2) formats.

pub mod astc;
pub mod bc7;
pub mod conditioner;
pub mod rdo;
pub mod shufflers;
pub mod space_curve;
pub mod transform;

pub use astc::{
    ASTC_BLOCK_SIZE, AstcConditioner, AstcFootprint, ETC2_RGB_BLOCK_SIZE, ETC2_RGBA_BLOCK_SIZE,
};
pub use conditioner::Gacl;
pub use transform::GaclTransform;
