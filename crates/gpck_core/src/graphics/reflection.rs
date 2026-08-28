// src/graphics/reflection.rs
//! # Unified Shader Reflection & Binding Metadata Matrix
//!
//! Provides a single, unified intermediate representation for compiled SPIR-V,
//! DXBC (SM4/SM5), and DXIL (SM6) shader bytecode.

use ash::vk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum ShaderStage {
    Vertex = 0,
    Pixel = 1,
    Geometry = 2,
    Hull = 3,
    Domain = 4,
    #[default]
    Compute = 5,
    Mesh = 6,
    Amplification = 7,
    RayGeneration = 8,
    Intersection = 9,
    AnyHit = 10,
    ClosestHit = 11,
    Miss = 12,
    Callable = 13,
    Unknown = 99,
}

impl ShaderStage {
    pub fn from_dxbc_u16(val: u16) -> Self {
        match val {
            0 => Self::Pixel,
            1 => Self::Vertex,
            2 => Self::Geometry,
            3 => Self::Hull,
            4 => Self::Domain,
            5 => Self::Compute,
            13 => Self::Mesh,
            14 => Self::Amplification,
            _ => Self::Unknown,
        }
    }

    pub fn from_spirv_u32(val: u32) -> Self {
        match val {
            0 => Self::Vertex,
            1 => Self::Hull,
            2 => Self::Domain,
            3 => Self::Geometry,
            4 => Self::Pixel,
            5 => Self::Compute,
            5267 => Self::Amplification,
            5268 => Self::Mesh,
            5313 => Self::RayGeneration,
            5314 => Self::Intersection,
            5315 => Self::AnyHit,
            5316 => Self::ClosestHit,
            5317 => Self::Miss,
            5318 => Self::Callable,
            _ => Self::Compute,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DescriptorType {
    Sampler,
    CombinedImageSampler,
    SampledImage,
    StorageImage,
    UniformTexelBuffer,
    StorageTexelBuffer,
    UniformBuffer,
    #[default]
    StorageBuffer,
    PushConstant,
    AccelerationStructure,
}

impl DescriptorType {
    pub fn to_vk_descriptor_type(self) -> vk::DescriptorType {
        match self {
            Self::Sampler => vk::DescriptorType::SAMPLER,
            Self::CombinedImageSampler => vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            Self::SampledImage => vk::DescriptorType::SAMPLED_IMAGE,
            Self::StorageImage => vk::DescriptorType::STORAGE_IMAGE,
            Self::UniformTexelBuffer => vk::DescriptorType::UNIFORM_TEXEL_BUFFER,
            Self::StorageTexelBuffer => vk::DescriptorType::STORAGE_TEXEL_BUFFER,
            Self::UniformBuffer => vk::DescriptorType::UNIFORM_BUFFER,
            Self::StorageBuffer => vk::DescriptorType::STORAGE_BUFFER,
            Self::AccelerationStructure => vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
            Self::PushConstant => vk::DescriptorType::STORAGE_BUFFER,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DescriptorBindingInfo {
    pub name: String,
    pub set: u32,
    pub binding: u32,
    pub descriptor_type: DescriptorType,
    pub is_read_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct PushConstantRangeInfo {
    pub name: String,
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ShaderReflectionInfo {
    pub entry_point_name: String,
    pub stage: ShaderStage,
    pub major_version: u8,
    pub minor_version: u8,
    pub thread_group_size: (u32, u32, u32),
    pub bindings: Vec<DescriptorBindingInfo>,
    pub push_constants: Vec<PushConstantRangeInfo>,
    pub constant_buffer_count: usize,
    pub srv_count: usize,
    pub uav_count: usize,
    pub sampler_count: usize,
}

impl ShaderReflectionInfo {
    #[inline(always)]
    pub fn calculate_dispatch_groups_1d(&self, total_elements: u32) -> u32 {
        let block_size = self.thread_group_size.0.max(1);
        total_elements.div_ceil(block_size)
    }
}
