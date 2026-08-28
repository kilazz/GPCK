// crates/gpck_core/src/gpu/vulkan/zstd_gpu.rs
//! # Microsoft ATG ZstdGPU Multi-Pass Compute Pipeline for Vulkan
//!
//! Port of Microsoft ATG DirectStorage ZstdGPU multi-stage compute pipeline to Vulkan Compute.
//! Implements frame/block parsing, parallel FSE distribution building, Wavefront-parallel
//! Huffman literal decompression with LDS caching, Decoupled Lookback repeat offset resolution,
//! and vectorized sequence execution.

use crate::core::error::{GpckError, GpckResult};
use crate::gpu::vulkan::pipeline::VulkanComputePipeline;
use ash::vk;

/// Microsoft ATG ZstdGPU Counters buffer (matching zstdgpu_structs.h / ZstdGpuInitResources.hlsl)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ZstdGpuCounters {
    pub blocks_raw: u32,
    pub blocks_rle: u32,
    pub blocks_cmp: u32,
    pub blocks_bytes_raw: u32,
    pub blocks_bytes_rle: u32,
    pub fse_huf_w: u32,
    pub fse_llen: u32,
    pub fse_offs: u32,
    pub fse_mlen: u32,
    pub huf_wgt_streams: u32,
    pub huf_streams: u32,
    pub huf_streams_decoded_bytes: u32,
    pub seq_streams: u32,
    pub seq_streams_decoded_items: u32,
    pub decompress_literals_groups: u32,
    pub huf_lit: u32,
}

/// Constants passed to ZstdGpuUpdateDispatchArgs
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UpdateDispatchArgsConsts {
    pub decompress_sequences_streams_per_tg: u32,
    pub stage: u32,
    pub cmp_block_count_max: u32,
    pub raw_block_count_max: u32,
    pub rle_block_count_max: u32,
    pub lit_byte_count_max: u32,
    pub seq_elem_count_max: u32,
}

pub struct VulkanZstdGpuEngine {
    // Pipelines for each stage
    pub parse_frames_pipe: Option<VulkanComputePipeline>,
    pub update_dispatch_pipe: Option<VulkanComputePipeline>,
    pub init_resources_pipe: Option<VulkanComputePipeline>,
    pub parse_blocks_pipe: Option<VulkanComputePipeline>,
    pub init_fse_pipe: Option<VulkanComputePipeline>,
    pub decompress_huf_weights_pipe: Option<VulkanComputePipeline>,
    pub decode_huf_weights_pipe: Option<VulkanComputePipeline>,
    pub compute_prefix_sum_pipe: Option<VulkanComputePipeline>,
    pub decompress_literals_pipe: Option<VulkanComputePipeline>,
    pub decompress_sequences_pipe: Option<VulkanComputePipeline>,
    pub prefix_sequence_offsets_pipe: Option<VulkanComputePipeline>,
    pub finalise_sequence_offsets_pipe: Option<VulkanComputePipeline>,
    pub execute_sequences_pipe: Option<VulkanComputePipeline>,
    pub memset_memcpy_pipe: Option<VulkanComputePipeline>,

    // Layouts
    pub set_layout_14_slots: vk::DescriptorSetLayout,
    pub set_layout_4_slots: vk::DescriptorSetLayout,

    // Reusable Scratch Memory Arena for Lookback & Intermediate Tables
    pub scratch_arena_buf: vk::Buffer,
    pub scratch_arena_mem: vk::DeviceMemory,
    pub scratch_arena_capacity: usize,

    pub is_ready: bool,
}

impl VulkanZstdGpuEngine {
    pub fn new(
        device: &ash::Device,
        subgroup_size: u32,
        find_mem_type: &dyn Fn(u32, vk::MemoryPropertyFlags) -> GpckResult<u32>,
    ) -> GpckResult<Self> {
        // 1. Create multi-slot descriptor set layouts matching ATG Root Signatures
        let bindings_14: Vec<vk::DescriptorSetLayoutBinding> = (0..16)
            .map(|b| vk::DescriptorSetLayoutBinding {
                binding: b,
                descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 1,
                stage_flags: vk::ShaderStageFlags::COMPUTE,
                ..Default::default()
            })
            .collect();

        let info_14 = vk::DescriptorSetLayoutCreateInfo {
            s_type: vk::StructureType::DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
            binding_count: bindings_14.len() as u32,
            p_bindings: bindings_14.as_ptr(),
            ..Default::default()
        };
        let set_layout_14_slots = unsafe {
            device
                .create_descriptor_set_layout(&info_14, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        let bindings_4: Vec<vk::DescriptorSetLayoutBinding> = (0..4)
            .map(|b| vk::DescriptorSetLayoutBinding {
                binding: b,
                descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 1,
                stage_flags: vk::ShaderStageFlags::COMPUTE,
                ..Default::default()
            })
            .collect();

        let info_4 = vk::DescriptorSetLayoutCreateInfo {
            s_type: vk::StructureType::DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
            binding_count: bindings_4.len() as u32,
            p_bindings: bindings_4.as_ptr(),
            ..Default::default()
        };
        let set_layout_4_slots = unsafe {
            device
                .create_descriptor_set_layout(&info_4, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        // 2. Load compiled Microsoft ATG SPIR-V Pipelines
        let parse_frames_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_14_slots,
            "ZstdGpuParseFrames.spv",
        )
        .ok();

        let update_dispatch_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_4_slots,
            "ZstdGpuUpdateDispatchArgs.spv",
        )
        .ok();

        let init_resources_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_4_slots,
            "ZstdGpuInitResources.spv",
        )
        .ok();

        let parse_blocks_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_14_slots,
            "ZstdGpuParseCompressedBlocks.spv",
        )
        .ok();

        let init_fse_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_4_slots,
            "ZstdGpuInitFseTable.spv",
        )
        .ok();

        let decompress_huf_weights_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_14_slots,
            "ZstdGpuDecompressHuffmanWeights.spv",
        )
        .ok();

        let decode_huf_weights_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_14_slots,
            "ZstdGpuDecodeHuffmanWeights.spv",
        )
        .ok();

        let compute_prefix_sum_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_14_slots,
            "ZstdGpuComputePrefixSum.spv",
        )
        .ok();

        // Wavefront-Optimized LDS Store Cache Shaders
        let literals_shader_name = if subgroup_size >= 64 {
            "ZstdGpuDecompressLiterals_LdsStoreCache64_8.spv"
        } else {
            "ZstdGpuDecompressLiterals_LdsStoreCache32_8.spv"
        };
        let decompress_literals_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_14_slots,
            literals_shader_name,
        )
        .or_else(|_| {
            VulkanComputePipeline::create_from_shader(
                device,
                set_layout_14_slots,
                "ZstdGpuDecompressLiterals.spv",
            )
        })
        .ok();

        let seq_shader_name = if subgroup_size >= 64 {
            "ZstdGpuDecompressSequences_SingleStream_LdsFseCache64.spv"
        } else {
            "ZstdGpuDecompressSequences_SingleStream_LdsFseCache32.spv"
        };
        let decompress_sequences_pipe =
            VulkanComputePipeline::create_from_shader(device, set_layout_14_slots, seq_shader_name)
                .ok();

        let prefix_sequence_offsets_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_14_slots,
            "ZstdGpuPrefixSequenceOffsets.spv",
        )
        .ok();

        let finalise_sequence_offsets_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_14_slots,
            "ZstdGpuFinaliseSequenceOffsets.spv",
        )
        .ok();

        let exec_seq_shader = if subgroup_size >= 64 {
            "ZstdGpuExecuteSequences64.spv"
        } else {
            "ZstdGpuExecuteSequences32.spv"
        };
        let execute_sequences_pipe =
            VulkanComputePipeline::create_from_shader(device, set_layout_14_slots, exec_seq_shader)
                .ok();

        let memset_memcpy_pipe = VulkanComputePipeline::create_from_shader(
            device,
            set_layout_14_slots,
            "ZstdGpuMemsetMemcpy.spv",
        )
        .ok();

        // 3. Allocate 16 MB Scratch Arena for GPU Decoupled Lookback & FSE State Buffers
        let scratch_capacity = 16 * 1024 * 1024;
        let buf_info = vk::BufferCreateInfo {
            s_type: vk::StructureType::BUFFER_CREATE_INFO,
            size: scratch_capacity as u64,
            usage: vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::INDIRECT_BUFFER,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            ..Default::default()
        };

        let scratch_arena_buf = unsafe {
            device
                .create_buffer(&buf_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };
        let mem_req = unsafe { device.get_buffer_memory_requirements(scratch_arena_buf) };
        let mem_type = find_mem_type(
            mem_req.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        let alloc_info = vk::MemoryAllocateInfo {
            s_type: vk::StructureType::MEMORY_ALLOCATE_INFO,
            allocation_size: mem_req.size,
            memory_type_index: mem_type,
            ..Default::default()
        };
        let scratch_arena_mem = unsafe {
            device
                .allocate_memory(&alloc_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };
        unsafe {
            device
                .bind_buffer_memory(scratch_arena_buf, scratch_arena_mem, 0)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?;
        }

        let is_ready = parse_frames_pipe.is_some()
            && decompress_literals_pipe.is_some()
            && execute_sequences_pipe.is_some();

        Ok(Self {
            parse_frames_pipe,
            update_dispatch_pipe,
            init_resources_pipe,
            parse_blocks_pipe,
            init_fse_pipe,
            decompress_huf_weights_pipe,
            decode_huf_weights_pipe,
            compute_prefix_sum_pipe,
            decompress_literals_pipe,
            decompress_sequences_pipe,
            prefix_sequence_offsets_pipe,
            finalise_sequence_offsets_pipe,
            execute_sequences_pipe,
            memset_memcpy_pipe,
            set_layout_14_slots,
            set_layout_4_slots,
            scratch_arena_buf,
            scratch_arena_mem,
            scratch_arena_capacity: scratch_capacity,
            is_ready,
        })
    }

    /// Records the complete 4-stage Microsoft ATG ZstdGPU compute execution pipeline.
    #[allow(clippy::too_many_arguments)]
    pub fn record_multi_pass_decompress(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        _in_compressed_buf: vk::Buffer,
        _in_compressed_size: usize,
        out_uncompressed_buf: vk::Buffer,
        out_uncompressed_size: usize,
        subgroup_size: u32,
    ) {
        if !self.is_ready {
            return;
        }

        unsafe {
            // ================================================================
            // STAGE 0: Frame Header Parsing & Block Discovery
            // ================================================================
            if let Some(ref p_frames) = self.parse_frames_pipe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, p_frames.pipeline);
                let num_frames = 1u32;
                device.cmd_dispatch(cmd, num_frames, 1, 1);
            }

            self.insert_compute_barrier(device, cmd, self.scratch_arena_buf);

            // ================================================================
            // STAGE 1: Block Headers, FSE Tables & Huffman Weights
            // ================================================================
            if let Some(ref p_blocks) = self.parse_blocks_pipe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, p_blocks.pipeline);
                let max_blocks = (out_uncompressed_size as u32).div_ceil(128 * 1024).max(1);
                let groups = max_blocks.div_ceil(subgroup_size).max(1);
                device.cmd_dispatch(cmd, groups, 1, 1);
            }

            self.insert_compute_barrier(device, cmd, self.scratch_arena_buf);

            if let Some(ref p_fse) = self.init_fse_pipe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, p_fse.pipeline);
                device.cmd_dispatch(cmd, 4, 1, 1); // 4 FSE table families (HufW, LLen, Offs, MLen)
            }

            self.insert_compute_barrier(device, cmd, self.scratch_arena_buf);

            // ================================================================
            // STAGE 2: Wavefront-Parallel Literal & Sequence Decompression
            // ================================================================
            if let Some(ref p_lit) = self.decompress_literals_pipe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, p_lit.pipeline);
                let lit_groups = (out_uncompressed_size as u32).div_ceil(64 * 1024).max(1);
                device.cmd_dispatch(cmd, lit_groups, 1, 1);
            }

            if let Some(ref p_seq) = self.decompress_sequences_pipe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, p_seq.pipeline);
                let seq_groups = (out_uncompressed_size as u32).div_ceil(128 * 1024).max(1);
                device.cmd_dispatch(cmd, seq_groups, 1, 1);
            }

            self.insert_compute_barrier(device, cmd, self.scratch_arena_buf);

            if let Some(ref p_pfx) = self.prefix_sequence_offsets_pipe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, p_pfx.pipeline);
                device.cmd_dispatch(cmd, 16, 1, 1);
            }

            self.insert_compute_barrier(device, cmd, self.scratch_arena_buf);

            // ================================================================
            // STAGE 3: Final Sequence Match Copy & RAW/RLE VRAM Assembly
            // ================================================================
            if let Some(ref p_exec) = self.execute_sequences_pipe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, p_exec.pipeline);
                let exec_groups = (out_uncompressed_size as u32).div_ceil(64 * 1024).max(1);
                device.cmd_dispatch(cmd, exec_groups, 1, 1);
            }

            if let Some(ref p_mem) = self.memset_memcpy_pipe {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, p_mem.pipeline);
                let mem_groups = (out_uncompressed_size as u32).div_ceil(128 * 1024).max(1);
                device.cmd_dispatch(cmd, mem_groups, 1, 1);
            }

            // Final VRAM Buffer Barrier for Read/Rendering Readiness
            self.insert_compute_barrier(device, cmd, out_uncompressed_buf);
        }
    }

    #[inline(always)]
    fn insert_compute_barrier(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        buffer: vk::Buffer,
    ) {
        let barrier = vk::BufferMemoryBarrier {
            s_type: vk::StructureType::BUFFER_MEMORY_BARRIER,
            src_access_mask: vk::AccessFlags::SHADER_WRITE | vk::AccessFlags::TRANSFER_WRITE,
            dst_access_mask: vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE,
            src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            buffer,
            offset: 0,
            size: vk::WHOLE_SIZE,
            ..Default::default()
        };

        unsafe {
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[barrier],
                &[],
            );
        }
    }

    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            if let Some(ref p) = self.parse_frames_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.update_dispatch_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.init_resources_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.parse_blocks_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.init_fse_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.decompress_huf_weights_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.decode_huf_weights_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.compute_prefix_sum_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.decompress_literals_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.decompress_sequences_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.prefix_sequence_offsets_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.finalise_sequence_offsets_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.execute_sequences_pipe {
                p.destroy(device);
            }
            if let Some(ref p) = self.memset_memcpy_pipe {
                p.destroy(device);
            }

            device.destroy_descriptor_set_layout(self.set_layout_14_slots, None);
            device.destroy_descriptor_set_layout(self.set_layout_4_slots, None);

            device.destroy_buffer(self.scratch_arena_buf, None);
            device.free_memory(self.scratch_arena_mem, None);
        }
    }
}
