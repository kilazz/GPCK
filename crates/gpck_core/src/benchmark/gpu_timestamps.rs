// src/benchmark/gpu_timestamps.rs
//! # Hardware GPU Timestamp Query Infrastructure
//!
//! Measures exact GPU silicon execution time in nanoseconds/microseconds
//! bypassing CPU driver submission latency and OS scheduler jitter.

use ash::vk;

pub struct VulkanGpuTimer {
    device: ash::Device,
    query_pool: vk::QueryPool,
    timestamp_period_ns: f32,
}

impl VulkanGpuTimer {
    pub fn new(
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        device: &ash::Device,
    ) -> Option<Self> {
        let props = unsafe { instance.get_physical_device_properties(physical_device) };
        let timestamp_period_ns = props.limits.timestamp_period;
        if timestamp_period_ns <= 0.0 {
            return None;
        }

        let pool_info = vk::QueryPoolCreateInfo {
            s_type: vk::StructureType::QUERY_POOL_CREATE_INFO,
            query_type: vk::QueryType::TIMESTAMP,
            query_count: 2,
            ..Default::default()
        };

        let query_pool = unsafe { device.create_query_pool(&pool_info, None) }.ok()?;

        Some(Self {
            device: device.clone(),
            query_pool,
            timestamp_period_ns,
        })
    }

    #[inline(always)]
    pub fn record_start(&self, cmd: vk::CommandBuffer) {
        unsafe {
            self.device.cmd_reset_query_pool(cmd, self.query_pool, 0, 2);
            self.device.cmd_write_timestamp(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                self.query_pool,
                0,
            );
        }
    }

    #[inline(always)]
    pub fn record_end(&self, cmd: vk::CommandBuffer) {
        unsafe {
            self.device.cmd_write_timestamp(
                cmd,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                self.query_pool,
                1,
            );
        }
    }

    pub fn get_elapsed_ms(&self) -> Option<f64> {
        let mut results = [0u64; 2];
        unsafe {
            let res = self.device.get_query_pool_results(
                self.query_pool,
                0,
                &mut results,
                vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
            );

            if res.is_ok() {
                let delta_ticks = results[1].saturating_sub(results[0]);
                let ns = (delta_ticks as f64) * (self.timestamp_period_ns as f64);
                Some(ns / 1_000_000.0)
            } else {
                None
            }
        }
    }
}

impl Drop for VulkanGpuTimer {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_query_pool(self.query_pool, None);
        }
    }
}
