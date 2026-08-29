// crates/gpck_core/src/gpu/vulkan/registry.rs
//! # Centralized Vulkan Compute Pipeline Registry & Shader Dispatcher
//!
//! Provides a type-safe, centralized registry for compiling, caching, resolving,
//! and cleanly destroying Vulkan compute pipelines (Decompression, GACL Unshuffling,
//! Geometry decoders, and Neural Texture DP4a compute passes).

use crate::compression::codecs::CompressionMethod;
use crate::core::error::GpckResult;
use crate::gacl::GaclTransform;
use crate::gpu::vulkan::pipeline::VulkanComputePipeline;
use ash::vk;
use std::collections::HashMap;

/// Strongly-typed key representing all supported GPU compute shader passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderPassKey {
    // ========================================================================
    // Decompression Codec Passes
    // ========================================================================
    GDeflate,
    Zstd,
    BrotliG,

    // ========================================================================
    // GACL Texture Unshuffle Passes
    // ========================================================================
    UnshuffleBc1x,
    UnshuffleBc2,
    UnshuffleBc3x,
    UnshuffleBc4x,
    UnshuffleBc5x,
    UnshuffleBc6h,
    UnshuffleBc7,
    UnshuffleCurveOnly,

    // ========================================================================
    // Geometry & Neural Texture Passes
    // ========================================================================
    DecodeMeshlet,
    DecodeDmm,
    DecodeDgf,
    NtcCompressBc7,
    NtcDecompressDp4a,
}

impl ShaderPassKey {
    /// Returns the compiled SPIR-V bytecode filename for this shader pass.
    #[inline(always)]
    pub fn spv_filename(self) -> &'static str {
        match self {
            Self::GDeflate => "GDeflate.spv",
            Self::Zstd => "Zstd.spv",
            Self::BrotliG => "BrotliGCompute.spv",
            Self::UnshuffleBc1x => "UnshuffleBC1x.spv",
            Self::UnshuffleBc2 => "UnshuffleBC2.spv",
            Self::UnshuffleBc3x => "UnshuffleBC3x.spv",
            Self::UnshuffleBc4x => "UnshuffleBC4x.spv",
            Self::UnshuffleBc5x => "UnshuffleBC5x.spv",
            Self::UnshuffleBc6h => "UnshuffleBC6h.spv",
            Self::UnshuffleBc7 => "UnshuffleBC7.spv",
            Self::UnshuffleCurveOnly => "UnshuffleCurveOnly.spv",
            Self::DecodeMeshlet => "DecodeMeshlet.spv",
            Self::DecodeDmm => "DecodeDmm.spv",
            Self::DecodeDgf => "DGFDecompression.spv",
            Self::NtcCompressBc7 => "NTCCompressBC7.spv",
            Self::NtcDecompressDp4a => "NTCDecompressDP4a.spv",
        }
    }

    /// Maps a high-level compression method to its corresponding compute shader key.
    #[inline(always)]
    pub fn from_compression_method(method: CompressionMethod) -> Option<Self> {
        match method {
            CompressionMethod::GDeflate => Some(Self::GDeflate),
            CompressionMethod::Zstd => Some(Self::Zstd),
            CompressionMethod::BrotliG => Some(Self::BrotliG),
            _ => None,
        }
    }

    /// Maps a GACL texture transformation enum to its corresponding compute shader key.
    #[inline(always)]
    pub fn from_gacl_transform(transform: GaclTransform) -> Option<Self> {
        match transform {
            GaclTransform::Bc1Linear
            | GaclTransform::Bc1LinearSpaceCurve
            | GaclTransform::Bc1V2BitInterleaved
            | GaclTransform::Bc1V2SpaceCurve => Some(Self::UnshuffleBc1x),

            GaclTransform::Bc2AlphaNibble => Some(Self::UnshuffleBc2),

            GaclTransform::Bc3Linear
            | GaclTransform::Bc3LinearSpaceCurve
            | GaclTransform::Bc3V2BitInterleaved
            | GaclTransform::Bc3V2SpaceCurve => Some(Self::UnshuffleBc3x),

            GaclTransform::Bc4Linear | GaclTransform::Bc4LinearSpaceCurve => {
                Some(Self::UnshuffleBc4x)
            }

            GaclTransform::Bc5DualChannel | GaclTransform::Bc5SpaceCurve => {
                Some(Self::UnshuffleBc5x)
            }

            GaclTransform::Bc6hHeaderJoin => Some(Self::UnshuffleBc6h),

            GaclTransform::Bc7ModeSplit | GaclTransform::Bc7ModeJoin => Some(Self::UnshuffleBc7),

            GaclTransform::CurveOnly16B => Some(Self::UnshuffleCurveOnly),

            _ => None,
        }
    }
}

/// Centralized registry managing compute pipelines and their GPU lifecycles.
pub struct PipelineRegistry {
    pipelines: HashMap<ShaderPassKey, VulkanComputePipeline>,
}

impl PipelineRegistry {
    /// Creates an empty compute pipeline registry.
    pub fn new() -> Self {
        Self {
            pipelines: HashMap::new(),
        }
    }

    /// Compiles and registers a single shader pass into the registry.
    pub fn register(
        &mut self,
        device: &ash::Device,
        set_layout: vk::DescriptorSetLayout,
        key: ShaderPassKey,
    ) -> GpckResult<()> {
        let pipeline =
            VulkanComputePipeline::create_from_shader(device, set_layout, key.spv_filename())?;
        self.pipelines.insert(key, pipeline);
        Ok(())
    }

    /// Registers a batch of shader passes, gracefully logging warnings if optional shaders are missing.
    pub fn register_batch(
        &mut self,
        device: &ash::Device,
        set_layout: vk::DescriptorSetLayout,
        keys: &[ShaderPassKey],
    ) {
        for &key in keys {
            if let Err(err) = self.register(device, set_layout, key) {
                crate::core::logger::log_warn(&format!(
                    "[Vulkan Pipeline Registry] Optional shader '{}' ({:?}) was not loaded: {}",
                    key.spv_filename(),
                    key,
                    err
                ));
            }
        }
    }

    /// Returns a reference to a registered compute pipeline.
    #[inline(always)]
    pub fn get(&self, key: ShaderPassKey) -> Option<&VulkanComputePipeline> {
        self.pipelines.get(&key)
    }

    /// Resolves the compute pipeline for a specific compression codec.
    #[inline(always)]
    pub fn get_decompression(
        &self,
        method: Option<CompressionMethod>,
    ) -> Option<&VulkanComputePipeline> {
        let key = ShaderPassKey::from_compression_method(method?)?;
        self.get(key)
    }

    /// Resolves the compute pipeline for a specific GACL texture unshuffle transform.
    #[inline(always)]
    pub fn get_unshuffle(
        &self,
        transform: Option<GaclTransform>,
    ) -> Option<&VulkanComputePipeline> {
        let key = ShaderPassKey::from_gacl_transform(transform?)?;
        self.get(key)
    }

    /// Returns true if a specific shader pass is loaded and ready for dispatch.
    #[inline(always)]
    pub fn has_pass(&self, key: ShaderPassKey) -> bool {
        self.pipelines.contains_key(&key)
    }

    /// Destroys all registered pipelines and layouts on the Vulkan device.
    ///
    /// # Safety
    /// All GPU command buffers executing these pipelines must be idle before calling destroy.
    pub unsafe fn destroy(&mut self, device: &ash::Device) {
        for (_, pipeline) in self.pipelines.drain() {
            unsafe {
                pipeline.destroy(device);
            }
        }
    }
}

impl Default for PipelineRegistry {
    fn default() -> Self {
        Self::new()
    }
}
