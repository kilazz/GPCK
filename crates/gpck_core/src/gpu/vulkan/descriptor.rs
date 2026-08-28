// crates/gpck_core/src/gpu/vulkan/descriptor.rs
//! # Vulkan Descriptor Pool & Set Lifecycle Manager

use crate::core::error::{GpckError, GpckResult};
use ash::vk;

pub struct DescriptorPoolManager {
    pools: Vec<vk::DescriptorPool>,
    max_sets_per_pool: u32,
}

impl DescriptorPoolManager {
    pub fn new(device: &ash::Device, max_sets_per_pool: u32) -> GpckResult<Self> {
        let initial_pool = Self::create_pool(device, max_sets_per_pool)?;
        Ok(Self {
            pools: vec![initial_pool],
            max_sets_per_pool,
        })
    }

    fn create_pool(device: &ash::Device, max_sets: u32) -> GpckResult<vk::DescriptorPool> {
        let pool_sizes = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::STORAGE_BUFFER,
            descriptor_count: max_sets * 4,
        }];

        let pool_info = vk::DescriptorPoolCreateInfo {
            s_type: vk::StructureType::DESCRIPTOR_POOL_CREATE_INFO,
            flags: vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET,
            max_sets,
            pool_size_count: pool_sizes.len() as u32,
            p_pool_sizes: pool_sizes.as_ptr(),
            ..Default::default()
        };

        unsafe {
            device
                .create_descriptor_pool(&pool_info, None)
                .map_err(|e| GpckError::VulkanError(e.to_string()))
        }
    }

    pub fn allocate_set(
        &mut self,
        device: &ash::Device,
        layout: vk::DescriptorSetLayout,
    ) -> GpckResult<(vk::DescriptorSet, vk::DescriptorPool)> {
        let layouts = [layout];
        let alloc_info = vk::DescriptorSetAllocateInfo {
            s_type: vk::StructureType::DESCRIPTOR_SET_ALLOCATE_INFO,
            descriptor_pool: *self.pools.last().unwrap(),
            descriptor_set_count: 1,
            p_set_layouts: layouts.as_ptr(),
            ..Default::default()
        };

        match unsafe { device.allocate_descriptor_sets(&alloc_info) } {
            Ok(sets) => Ok((sets[0], *self.pools.last().unwrap())),
            Err(vk::Result::ERROR_OUT_OF_POOL_MEMORY | vk::Result::ERROR_FRAGMENTED_POOL) => {
                let new_pool = Self::create_pool(device, self.max_sets_per_pool)?;
                self.pools.push(new_pool);

                let retry_alloc = vk::DescriptorSetAllocateInfo {
                    s_type: vk::StructureType::DESCRIPTOR_SET_ALLOCATE_INFO,
                    descriptor_pool: new_pool,
                    descriptor_set_count: 1,
                    p_set_layouts: layouts.as_ptr(),
                    ..Default::default()
                };

                let sets = unsafe {
                    device
                        .allocate_descriptor_sets(&retry_alloc)
                        .map_err(|e| GpckError::VulkanError(e.to_string()))?
                };
                Ok((sets[0], new_pool))
            }
            Err(e) => Err(GpckError::VulkanError(format!(
                "Failed to allocate descriptor set: {:?}",
                e
            ))),
        }
    }

    pub fn free_set(&self, device: &ash::Device, pool: vk::DescriptorPool, set: vk::DescriptorSet) {
        unsafe {
            let _ = device.free_descriptor_sets(pool, &[set]);
        }
    }

    pub fn destroy_all(&mut self, device: &ash::Device) {
        for &pool in &self.pools {
            unsafe {
                device.destroy_descriptor_pool(pool, None);
            }
        }
        self.pools.clear();
    }
}
