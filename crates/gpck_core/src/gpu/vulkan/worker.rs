// crates/gpck_core/src/gpu/vulkan/worker.rs
//! # Vulkan Ring-Buffer Worker Context & Pool Management

use ash::vk;
use std::sync::{Condvar, Mutex};

pub const MAX_GPU_WORKERS: usize = 4;

pub struct WorkerContext {
    pub in_buf: vk::Buffer,
    pub in_mem: vk::DeviceMemory,
    pub inter_buf: vk::Buffer,
    pub inter_mem: vk::DeviceMemory,
    pub out_buf: vk::Buffer,
    pub out_mem: vk::DeviceMemory,

    pub staging_in_buf: vk::Buffer,
    pub staging_in_mem: vk::DeviceMemory,
    pub staging_in_mapped: *mut u8,

    pub staging_out_buf: vk::Buffer,
    pub staging_out_mem: vk::DeviceMemory,
    pub staging_out_mapped: *mut u8,

    pub scratch_buf: vk::Buffer,
    pub scratch_mem: vk::DeviceMemory,
    pub scratch_mapped: *mut u8,

    pub gdef_set: vk::DescriptorSet,
    pub gdef_pool_owner: vk::DescriptorPool,
    pub unshuffle_set: vk::DescriptorSet,
    pub unshuffle_pool_owner: vk::DescriptorPool,

    pub cmd: vk::CommandBuffer,
    pub capacity: usize,
    pub sync_value: u64,
}

unsafe impl Send for WorkerContext {}
unsafe impl Sync for WorkerContext {}

pub struct BoundedWorkerPool {
    pub workers: Mutex<Vec<WorkerContext>>,
    pub condvar: Condvar,
    pub total_allocated: Mutex<usize>,
    pub max_capacity: usize,
}

impl BoundedWorkerPool {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            workers: Mutex::new(Vec::with_capacity(max_capacity)),
            condvar: Condvar::new(),
            total_allocated: Mutex::new(0),
            max_capacity,
        }
    }
}
