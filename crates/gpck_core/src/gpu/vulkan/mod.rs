// crates/gpck_core/src/gpu/vulkan/mod.rs
//! # Vulkan Compute Shader Decompressor & Dynamic SPIR-V Dispatcher

pub mod descriptor;
pub mod pipeline;
pub mod worker;
pub mod zstd_gpu;

pub use descriptor::DescriptorPoolManager;
pub use pipeline::{GaclPushConstants, VulkanComputePipeline};
pub use worker::{BoundedWorkerPool, MAX_GPU_WORKERS, WorkerContext};
pub use zstd_gpu::VulkanZstdGpuEngine;

use crate::compression::codecs::CompressionMethod;
use crate::core::error::{GpckError, GpckResult};
use crate::gacl::GaclTransform;
use crate::gpu::traits::GpuStreamingBackend;

use ash::vk;
use std::ffi::{CStr, CString, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub struct VulkanDecompressor {
    instance: ash::Instance,
    physical_device: vk::PhysicalDevice,
    device: ash::Device,
    compute_queue: Mutex<vk::Queue>,
    _queue_family_index: u32,
    command_pool: Mutex<vk::CommandPool>,

    timeline_semaphore: vk::Semaphore,
    timeline_value: AtomicU64,

    gdeflate_pipeline: Option<VulkanComputePipeline>,
    zstd_pipeline: Option<VulkanComputePipeline>,
    brotlig_pipeline: Option<VulkanComputePipeline>,
    unshuffle_bc1x_pipeline: Option<VulkanComputePipeline>,
    unshuffle_bc2_pipeline: Option<VulkanComputePipeline>,
    unshuffle_bc3x_pipeline: Option<VulkanComputePipeline>,
    unshuffle_bc4x_pipeline: Option<VulkanComputePipeline>,
    unshuffle_bc5x_pipeline: Option<VulkanComputePipeline>,
    unshuffle_bc6h_pipeline: Option<VulkanComputePipeline>,
    unshuffle_bc7_pipeline: Option<VulkanComputePipeline>,
    unshuffle_curve_only_pipeline: Option<VulkanComputePipeline>,

    pub zstd_engine: Option<VulkanZstdGpuEngine>,

    desc_pool_mgr: Mutex<DescriptorPoolManager>,
    gdef_set_layout: vk::DescriptorSetLayout,
    unshuffle_set_layout: vk::DescriptorSetLayout,
    debug_messenger: Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    device_name: String,
    subgroup_size: u32,
    worker_pool: Arc<BoundedWorkerPool>,
}

unsafe impl Send for VulkanDecompressor {}
unsafe impl Sync for VulkanDecompressor {}

fn get_vulkan_entry() -> GpckResult<&'static ash::Entry> {
    static ENTRY: OnceLock<Option<ash::Entry>> = OnceLock::new();
    let entry_opt = ENTRY.get_or_init(|| unsafe { ash::Entry::load().ok() });
    entry_opt
        .as_ref()
        .ok_or_else(|| GpckError::VulkanError("Failed to load Vulkan library".to_string()))
}

impl VulkanDecompressor {
    pub fn shared() -> Option<Arc<Self>> {
        static INSTANCE: OnceLock<Option<Arc<VulkanDecompressor>>> = OnceLock::new();
        INSTANCE
            .get_or_init(|| Self::new().ok().map(Arc::new))
            .clone()
    }

    pub fn new() -> GpckResult<Self> {
        let entry = get_vulkan_entry()?;

        let app_name = CString::new("GPCK Vulkan Pipeline")
            .map_err(|e| GpckError::VulkanError(e.to_string()))?;
        let app_info = vk::ApplicationInfo {
            s_type: vk::StructureType::APPLICATION_INFO,
            p_application_name: app_name.as_ptr(),
            api_version: vk::API_VERSION_1_2,
            ..Default::default()
        };

        let mut instance_extensions = Vec::new();
        let available_extensions = unsafe {
            entry
                .enumerate_instance_extension_properties(None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };
        let has_debug_utils = available_extensions.iter().any(|ext| {
            let name = unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) };
            name == c"VK_EXT_debug_utils"
        });

        if has_debug_utils {
            instance_extensions.push(c"VK_EXT_debug_utils".as_ptr());
        }

        let instance_layers: Vec<*const std::ffi::c_char> = {
            #[cfg(debug_assertions)]
            {
                let available_layers = unsafe {
                    entry
                        .enumerate_instance_layer_properties()
                        .unwrap_or_default()
                };
                if available_layers.iter().any(|layer| {
                    let name = unsafe { CStr::from_ptr(layer.layer_name.as_ptr()) };
                    name == c"VK_LAYER_KHRONOS_validation"
                }) {
                    vec![c"VK_LAYER_KHRONOS_validation".as_ptr()]
                } else {
                    Vec::new()
                }
            }
            #[cfg(not(debug_assertions))]
            {
                Vec::new()
            }
        };

        let create_info = vk::InstanceCreateInfo {
            s_type: vk::StructureType::INSTANCE_CREATE_INFO,
            p_application_info: &app_info,
            enabled_layer_count: instance_layers.len() as u32,
            pp_enabled_layer_names: instance_layers.as_ptr(),
            enabled_extension_count: instance_extensions.len() as u32,
            pp_enabled_extension_names: instance_extensions.as_ptr(),
            ..Default::default()
        };

        let instance = unsafe {
            entry
                .create_instance(&create_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        let debug_messenger = if has_debug_utils && !instance_layers.is_empty() {
            let debug_utils_loader = ash::ext::debug_utils::Instance::new(entry, &instance);
            let debug_info = vk::DebugUtilsMessengerCreateInfoEXT {
                s_type: vk::StructureType::DEBUG_UTILS_MESSENGER_CREATE_INFO_EXT,
                message_severity: vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                    | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                message_type: vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                pfn_user_callback: Some(vulkan_debug_callback),
                ..Default::default()
            };
            let messenger =
                unsafe { debug_utils_loader.create_debug_utils_messenger(&debug_info, None) }.ok();
            messenger.map(|m| (debug_utils_loader, m))
        } else {
            None
        };

        let (physical_device, device_name, subgroup_size) = unsafe {
            let devices = instance
                .enumerate_physical_devices()
                .map_err(|e| GpckError::VulkanError(e.to_string()))?;
            if devices.is_empty() {
                return Err(GpckError::VulkanError(
                    "No Vulkan-compatible physical devices found".to_string(),
                ));
            }

            let mut selected = devices[0];
            let mut selected_name = String::from("Generic GPU");
            let mut selected_subgroup_size = 32u32;

            for dev in devices {
                let props = instance.get_physical_device_properties(dev);
                let mut subgroup_props = vk::PhysicalDeviceSubgroupProperties::default();
                let mut props2 = vk::PhysicalDeviceProperties2 {
                    p_next: &mut subgroup_props as *mut _ as *mut c_void,
                    ..Default::default()
                };
                instance.get_physical_device_properties2(dev, &mut props2);
                selected_subgroup_size = subgroup_props.subgroup_size.max(16);

                if props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU {
                    selected = dev;
                    selected_name = std::ffi::CStr::from_ptr(props.device_name.as_ptr())
                        .to_string_lossy()
                        .into_owned();
                    break;
                }
            }
            (selected, selected_name, selected_subgroup_size)
        };

        let queue_family_index = unsafe {
            let props = instance.get_physical_device_queue_family_properties(physical_device);
            props
                .iter()
                .enumerate()
                .position(|(_, p)| p.queue_flags.contains(vk::QueueFlags::COMPUTE))
                .ok_or_else(|| {
                    GpckError::VulkanError(
                        "No compute queue family found on selected GPU".to_string(),
                    )
                })? as u32
        };

        let queue_priorities = [1.0f32];
        let queue_create_info = vk::DeviceQueueCreateInfo {
            s_type: vk::StructureType::DEVICE_QUEUE_CREATE_INFO,
            queue_family_index,
            queue_count: 1,
            p_queue_priorities: queue_priorities.as_ptr(),
            ..Default::default()
        };

        let mut device_extensions = Vec::new();
        let available_device_extensions = unsafe {
            instance
                .enumerate_device_extension_properties(physical_device)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        if available_device_extensions.iter().any(|ext| {
            let name = unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) };
            name == c"VK_KHR_shader_non_semantic_info"
        }) {
            device_extensions.push(c"VK_KHR_shader_non_semantic_info".as_ptr());
        }

        if available_device_extensions.iter().any(|ext| {
            let name = unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) };
            name == c"VK_KHR_timeline_semaphore"
        }) {
            device_extensions.push(c"VK_KHR_timeline_semaphore".as_ptr());
        }

        let device_features = unsafe { instance.get_physical_device_features(physical_device) };
        let enabled_features = vk::PhysicalDeviceFeatures {
            shader_int64: device_features.shader_int64,
            ..Default::default()
        };

        let mut timeline_features = vk::PhysicalDeviceTimelineSemaphoreFeatures {
            s_type: vk::StructureType::PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES,
            timeline_semaphore: vk::TRUE,
            ..Default::default()
        };

        let mut variable_pointers_features = vk::PhysicalDeviceVariablePointersFeatures {
            s_type: vk::StructureType::PHYSICAL_DEVICE_VARIABLE_POINTERS_FEATURES,
            p_next: &mut timeline_features as *mut _ as *mut c_void,
            variable_pointers_storage_buffer: vk::TRUE,
            variable_pointers: vk::TRUE,
            ..Default::default()
        };

        let features2 = vk::PhysicalDeviceFeatures2 {
            s_type: vk::StructureType::PHYSICAL_DEVICE_FEATURES_2,
            p_next: &mut variable_pointers_features as *mut _ as *mut c_void,
            features: enabled_features,
            ..Default::default()
        };

        let device_create_info = vk::DeviceCreateInfo {
            s_type: vk::StructureType::DEVICE_CREATE_INFO,
            p_next: &features2 as *const _ as *const c_void,
            queue_create_info_count: 1,
            p_queue_create_infos: &queue_create_info,
            enabled_extension_count: device_extensions.len() as u32,
            pp_enabled_extension_names: device_extensions.as_ptr(),
            p_enabled_features: std::ptr::null(),
            ..Default::default()
        };

        let device = unsafe {
            instance
                .create_device(physical_device, &device_create_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };
        let compute_queue = unsafe { device.get_device_queue(queue_family_index, 0) };

        let mut type_info = vk::SemaphoreTypeCreateInfo {
            s_type: vk::StructureType::SEMAPHORE_TYPE_CREATE_INFO,
            semaphore_type: vk::SemaphoreType::TIMELINE,
            initial_value: 0,
            ..Default::default()
        };
        let create_info = vk::SemaphoreCreateInfo {
            s_type: vk::StructureType::SEMAPHORE_CREATE_INFO,
            p_next: &mut type_info as *mut _ as *mut c_void,
            ..Default::default()
        };
        let timeline_semaphore = unsafe {
            device
                .create_semaphore(&create_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        let pool_info = vk::CommandPoolCreateInfo {
            s_type: vk::StructureType::COMMAND_POOL_CREATE_INFO,
            flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            queue_family_index,
            ..Default::default()
        };
        let command_pool = unsafe {
            device
                .create_command_pool(&pool_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        // Descriptor Set Layouts
        let gdef_bindings = [
            vk::DescriptorSetLayoutBinding {
                binding: 0,
                descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 1,
                stage_flags: vk::ShaderStageFlags::COMPUTE,
                ..Default::default()
            },
            vk::DescriptorSetLayoutBinding {
                binding: 1,
                descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 1,
                stage_flags: vk::ShaderStageFlags::COMPUTE,
                ..Default::default()
            },
            vk::DescriptorSetLayoutBinding {
                binding: 2,
                descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 1,
                stage_flags: vk::ShaderStageFlags::COMPUTE,
                ..Default::default()
            },
            vk::DescriptorSetLayoutBinding {
                binding: 3,
                descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 1,
                stage_flags: vk::ShaderStageFlags::COMPUTE,
                ..Default::default()
            },
        ];
        let gdef_layout_info = vk::DescriptorSetLayoutCreateInfo {
            s_type: vk::StructureType::DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
            binding_count: 4,
            p_bindings: gdef_bindings.as_ptr(),
            ..Default::default()
        };
        let gdef_set_layout = unsafe {
            device
                .create_descriptor_set_layout(&gdef_layout_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        let unshuffle_bindings = [
            vk::DescriptorSetLayoutBinding {
                binding: 0,
                descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 1,
                stage_flags: vk::ShaderStageFlags::COMPUTE,
                ..Default::default()
            },
            vk::DescriptorSetLayoutBinding {
                binding: 1,
                descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 1,
                stage_flags: vk::ShaderStageFlags::COMPUTE,
                ..Default::default()
            },
        ];
        let unshuffle_layout_info = vk::DescriptorSetLayoutCreateInfo {
            s_type: vk::StructureType::DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
            binding_count: 2,
            p_bindings: unshuffle_bindings.as_ptr(),
            ..Default::default()
        };
        let unshuffle_set_layout = unsafe {
            device
                .create_descriptor_set_layout(&unshuffle_layout_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        // Load Compute Pipelines
        let gdeflate_pipeline =
            VulkanComputePipeline::create_from_shader(&device, gdef_set_layout, "GDeflate.spv")
                .ok();
        let zstd_pipeline =
            VulkanComputePipeline::create_from_shader(&device, gdef_set_layout, "Zstd.spv").ok();
        let brotlig_pipeline = VulkanComputePipeline::create_from_shader(
            &device,
            gdef_set_layout,
            "BrotliGCompute.spv",
        )
        .ok();

        let unshuffle_bc1x_pipeline = VulkanComputePipeline::create_from_shader(
            &device,
            unshuffle_set_layout,
            "UnshuffleBC1x.spv",
        )
        .ok();
        let unshuffle_bc2_pipeline = VulkanComputePipeline::create_from_shader(
            &device,
            unshuffle_set_layout,
            "UnshuffleBC2.spv",
        )
        .ok();
        let unshuffle_bc3x_pipeline = VulkanComputePipeline::create_from_shader(
            &device,
            unshuffle_set_layout,
            "UnshuffleBC3x.spv",
        )
        .ok();
        let unshuffle_bc4x_pipeline = VulkanComputePipeline::create_from_shader(
            &device,
            unshuffle_set_layout,
            "UnshuffleBC4x.spv",
        )
        .ok();
        let unshuffle_bc5x_pipeline = VulkanComputePipeline::create_from_shader(
            &device,
            unshuffle_set_layout,
            "UnshuffleBC5x.spv",
        )
        .ok();
        let unshuffle_bc6h_pipeline = VulkanComputePipeline::create_from_shader(
            &device,
            unshuffle_set_layout,
            "UnshuffleBC6h.spv",
        )
        .ok();
        let unshuffle_bc7_pipeline = VulkanComputePipeline::create_from_shader(
            &device,
            unshuffle_set_layout,
            "UnshuffleBC7.spv",
        )
        .ok();
        let unshuffle_curve_only_pipeline = VulkanComputePipeline::create_from_shader(
            &device,
            unshuffle_set_layout,
            "UnshuffleCurveOnly.spv",
        )
        .ok();

        let inst_clone = instance.clone();
        let phys_clone = physical_device;
        let find_mem = move |type_filter: u32, props: vk::MemoryPropertyFlags| -> GpckResult<u32> {
            let mem_properties =
                unsafe { inst_clone.get_physical_device_memory_properties(phys_clone) };
            for i in 0..mem_properties.memory_type_count {
                if (type_filter & (1 << i)) != 0
                    && (mem_properties.memory_types[i as usize].property_flags & props) == props
                {
                    return Ok(i);
                }
            }
            Err(GpckError::VulkanError(
                "Failed to find memory type".to_string(),
            ))
        };

        let zstd_engine = VulkanZstdGpuEngine::new(&device, subgroup_size, &find_mem).ok();
        let desc_pool_mgr = DescriptorPoolManager::new(&device, 512)?;

        Ok(Self {
            instance,
            physical_device,
            device,
            compute_queue: Mutex::new(compute_queue),
            _queue_family_index: queue_family_index,
            command_pool: Mutex::new(command_pool),
            timeline_semaphore,
            timeline_value: AtomicU64::new(0),
            gdeflate_pipeline,
            zstd_pipeline,
            brotlig_pipeline,
            unshuffle_bc1x_pipeline,
            unshuffle_bc2_pipeline,
            unshuffle_bc3x_pipeline,
            unshuffle_bc4x_pipeline,
            unshuffle_bc5x_pipeline,
            unshuffle_bc6h_pipeline,
            unshuffle_bc7_pipeline,
            unshuffle_curve_only_pipeline,
            zstd_engine,
            desc_pool_mgr: Mutex::new(desc_pool_mgr),
            gdef_set_layout,
            unshuffle_set_layout,
            debug_messenger,
            device_name,
            subgroup_size,
            worker_pool: Arc::new(BoundedWorkerPool::new(MAX_GPU_WORKERS)),
        })
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn subgroup_size(&self) -> u32 {
        self.subgroup_size
    }

    // ========================================================================
    // Decomposed Pipeline Recording Stages
    // ========================================================================

    unsafe fn record_upload_stage(
        &self,
        cmd: vk::CommandBuffer,
        worker: &WorkerContext,
        input_data: &[u8],
    ) {
        unsafe {
            std::ptr::copy_nonoverlapping(
                input_data.as_ptr(),
                worker.staging_in_mapped,
                input_data.len(),
            );

            let copy_in = [vk::BufferCopy {
                src_offset: 0,
                dst_offset: 0,
                size: input_data.len() as u64,
            }];
            self.device
                .cmd_copy_buffer(cmd, worker.staging_in_buf, worker.in_buf, &copy_in);

            let upload_barrier = vk::BufferMemoryBarrier {
                s_type: vk::StructureType::BUFFER_MEMORY_BARRIER,
                src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                dst_access_mask: vk::AccessFlags::SHADER_READ,
                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                buffer: worker.in_buf,
                offset: 0,
                size: input_data.len() as u64,
                ..Default::default()
            };
            self.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[upload_barrier],
                &[],
            );
        }
    }

    unsafe fn record_decompression_stage(
        &self,
        cmd: vk::CommandBuffer,
        worker: &WorkerContext,
        input_size: usize,
        target_size: usize,
        decomp_pipe: &VulkanComputePipeline,
    ) {
        unsafe {
            let control_data = [1u32, 0u32, 0u32, 0u32];
            std::ptr::copy_nonoverlapping(
                control_data.as_ptr() as *const u8,
                worker.scratch_mapped,
                16,
            );
            std::ptr::write_bytes(worker.scratch_mapped.add(16), 0, 16);

            let in_info = [vk::DescriptorBufferInfo {
                buffer: worker.in_buf,
                offset: 0,
                range: input_size as u64,
            }];
            let control_info = [vk::DescriptorBufferInfo {
                buffer: worker.scratch_buf,
                offset: 0,
                range: 16,
            }];
            let decomp_out_info = [vk::DescriptorBufferInfo {
                buffer: worker.inter_buf,
                offset: 0,
                range: target_size as u64,
            }];
            let scratch_info = [vk::DescriptorBufferInfo {
                buffer: worker.scratch_buf,
                offset: 16,
                range: 16,
            }];

            let gdef_writes = [
                vk::WriteDescriptorSet {
                    s_type: vk::StructureType::WRITE_DESCRIPTOR_SET,
                    dst_set: worker.gdef_set,
                    dst_binding: 0,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                    p_buffer_info: in_info.as_ptr(),
                    ..Default::default()
                },
                vk::WriteDescriptorSet {
                    s_type: vk::StructureType::WRITE_DESCRIPTOR_SET,
                    dst_set: worker.gdef_set,
                    dst_binding: 1,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                    p_buffer_info: control_info.as_ptr(),
                    ..Default::default()
                },
                vk::WriteDescriptorSet {
                    s_type: vk::StructureType::WRITE_DESCRIPTOR_SET,
                    dst_set: worker.gdef_set,
                    dst_binding: 2,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                    p_buffer_info: decomp_out_info.as_ptr(),
                    ..Default::default()
                },
                vk::WriteDescriptorSet {
                    s_type: vk::StructureType::WRITE_DESCRIPTOR_SET,
                    dst_set: worker.gdef_set,
                    dst_binding: 3,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                    p_buffer_info: scratch_info.as_ptr(),
                    ..Default::default()
                },
            ];
            self.device.update_descriptor_sets(&gdef_writes, &[]);

            self.device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                decomp_pipe.pipeline,
            );
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                decomp_pipe.pipeline_layout,
                0,
                &[worker.gdef_set],
                &[],
            );

            let dispatch_groups = (target_size as u32).div_ceil(64 * 1024).clamp(1, 128);
            self.device.cmd_dispatch(cmd, dispatch_groups, 1, 1);
        }
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn record_unshuffle_stage(
        &self,
        cmd: vk::CommandBuffer,
        worker: &WorkerContext,
        target_size: usize,
        transform: GaclTransform,
        width_pixels: usize,
        unshuffle_pipe: &VulkanComputePipeline,
        src_buffer: vk::Buffer,
    ) {
        unsafe {
            let unshuffle_src = [vk::DescriptorBufferInfo {
                buffer: src_buffer,
                offset: 0,
                range: target_size as u64,
            }];
            let unshuffle_dst = [vk::DescriptorBufferInfo {
                buffer: worker.out_buf,
                offset: 0,
                range: target_size as u64,
            }];

            let unshuffle_writes = [
                vk::WriteDescriptorSet {
                    s_type: vk::StructureType::WRITE_DESCRIPTOR_SET,
                    dst_set: worker.unshuffle_set,
                    dst_binding: 0,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                    p_buffer_info: unshuffle_src.as_ptr(),
                    ..Default::default()
                },
                vk::WriteDescriptorSet {
                    s_type: vk::StructureType::WRITE_DESCRIPTOR_SET,
                    dst_set: worker.unshuffle_set,
                    dst_binding: 1,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                    p_buffer_info: unshuffle_dst.as_ptr(),
                    ..Default::default()
                },
            ];
            self.device.update_descriptor_sets(&unshuffle_writes, &[]);

            let push_constants = GaclPushConstants {
                buffer_size_in_bytes: target_size as u32,
                buffer_offset_in_bytes: 0,
                transform_id: transform.to_u32(),
                width_in_pixels: width_pixels as u32,
            };

            self.device.cmd_push_constants(
                cmd,
                unshuffle_pipe.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                bytemuck::bytes_of(&push_constants),
            );
            self.device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                unshuffle_pipe.pipeline,
            );
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                unshuffle_pipe.pipeline_layout,
                0,
                &[worker.unshuffle_set],
                &[],
            );

            let block_size = transform.block_size();
            let total_blocks = (target_size / block_size) as u32;
            let unshuffle_groups = unshuffle_pipe
                .reflection
                .calculate_dispatch_groups_1d(total_blocks);

            self.device.cmd_dispatch(cmd, unshuffle_groups, 1, 1);
        }
    }

    unsafe fn record_buffer_transfer(
        &self,
        cmd: vk::CommandBuffer,
        src: vk::Buffer,
        dst: vk::Buffer,
        size: usize,
        src_stage: vk::PipelineStageFlags,
        src_access: vk::AccessFlags,
    ) {
        unsafe {
            let barrier = vk::BufferMemoryBarrier {
                s_type: vk::StructureType::BUFFER_MEMORY_BARRIER,
                src_access_mask: src_access,
                dst_access_mask: vk::AccessFlags::TRANSFER_READ,
                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                buffer: src,
                offset: 0,
                size: size as u64,
                ..Default::default()
            };
            self.device.cmd_pipeline_barrier(
                cmd,
                src_stage,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[barrier],
                &[],
            );

            let copy_region = [vk::BufferCopy {
                src_offset: 0,
                dst_offset: 0,
                size: size as u64,
            }];
            self.device.cmd_copy_buffer(cmd, src, dst, &copy_region);
        }
    }

    unsafe fn record_download_stage(
        &self,
        cmd: vk::CommandBuffer,
        worker: &WorkerContext,
        target_size: usize,
    ) {
        unsafe {
            let compute_to_transfer_barrier = vk::BufferMemoryBarrier {
                s_type: vk::StructureType::BUFFER_MEMORY_BARRIER,
                src_access_mask: vk::AccessFlags::SHADER_WRITE | vk::AccessFlags::TRANSFER_WRITE,
                dst_access_mask: vk::AccessFlags::TRANSFER_READ,
                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                buffer: worker.out_buf,
                offset: 0,
                size: target_size as u64,
                ..Default::default()
            };
            self.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[compute_to_transfer_barrier],
                &[],
            );

            let copy_out = [vk::BufferCopy {
                src_offset: 0,
                dst_offset: 0,
                size: target_size as u64,
            }];
            self.device
                .cmd_copy_buffer(cmd, worker.out_buf, worker.staging_out_buf, &copy_out);
        }
    }

    /// Records and submits pipeline commands without blocking the host CPU.
    fn record_and_submit(
        &self,
        worker: &mut WorkerContext,
        input_data: &[u8],
        target_size: usize,
        decomp_method: Option<CompressionMethod>,
        unshuffle_transform: Option<GaclTransform>,
        width_pixels: usize,
    ) -> GpckResult<u64> {
        let is_zstd = decomp_method == Some(CompressionMethod::Zstd);
        let decomp_pipe = self.resolve_decompression_pipeline(decomp_method);
        let unshuffle_pipe = self.resolve_unshuffle_pipeline(unshuffle_transform);

        unsafe {
            self.device
                .reset_command_buffer(worker.cmd, vk::CommandBufferResetFlags::empty())
                .map_err(|e| GpckError::VulkanError(e.to_string()))?;

            let begin_info = vk::CommandBufferBeginInfo {
                s_type: vk::StructureType::COMMAND_BUFFER_BEGIN_INFO,
                flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
                ..Default::default()
            };
            self.device
                .begin_command_buffer(worker.cmd, &begin_info)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?;

            // Stage 1: Upload input buffer
            self.record_upload_stage(worker.cmd, worker, input_data);

            // Stage 2: Decompression (Zstd ATG Multi-Pass or Compute Shader)
            let has_decomp = if is_zstd
                && let Some(ref atg_engine) = self.zstd_engine
                && atg_engine.is_ready
            {
                atg_engine.record_multi_pass_decompress(
                    &self.device,
                    worker.cmd,
                    worker.in_buf,
                    input_data.len(),
                    worker.inter_buf,
                    target_size,
                    self.subgroup_size,
                );
                true
            } else if let Some(pipe) = decomp_pipe {
                self.record_decompression_stage(
                    worker.cmd,
                    worker,
                    input_data.len(),
                    target_size,
                    pipe,
                );
                true
            } else {
                false
            };

            // Stage 3: Unshuffle or Buffer Route to output
            if let Some(pipe) = unshuffle_pipe {
                if has_decomp {
                    let barrier = vk::BufferMemoryBarrier {
                        s_type: vk::StructureType::BUFFER_MEMORY_BARRIER,
                        src_access_mask: vk::AccessFlags::SHADER_WRITE,
                        dst_access_mask: vk::AccessFlags::SHADER_READ,
                        src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                        dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                        buffer: worker.inter_buf,
                        offset: 0,
                        size: target_size as u64,
                        ..Default::default()
                    };
                    self.device.cmd_pipeline_barrier(
                        worker.cmd,
                        vk::PipelineStageFlags::COMPUTE_SHADER,
                        vk::PipelineStageFlags::COMPUTE_SHADER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[barrier],
                        &[],
                    );
                }

                let src_buf = if has_decomp {
                    worker.inter_buf
                } else {
                    worker.in_buf
                };
                let transform = unshuffle_transform.unwrap_or(GaclTransform::None);
                self.record_unshuffle_stage(
                    worker.cmd,
                    worker,
                    target_size,
                    transform,
                    width_pixels,
                    pipe,
                    src_buf,
                );
            } else if has_decomp {
                self.record_buffer_transfer(
                    worker.cmd,
                    worker.inter_buf,
                    worker.out_buf,
                    target_size,
                    vk::PipelineStageFlags::COMPUTE_SHADER,
                    vk::AccessFlags::SHADER_WRITE,
                );
            } else {
                self.record_buffer_transfer(
                    worker.cmd,
                    worker.in_buf,
                    worker.out_buf,
                    target_size,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::AccessFlags::TRANSFER_WRITE,
                );
            }

            // Stage 4: Readback Copy to Staging
            self.record_download_stage(worker.cmd, worker, target_size);

            self.device
                .end_command_buffer(worker.cmd)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?;

            // Stage 5: Timeline Semaphore Submission
            self.submit_to_compute_queue(worker)
        }
    }

    unsafe fn submit_to_compute_queue(&self, worker: &mut WorkerContext) -> GpckResult<u64> {
        let signal_val = self.timeline_value.fetch_add(1, Ordering::SeqCst) + 1;
        worker.sync_value = signal_val;

        let timeline_info = vk::TimelineSemaphoreSubmitInfo {
            s_type: vk::StructureType::TIMELINE_SEMAPHORE_SUBMIT_INFO,
            signal_semaphore_value_count: 1,
            p_signal_semaphore_values: &signal_val,
            ..Default::default()
        };

        let submit_info = vk::SubmitInfo {
            s_type: vk::StructureType::SUBMIT_INFO,
            p_next: &timeline_info as *const _ as *const c_void,
            command_buffer_count: 1,
            p_command_buffers: &worker.cmd,
            signal_semaphore_count: 1,
            p_signal_semaphores: &self.timeline_semaphore,
            ..Default::default()
        };

        unsafe {
            let queue_guard = self.compute_queue.lock().unwrap();
            self.device
                .queue_submit(*queue_guard, &[submit_info], vk::Fence::null())
                .map_err(|e| GpckError::VulkanError(e.to_string()))?;
        }

        Ok(signal_val)
    }

    #[inline(always)]
    fn resolve_decompression_pipeline(
        &self,
        method: Option<CompressionMethod>,
    ) -> Option<&VulkanComputePipeline> {
        match method {
            Some(CompressionMethod::Zstd) => self.zstd_pipeline.as_ref(),
            Some(CompressionMethod::GDeflate) => self.gdeflate_pipeline.as_ref(),
            Some(CompressionMethod::BrotliG) => self.brotlig_pipeline.as_ref(),
            _ => None,
        }
    }

    #[inline(always)]
    fn resolve_unshuffle_pipeline(
        &self,
        transform: Option<GaclTransform>,
    ) -> Option<&VulkanComputePipeline> {
        match transform {
            Some(
                GaclTransform::Bc1Linear
                | GaclTransform::Bc1LinearSpaceCurve
                | GaclTransform::Bc1V2BitInterleaved
                | GaclTransform::Bc1V2SpaceCurve,
            ) => self.unshuffle_bc1x_pipeline.as_ref(),
            Some(GaclTransform::Bc2AlphaNibble) => self.unshuffle_bc2_pipeline.as_ref(),
            Some(
                GaclTransform::Bc3Linear
                | GaclTransform::Bc3LinearSpaceCurve
                | GaclTransform::Bc3V2BitInterleaved
                | GaclTransform::Bc3V2SpaceCurve,
            ) => self.unshuffle_bc3x_pipeline.as_ref(),
            Some(GaclTransform::Bc4Linear | GaclTransform::Bc4LinearSpaceCurve) => {
                self.unshuffle_bc4x_pipeline.as_ref()
            }
            Some(GaclTransform::Bc5DualChannel | GaclTransform::Bc5SpaceCurve) => {
                self.unshuffle_bc5x_pipeline.as_ref()
            }
            Some(GaclTransform::Bc6hHeaderJoin) => self.unshuffle_bc6h_pipeline.as_ref(),
            Some(GaclTransform::Bc7ModeSplit | GaclTransform::Bc7ModeJoin) => {
                self.unshuffle_bc7_pipeline.as_ref()
            }
            Some(GaclTransform::CurveOnly16B) => self.unshuffle_curve_only_pipeline.as_ref(),
            _ => None,
        }
    }

    // ========================================================================
    // Execution API
    // ========================================================================

    pub async fn wait_timeline_async(
        &self,
        target_value: u64,
        timeout: Option<std::time::Duration>,
    ) -> GpckResult<()> {
        // Fast-path: Check if semaphore has already reached target value
        let current = unsafe {
            self.device
                .get_semaphore_counter_value(self.timeline_semaphore)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        if current >= target_value {
            return Ok(());
        }

        let timeout_duration = timeout.unwrap_or(std::time::Duration::from_secs(10));
        let timeout_ns = timeout_duration.as_nanos().min(u64::MAX as u128) as u64;

        let device = self.device.clone();
        let semaphore = self.timeline_semaphore;

        // Native OS/driver-level blocking wait offloaded to Tokio blocking pool
        tokio::task::spawn_blocking(move || {
            let wait_info = vk::SemaphoreWaitInfo {
                s_type: vk::StructureType::SEMAPHORE_WAIT_INFO,
                semaphore_count: 1,
                p_semaphores: &semaphore,
                p_values: &target_value,
                ..Default::default()
            };
            unsafe { device.wait_semaphores(&wait_info, timeout_ns) }
        })
        .await
        .map_err(|e| GpckError::VulkanError(format!("Vulkan timeline async join error: {}", e)))?
        .map_err(|e| {
            GpckError::VulkanError(format!("Vulkan timeline semaphore wait failed: {:?}", e))
        })
    }

    pub fn wait_timeline_sync(&self, target_value: u64, timeout_ms: u32) -> GpckResult<()> {
        let wait_info = vk::SemaphoreWaitInfo {
            s_type: vk::StructureType::SEMAPHORE_WAIT_INFO,
            semaphore_count: 1,
            p_semaphores: &self.timeline_semaphore,
            p_values: &target_value,
            ..Default::default()
        };
        unsafe {
            self.device
                .wait_semaphores(&wait_info, (timeout_ms as u64) * 1_000_000)
                .map_err(|e| {
                    GpckError::VulkanError(format!(
                        "Vulkan timeline semaphore wait failed: {:?}",
                        e
                    ))
                })
        }
    }

    fn acquire_worker(&self, required_capacity: usize) -> GpckResult<WorkerContext> {
        let mut workers_guard = self.worker_pool.workers.lock().unwrap();

        loop {
            let current_timeline = unsafe {
                self.device
                    .get_semaphore_counter_value(self.timeline_semaphore)
                    .unwrap_or(0)
            };

            if let Some(idx) = workers_guard
                .iter()
                .position(|w| w.sync_value <= current_timeline && w.capacity >= required_capacity)
            {
                return Ok(workers_guard.swap_remove(idx));
            }

            if let Some(idx) = workers_guard
                .iter()
                .position(|w| w.sync_value <= current_timeline)
            {
                let old_worker = workers_guard.swap_remove(idx);
                unsafe {
                    self.destroy_worker_resources(&old_worker);
                }
                drop(workers_guard);
                return self.create_worker(required_capacity);
            }

            let mut total_alloc = self.worker_pool.total_allocated.lock().unwrap();
            if *total_alloc < self.worker_pool.max_capacity {
                *total_alloc += 1;
                drop(total_alloc);
                drop(workers_guard);
                return self.create_worker(required_capacity);
            }
            drop(total_alloc);

            workers_guard = self.worker_pool.condvar.wait(workers_guard).unwrap();
        }
    }

    fn release_worker(&self, worker: WorkerContext) {
        let mut pool = self.worker_pool.workers.lock().unwrap();
        pool.push(worker);
        self.worker_pool.condvar.notify_one();
    }

    fn create_worker(&self, capacity: usize) -> GpckResult<WorkerContext> {
        let (in_buf, in_mem) = self.create_buffer(
            capacity as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        let (inter_buf, inter_mem) = self.create_buffer(
            capacity as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::TRANSFER_DST,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        let (out_buf, out_mem) = self.create_buffer(
            capacity as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::TRANSFER_DST,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        let (staging_in_buf, staging_in_mem) = self.create_buffer(
            capacity as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        let (staging_out_buf, staging_out_mem) = self.create_buffer(
            capacity as u64,
            vk::BufferUsageFlags::TRANSFER_DST,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        let (scratch_buf, scratch_mem) = self.create_buffer(
            64 * 1024,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        let staging_in_mapped = unsafe {
            self.device
                .map_memory(
                    staging_in_mem,
                    0,
                    capacity as u64,
                    vk::MemoryMapFlags::empty(),
                )
                .map_err(|e| GpckError::VulkanError(e.to_string()))? as *mut u8
        };
        let staging_out_mapped = unsafe {
            self.device
                .map_memory(
                    staging_out_mem,
                    0,
                    capacity as u64,
                    vk::MemoryMapFlags::empty(),
                )
                .map_err(|e| GpckError::VulkanError(e.to_string()))? as *mut u8
        };
        let scratch_mapped = unsafe {
            self.device
                .map_memory(scratch_mem, 0, 64 * 1024, vk::MemoryMapFlags::empty())
                .map_err(|e| GpckError::VulkanError(e.to_string()))? as *mut u8
        };

        let mut pool_mgr_guard = self.desc_pool_mgr.lock().unwrap();
        let (gdef_set, gdef_pool_owner) =
            pool_mgr_guard.allocate_set(&self.device, self.gdef_set_layout)?;
        let (unshuffle_set, unshuffle_pool_owner) =
            pool_mgr_guard.allocate_set(&self.device, self.unshuffle_set_layout)?;
        drop(pool_mgr_guard);

        let cmd_pool_guard = self.command_pool.lock().unwrap();
        let cmd_info = vk::CommandBufferAllocateInfo {
            s_type: vk::StructureType::COMMAND_BUFFER_ALLOCATE_INFO,
            command_pool: *cmd_pool_guard,
            level: vk::CommandBufferLevel::PRIMARY,
            command_buffer_count: 1,
            ..Default::default()
        };
        let cmd_bufs = unsafe {
            self.device
                .allocate_command_buffers(&cmd_info)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };
        let cmd = cmd_bufs[0];
        drop(cmd_pool_guard);

        Ok(WorkerContext {
            in_buf,
            in_mem,
            inter_buf,
            inter_mem,
            out_buf,
            out_mem,
            staging_in_buf,
            staging_in_mem,
            staging_in_mapped,
            staging_out_buf,
            staging_out_mem,
            staging_out_mapped,
            scratch_buf,
            scratch_mem,
            scratch_mapped,
            gdef_set,
            gdef_pool_owner,
            unshuffle_set,
            unshuffle_pool_owner,
            cmd,
            capacity,
            sync_value: 0,
        })
    }

    unsafe fn destroy_worker_resources(&self, w: &WorkerContext) {
        unsafe {
            let cmd_pool_guard = self.command_pool.lock().unwrap();
            self.device.free_command_buffers(*cmd_pool_guard, &[w.cmd]);
            drop(cmd_pool_guard);

            let pool_mgr_guard = self.desc_pool_mgr.lock().unwrap();
            pool_mgr_guard.free_set(&self.device, w.gdef_pool_owner, w.gdef_set);
            pool_mgr_guard.free_set(&self.device, w.unshuffle_pool_owner, w.unshuffle_set);
            drop(pool_mgr_guard);

            self.device.unmap_memory(w.staging_in_mem);
            self.device.unmap_memory(w.staging_out_mem);
            self.device.unmap_memory(w.scratch_mem);

            self.device.destroy_buffer(w.in_buf, None);
            self.device.free_memory(w.in_mem, None);
            self.device.destroy_buffer(w.inter_buf, None);
            self.device.free_memory(w.inter_mem, None);
            self.device.destroy_buffer(w.out_buf, None);
            self.device.free_memory(w.out_mem, None);

            self.device.destroy_buffer(w.staging_in_buf, None);
            self.device.free_memory(w.staging_in_mem, None);
            self.device.destroy_buffer(w.staging_out_buf, None);
            self.device.free_memory(w.staging_out_mem, None);
            self.device.destroy_buffer(w.scratch_buf, None);
            self.device.free_memory(w.scratch_mem, None);
        }
    }

    pub fn execute_pipeline_vram(
        &self,
        input_data: &[u8],
        target_size: usize,
        decomp_method: Option<CompressionMethod>,
        unshuffle_transform: Option<GaclTransform>,
        width_pixels: usize,
    ) -> GpckResult<()> {
        if input_data.is_empty() || target_size == 0 {
            return Ok(());
        }
        let required_capacity = (input_data.len().max(target_size))
            .next_power_of_two()
            .max(256 * 1024);
        let mut worker = self.acquire_worker(required_capacity)?;

        let signal_val = self.record_and_submit(
            &mut worker,
            input_data,
            target_size,
            decomp_method,
            unshuffle_transform,
            width_pixels,
        )?;

        let wait_res = self.wait_timeline_sync(signal_val, 10_000);
        self.release_worker(worker);
        wait_res
    }

    pub async fn execute_pipeline_vram_async(
        &self,
        input_data: &[u8],
        target_size: usize,
        decomp_method: Option<CompressionMethod>,
        unshuffle_transform: Option<GaclTransform>,
        width_pixels: usize,
    ) -> GpckResult<()> {
        if input_data.is_empty() || target_size == 0 {
            return Ok(());
        }
        let required_capacity = (input_data.len().max(target_size))
            .next_power_of_two()
            .max(256 * 1024);
        let mut worker = self.acquire_worker(required_capacity)?;

        let signal_val = self.record_and_submit(
            &mut worker,
            input_data,
            target_size,
            decomp_method,
            unshuffle_transform,
            width_pixels,
        )?;

        let wait_res = self.wait_timeline_async(signal_val, None).await;
        self.release_worker(worker);
        wait_res
    }

    pub fn unshuffle_to_vram(
        &self,
        input: &[u8],
        target_size: usize,
        transform: GaclTransform,
        width_pixels: usize,
    ) -> GpckResult<()> {
        self.execute_pipeline_vram(input, target_size, None, Some(transform), width_pixels)
    }

    pub async fn unshuffle_to_vram_async(
        &self,
        input: &[u8],
        target_size: usize,
        transform: GaclTransform,
        width_pixels: usize,
    ) -> GpckResult<()> {
        self.execute_pipeline_vram_async(input, target_size, None, Some(transform), width_pixels)
            .await
    }

    pub fn decompress_to_vram(
        &self,
        compressed: &[u8],
        target_size: usize,
        method: CompressionMethod,
    ) -> GpckResult<()> {
        self.execute_pipeline_vram(compressed, target_size, Some(method), None, 0)
    }

    pub async fn decompress_to_vram_async(
        &self,
        compressed: &[u8],
        target_size: usize,
        method: CompressionMethod,
    ) -> GpckResult<()> {
        self.execute_pipeline_vram_async(compressed, target_size, Some(method), None, 0)
            .await
    }

    pub fn decompress(
        &self,
        compressed: &[u8],
        target_size: usize,
        method: CompressionMethod,
    ) -> GpckResult<Vec<u8>> {
        self.decompress_and_unshuffle(compressed, target_size, method, GaclTransform::None, 0)
    }

    pub async fn decompress_async(
        &self,
        compressed: &[u8],
        target_size: usize,
        method: CompressionMethod,
    ) -> GpckResult<Vec<u8>> {
        self.decompress_and_unshuffle_async(compressed, target_size, method, GaclTransform::None, 0)
            .await
    }

    pub fn decompress_and_unshuffle(
        &self,
        compressed: &[u8],
        target_size: usize,
        method: CompressionMethod,
        transform: GaclTransform,
        width_pixels: usize,
    ) -> GpckResult<Vec<u8>> {
        if compressed.is_empty() || target_size == 0 {
            return Ok(Vec::new());
        }

        let required_capacity = (compressed.len().max(target_size))
            .next_power_of_two()
            .max(256 * 1024);
        let mut worker = self.acquire_worker(required_capacity)?;

        let signal_val = self.record_and_submit(
            &mut worker,
            compressed,
            target_size,
            Some(method),
            Some(transform),
            width_pixels,
        )?;

        self.wait_timeline_sync(signal_val, 10_000)?;

        let mut output = vec![0u8; target_size];
        unsafe {
            std::ptr::copy_nonoverlapping(
                worker.staging_out_mapped,
                output.as_mut_ptr(),
                target_size,
            );
        }

        self.release_worker(worker);
        Ok(output)
    }

    pub async fn decompress_and_unshuffle_async(
        &self,
        compressed: &[u8],
        target_size: usize,
        method: CompressionMethod,
        transform: GaclTransform,
        width_pixels: usize,
    ) -> GpckResult<Vec<u8>> {
        if compressed.is_empty() || target_size == 0 {
            return Ok(Vec::new());
        }

        let required_capacity = (compressed.len().max(target_size))
            .next_power_of_two()
            .max(256 * 1024);
        let mut worker = self.acquire_worker(required_capacity)?;

        let signal_val = self.record_and_submit(
            &mut worker,
            compressed,
            target_size,
            Some(method),
            Some(transform),
            width_pixels,
        )?;

        let wait_res = self.wait_timeline_async(signal_val, None).await;
        if let Err(e) = wait_res {
            self.release_worker(worker);
            return Err(e);
        }

        let mut output = vec![0u8; target_size];
        unsafe {
            std::ptr::copy_nonoverlapping(
                worker.staging_out_mapped,
                output.as_mut_ptr(),
                target_size,
            );
        }

        self.release_worker(worker);
        Ok(output)
    }

    fn create_buffer(
        &self,
        size: u64,
        usage: vk::BufferUsageFlags,
        properties: vk::MemoryPropertyFlags,
    ) -> GpckResult<(vk::Buffer, vk::DeviceMemory)> {
        let buffer_info = vk::BufferCreateInfo {
            s_type: vk::StructureType::BUFFER_CREATE_INFO,
            size,
            usage,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            ..Default::default()
        };

        let buffer = unsafe {
            self.device
                .create_buffer(&buffer_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };
        let mem_requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let memory_type_index =
            self.find_memory_type(mem_requirements.memory_type_bits, properties)?;

        let alloc_info = vk::MemoryAllocateInfo {
            s_type: vk::StructureType::MEMORY_ALLOCATE_INFO,
            allocation_size: mem_requirements.size,
            memory_type_index,
            ..Default::default()
        };

        let memory = unsafe {
            self.device
                .allocate_memory(&alloc_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };
        unsafe {
            self.device
                .bind_buffer_memory(buffer, memory, 0)
                .map_err(|e| GpckError::VulkanError(e.to_string()))?
        };

        Ok((buffer, memory))
    }

    fn find_memory_type(
        &self,
        type_filter: u32,
        properties: vk::MemoryPropertyFlags,
    ) -> GpckResult<u32> {
        let mem_properties = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.physical_device)
        };
        for i in 0..mem_properties.memory_type_count {
            if (type_filter & (1 << i)) != 0
                && (mem_properties.memory_types[i as usize].property_flags & properties)
                    == properties
            {
                return Ok(i);
            }
        }
        Err(GpckError::VulkanError(
            "Failed to find suitable Vulkan memory type".to_string(),
        ))
    }
}

unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _message_types: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut c_void,
) -> vk::Bool32 {
    unsafe {
        if p_callback_data.is_null() || (*p_callback_data).p_message.is_null() {
            return vk::FALSE;
        }
        let message = CStr::from_ptr((*p_callback_data).p_message).to_string_lossy();

        if message.contains("DEBUG-PRINTF") || message.contains("UNASSIGNED-DEBUG-PRINTF") {
            crate::core::logger::log_info(&format!("[GPU Printf] {}", message));
        } else if message_severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
            crate::core::logger::log_error(&format!("[Vulkan Error] {}", message));
        } else if message_severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
            crate::core::logger::log_warn(&format!("[Vulkan Warning] {}", message));
        }
    }
    vk::FALSE
}

impl GpuStreamingBackend for VulkanDecompressor {
    fn name(&self) -> &str {
        &self.device_name
    }

    fn is_hardware_accelerated(&self) -> bool {
        true
    }

    fn decompress(
        &self,
        compressed: &[u8],
        target_size: usize,
        method: CompressionMethod,
    ) -> GpckResult<Vec<u8>> {
        VulkanDecompressor::decompress(self, compressed, target_size, method)
    }

    fn decompress_and_unshuffle(
        &self,
        compressed: &[u8],
        target_size: usize,
        method: CompressionMethod,
        transform: GaclTransform,
        width_pixels: usize,
    ) -> GpckResult<Vec<u8>> {
        VulkanDecompressor::decompress_and_unshuffle(
            self,
            compressed,
            target_size,
            method,
            transform,
            width_pixels,
        )
    }
}

impl Drop for VulkanDecompressor {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();

            let mut pool = self.worker_pool.workers.lock().unwrap();
            for w in pool.drain(..) {
                self.destroy_worker_resources(&w);
            }

            if let Some(ref atg) = self.zstd_engine {
                atg.destroy(&self.device);
            }

            if let Some(p) = self.gdeflate_pipeline.take() {
                p.destroy(&self.device);
            }
            if let Some(p) = self.zstd_pipeline.take() {
                p.destroy(&self.device);
            }
            if let Some(p) = self.brotlig_pipeline.take() {
                p.destroy(&self.device);
            }
            if let Some(p) = self.unshuffle_bc1x_pipeline.take() {
                p.destroy(&self.device);
            }
            if let Some(p) = self.unshuffle_bc2_pipeline.take() {
                p.destroy(&self.device);
            }
            if let Some(p) = self.unshuffle_bc3x_pipeline.take() {
                p.destroy(&self.device);
            }
            if let Some(p) = self.unshuffle_bc4x_pipeline.take() {
                p.destroy(&self.device);
            }
            if let Some(p) = self.unshuffle_bc5x_pipeline.take() {
                p.destroy(&self.device);
            }
            if let Some(p) = self.unshuffle_bc6h_pipeline.take() {
                p.destroy(&self.device);
            }
            if let Some(p) = self.unshuffle_bc7_pipeline.take() {
                p.destroy(&self.device);
            }
            if let Some(p) = self.unshuffle_curve_only_pipeline.take() {
                p.destroy(&self.device);
            }

            self.device.destroy_semaphore(self.timeline_semaphore, None);

            let mut pool_mgr_guard = self.desc_pool_mgr.lock().unwrap();
            pool_mgr_guard.destroy_all(&self.device);
            drop(pool_mgr_guard);

            self.device
                .destroy_descriptor_set_layout(self.gdef_set_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.unshuffle_set_layout, None);

            let cmd_pool_guard = self.command_pool.lock().unwrap();
            self.device.destroy_command_pool(*cmd_pool_guard, None);

            self.device.destroy_device(None);

            if let Some((loader, messenger)) = self.debug_messenger.take() {
                loader.destroy_debug_utils_messenger(messenger, None);
            }

            self.instance.destroy_instance(None);
        }
    }
}
