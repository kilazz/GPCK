// crates/gpck_core/src/gpu/vulkan/pipeline.rs
//! # Vulkan Compute Pipeline & Shader Bytecode Resolver

use crate::core::error::{GpckError, GpckResult};
use crate::graphics::spirv::SpirvReflection;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};

// Auto-generated embedded compile-time shader lookup registry
mod embedded_shaders {
    include!(concat!(env!("OUT_DIR"), "/embedded_shaders.rs"));
}

/// Push constants layout for GACL texture unshuffling compute shaders.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct GaclPushConstants {
    pub buffer_size_in_bytes: u32,
    pub buffer_offset_in_bytes: u32,
    pub transform_id: u32,
    pub width_in_pixels: u32,
}

pub struct VulkanComputePipeline {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub reflection: SpirvReflection,
}

impl VulkanComputePipeline {
    pub fn create_from_shader(
        device: &ash::Device,
        descriptor_set_layout: vk::DescriptorSetLayout,
        shader_file: &str,
    ) -> GpckResult<Self> {
        let shader_bytes = Self::find_shader_bytecode(shader_file)?;
        if shader_bytes.is_empty() || !shader_bytes.len().is_multiple_of(4) {
            return Err(GpckError::VulkanError(format!(
                "Invalid SPIR-V bytecode size in {}",
                shader_file
            )));
        }

        let mut aligned_words = vec![0u32; shader_bytes.len() / 4];
        unsafe {
            std::ptr::copy_nonoverlapping(
                shader_bytes.as_ptr(),
                aligned_words.as_mut_ptr() as *mut u8,
                shader_bytes.len(),
            );
        }

        let reflection = SpirvReflection::parse_words(&aligned_words)
            .map_err(|e| GpckError::VulkanError(e.to_string()))?;
        let push_ranges = reflection.get_push_constant_ranges();
        let set_layouts = [descriptor_set_layout];

        let pipe_layout_info = vk::PipelineLayoutCreateInfo {
            s_type: vk::StructureType::PIPELINE_LAYOUT_CREATE_INFO,
            set_layout_count: set_layouts.len() as u32,
            p_set_layouts: set_layouts.as_ptr(),
            push_constant_range_count: push_ranges.len() as u32,
            p_push_constant_ranges: push_ranges.as_ptr(),
            ..Default::default()
        };
        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(&pipe_layout_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        let shader_info = vk::ShaderModuleCreateInfo {
            s_type: vk::StructureType::SHADER_MODULE_CREATE_INFO,
            code_size: shader_bytes.len(),
            p_code: aligned_words.as_ptr(),
            ..Default::default()
        };
        let shader_module = unsafe {
            device
                .create_shader_module(&shader_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        let entry_name = if reflection.entry_point_name.is_empty() {
            "main"
        } else {
            &reflection.entry_point_name
        };
        let entry_point =
            CString::new(entry_name).unwrap_or_else(|_| CString::new("main").unwrap());

        let stage_info = vk::PipelineShaderStageCreateInfo {
            s_type: vk::StructureType::PIPELINE_SHADER_STAGE_CREATE_INFO,
            stage: vk::ShaderStageFlags::COMPUTE,
            module: shader_module,
            p_name: entry_point.as_ptr(),
            ..Default::default()
        };

        let compute_info = vk::ComputePipelineCreateInfo {
            s_type: vk::StructureType::COMPUTE_PIPELINE_CREATE_INFO,
            stage: stage_info,
            layout: pipeline_layout,
            ..Default::default()
        };

        let pipelines = unsafe {
            match device.create_compute_pipelines(vk::PipelineCache::null(), &[compute_info], None)
            {
                Ok(p) => p,
                Err((_, e)) => {
                    device.destroy_shader_module(shader_module, None);
                    device.destroy_pipeline_layout(pipeline_layout, None);
                    return Err(GpckError::VulkanError(format!(
                        "Failed to create compute pipeline: {:?}",
                        e
                    )));
                }
            }
        };

        unsafe { device.destroy_shader_module(shader_module, None) };

        Ok(Self {
            pipeline: pipelines[0],
            pipeline_layout,
            reflection,
        })
    }

    /// Resolves SPIR-V bytecode from the embedded compile-time registry or hot-reload filesystem.
    pub fn find_shader_bytecode(shader_name: &str) -> GpckResult<Vec<u8>> {
        if let Some(bytes) = embedded_shaders::get_embedded_shader(shader_name) {
            return Ok(bytes.to_vec());
        }

        let mut candidates = Vec::new();
        candidates.push(PathBuf::from(shader_name));
        candidates.push(Path::new("shaders").join(shader_name));
        for sub in ["GACL", "GDeflate", "ZSTD", "BrotliG", "Geometry"] {
            candidates.push(Path::new("shaders").join(sub).join(shader_name));
        }

        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            candidates.push(exe_dir.join(shader_name));
            candidates.push(exe_dir.join("shaders").join(shader_name));
            for sub in ["GACL", "GDeflate", "ZSTD", "BrotliG", "Geometry"] {
                candidates.push(exe_dir.join("shaders").join(sub).join(shader_name));
            }
        }

        for p in candidates {
            if p.exists()
                && let Ok(bytes) = fs::read(&p)
                && !bytes.is_empty()
            {
                return Ok(bytes);
            }
        }

        Err(GpckError::ShaderNotFound(shader_name.to_string()))
    }

    /// Destroys the Vulkan compute pipeline and its pipeline layout.
    ///
    /// # Safety
    /// - `device` must be the same logical device used to create this pipeline and layout.
    /// - The pipeline and its layout must not be currently in use by any active in-flight GPU command buffers.
    pub unsafe fn destroy(&self, device: &ash::Device) {
        unsafe {
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}
