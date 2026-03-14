using Silk.NET.Vulkan;
using System.Runtime.InteropServices;
using Buffer = Silk.NET.Vulkan.Buffer;

namespace GPCK.Core.Vulkan
{
    /// <summary>
    /// Unified Cross-Platform Decompressor.
    /// Works on Windows (DirectX replacement) and Linux (Steam Deck native).
    /// Uses Vulkan Compute Shaders to run GDeflate/Copy operations.
    /// </summary>
    public unsafe class VulkanDecompressor : IDisposable
    {
        private readonly Vk _vk;
        private Instance _instance;
        private PhysicalDevice _physicalDevice;
        private Device _device;
        private Queue _computeQueue;
        private uint _queueFamilyIndex;

        private CommandPool _commandPool;
        private DescriptorSetLayout _descriptorSetLayout;
        private PipelineLayout _pipelineLayout;
        private Pipeline _pipeline;

        private DescriptorPool _descriptorPool;

        public bool IsInitialized { get; private set; }
        public string DeviceName { get; private set; } = "Unknown";

        // Memory heaps
        private uint _hostVisibleMemoryIndex;
        private uint _deviceLocalMemoryIndex;

        public VulkanDecompressor()
        {
            _vk = Vk.GetApi();
            InitVulkan();
            InitPipeline();
            IsInitialized = true;
        }

        private void InitVulkan()
        {
            // 1. Instance
            var appName = (byte*)Marshal.StringToHGlobalAnsi("GPCK Unified");
            var appInfo = new ApplicationInfo
            {
                SType = StructureType.ApplicationInfo,
                PApplicationName = appName,
                ApiVersion = Vk.Version12
            };

            // Enable Validation Layers for debug (optional)
            var layerName = (byte*)Marshal.StringToHGlobalAnsi("VK_LAYER_KHRONOS_validation");
            byte** layers = stackalloc byte*[1];
            layers[0] = layerName;

            var createInfo = new InstanceCreateInfo
            {
                SType = StructureType.InstanceCreateInfo,
                PApplicationInfo = &appInfo,
                // EnabledLayerCount = 1, // Uncomment for debug
                // PpEnabledLayerNames = layers
            };

            fixed (Instance* pInstance = &_instance)
            {
                if (_vk.CreateInstance(&createInfo, null, pInstance) != Result.Success)
                    throw new Exception("Failed to create Vulkan Instance");
            }
            Marshal.FreeHGlobal((IntPtr)appName);
            // Marshal.FreeHGlobal((IntPtr)layerName);

            // 2. Physical Device
            PickPhysicalDevice();

            // 3. Queue Family
            FindComputeQueue();

            // 4. Logical Device
            float queuePriority = 1.0f;
            var queueCreateInfo = new DeviceQueueCreateInfo
            {
                SType = StructureType.DeviceQueueCreateInfo,
                QueueFamilyIndex = _queueFamilyIndex,
                QueueCount = 1,
                PQueuePriorities = &queuePriority
            };

            var deviceCreateInfo = new DeviceCreateInfo
            {
                SType = StructureType.DeviceCreateInfo,
                PQueueCreateInfos = &queueCreateInfo,
                QueueCreateInfoCount = 1
            };

            fixed (Device* pDevice = &_device)
            {
                if (_vk.CreateDevice(_physicalDevice, &deviceCreateInfo, null, pDevice) != Result.Success)
                    throw new Exception("Failed to create Logical Device");
            }

            _vk.GetDeviceQueue(_device, _queueFamilyIndex, 0, out _computeQueue);

            // 5. Find Memory Types
            FindMemoryTypes();

            // 6. Command Pool
            var poolInfo = new CommandPoolCreateInfo
            {
                SType = StructureType.CommandPoolCreateInfo,
                QueueFamilyIndex = _queueFamilyIndex,
                Flags = CommandPoolCreateFlags.ResetCommandBufferBit
            };

            fixed (CommandPool* pPool = &_commandPool)
            {
                _vk.CreateCommandPool(_device, &poolInfo, null, pPool);
            }
        }

        private void PickPhysicalDevice()
        {
            uint count = 0;
            _vk.EnumeratePhysicalDevices(_instance, &count, null);
            var devices = new PhysicalDevice[count];
            fixed (PhysicalDevice* pDevices = devices) _vk.EnumeratePhysicalDevices(_instance, &count, pDevices);

            // Prefer Discrete GPU
            foreach (var dev in devices)
            {
                PhysicalDeviceProperties props;
                _vk.GetPhysicalDeviceProperties(dev, &props);
                if (props.DeviceType == PhysicalDeviceType.DiscreteGpu)
                {
                    _physicalDevice = dev;
                    DeviceName = Marshal.PtrToStringAnsi((IntPtr)props.DeviceName) ?? "Discrete GPU";
                    return;
                }
            }

            // Fallback
            _physicalDevice = devices[0];
            PhysicalDeviceProperties p;
            _vk.GetPhysicalDeviceProperties(_physicalDevice, &p);
            DeviceName = Marshal.PtrToStringAnsi((IntPtr)p.DeviceName) ?? "Integrated GPU";
        }

        private void FindComputeQueue()
        {
            uint count = 0;
            _vk.GetPhysicalDeviceQueueFamilyProperties(_physicalDevice, &count, null);
            var props = new QueueFamilyProperties[count];
            fixed (QueueFamilyProperties* pProps = props) _vk.GetPhysicalDeviceQueueFamilyProperties(_physicalDevice, &count, pProps);

            for (uint i = 0; i < count; i++)
            {
                if ((props[i].QueueFlags & QueueFlags.ComputeBit) != 0)
                {
                    _queueFamilyIndex = i;
                    return;
                }
            }
            throw new Exception("No compute queue found");
        }

        private void FindMemoryTypes()
        {
            PhysicalDeviceMemoryProperties memProps;
            _vk.GetPhysicalDeviceMemoryProperties(_physicalDevice, &memProps);

            // Find Host Visible (for CPU upload)
            for (uint i = 0; i < memProps.MemoryTypeCount; i++)
            {
                if ((memProps.MemoryTypes[(int)i].PropertyFlags & MemoryPropertyFlags.HostVisibleBit) != 0 &&
                    (memProps.MemoryTypes[(int)i].PropertyFlags & MemoryPropertyFlags.HostCoherentBit) != 0)
                {
                    _hostVisibleMemoryIndex = i;
                    break;
                }
            }

            // Find Device Local (for VRAM)
            for (uint i = 0; i < memProps.MemoryTypeCount; i++)
            {
                if ((memProps.MemoryTypes[(int)i].PropertyFlags & MemoryPropertyFlags.DeviceLocalBit) != 0)
                {
                    _deviceLocalMemoryIndex = i;
                    break;
                }
            }
        }

        private void InitPipeline()
        {
            // 1. Descriptor Set Layout
            // Binding 0: Input Buffer (ReadOnly SSBO)
            // Binding 1: Output Buffer (WriteOnly SSBO)
            var bindings = stackalloc DescriptorSetLayoutBinding[2];
            bindings[0] = new DescriptorSetLayoutBinding
            {
                Binding = 0,
                DescriptorType = DescriptorType.StorageBuffer,
                DescriptorCount = 1,
                StageFlags = ShaderStageFlags.ComputeBit
            };
            bindings[1] = new DescriptorSetLayoutBinding
            {
                Binding = 1,
                DescriptorType = DescriptorType.StorageBuffer,
                DescriptorCount = 1,
                StageFlags = ShaderStageFlags.ComputeBit
            };

            var layoutInfo = new DescriptorSetLayoutCreateInfo
            {
                SType = StructureType.DescriptorSetLayoutCreateInfo,
                BindingCount = 2,
                PBindings = bindings
            };

            fixed (DescriptorSetLayout* pLayout = &_descriptorSetLayout)
            {
                _vk.CreateDescriptorSetLayout(_device, &layoutInfo, null, pLayout);
            }

            // 2. Pipeline Layout
            var pushConstantRange = new PushConstantRange
            {
                StageFlags = ShaderStageFlags.ComputeBit,
                Offset = 0,
                Size = 8 // 2 uints
            };

            fixed (DescriptorSetLayout* pSetLayout = &_descriptorSetLayout)
            {
                var pipeLayoutInfo = new PipelineLayoutCreateInfo
                {
                    SType = StructureType.PipelineLayoutCreateInfo,
                    SetLayoutCount = 1,
                    PSetLayouts = pSetLayout,
                    PushConstantRangeCount = 1,
                    PPushConstantRanges = &pushConstantRange
                };

                fixed (PipelineLayout* pPipeLayout = &_pipelineLayout)
                {
                    _vk.CreatePipelineLayout(_device, &pipeLayoutInfo, null, pPipeLayout);
                }
            }

            // 3. Shader Module (Dummy Passthrough for now)
            // In a real app, load "gdeflate.spv" here.
            // This represents a compiled minimal GLSL compute shader.
            var shaderCode = GetDummyComputeShaderBytes();

            ShaderModule shaderModule;
            fixed (byte* pCode = shaderCode)
            {
                var shaderInfo = new ShaderModuleCreateInfo
                {
                    SType = StructureType.ShaderModuleCreateInfo,
                    CodeSize = (nuint)shaderCode.Length,
                    PCode = (uint*)pCode
                };

                // ShaderModule is a struct on the stack, no need to pin.
                if (_vk.CreateShaderModule(_device, &shaderInfo, null, &shaderModule) != Result.Success)
                {
                    throw new Exception("Failed to create shader module");
                }
            }

            // 4. Compute Pipeline
            var mainStr = (byte*)Marshal.StringToHGlobalAnsi("main");
            var stageInfo = new PipelineShaderStageCreateInfo
            {
                SType = StructureType.PipelineShaderStageCreateInfo,
                Stage = ShaderStageFlags.ComputeBit,
                Module = shaderModule,
                PName = mainStr
            };

            var pipelineInfo = new ComputePipelineCreateInfo
            {
                SType = StructureType.ComputePipelineCreateInfo,
                Stage = stageInfo,
                Layout = _pipelineLayout
            };

            fixed (Pipeline* pPipeline = &_pipeline)
            {
                _vk.CreateComputePipelines(_device, default, 1, &pipelineInfo, null, pPipeline);
            }

            _vk.DestroyShaderModule(_device, shaderModule, null);
            Marshal.FreeHGlobal((IntPtr)mainStr);

            // 5. Descriptor Pool
            var poolSize = new DescriptorPoolSize { Type = DescriptorType.StorageBuffer, DescriptorCount = 100 };
            var poolInfo = new DescriptorPoolCreateInfo
            {
                SType = StructureType.DescriptorPoolCreateInfo,
                PoolSizeCount = 1,
                PPoolSizes = &poolSize,
                MaxSets = 50
            };
            fixed (DescriptorPool* pPool = &_descriptorPool)
                _vk.CreateDescriptorPool(_device, &poolInfo, null, pPool);
        }

        // Mock SPIR-V for demonstration. In real life, use `glslc shader.comp -o shader.spv`
        private byte[] GetDummyComputeShaderBytes()
        {
            // This is just an empty placeholder.
            // The user needs to compile the provided GLSL to SPIR-V and load it.
            // For now, we return 4 bytes so it doesn't crash allocation logic immediately,
            // BUT calling CreateShaderModule on this will fail validation layers if checked.
            // In a real run, load from file: File.ReadAllBytes("gdeflate.spv");
            try
            {
                if (File.Exists("gdeflate.spv")) return File.ReadAllBytes("gdeflate.spv");
            }
            catch { }
            return new byte[32]; // invalid
        }

        /// <summary>
        /// Executes the decompression pipeline.
        /// Architecture: CPU RAM -> Staging Buffer (Host) -> GPU Buffer (VRAM) -> Compute -> Staging -> CPU RAM.
        /// On Steam Deck (UMA), this can be optimized to Zero-Copy by using HostVisible | DeviceLocal memory.
        /// </summary>
        public void Decompress(ReadOnlySpan<byte> compressedData, Span<byte> outputData)
        {
            if (compressedData.Length == 0) return;

            // 1. Allocate buffers
            CreateBuffer((ulong)compressedData.Length, BufferUsageFlags.StorageBufferBit | BufferUsageFlags.TransferSrcBit | BufferUsageFlags.TransferDstBit,
                MemoryPropertyFlags.HostVisibleBit | MemoryPropertyFlags.HostCoherentBit, out var inBuf, out var inMem);

            CreateBuffer((ulong)outputData.Length, BufferUsageFlags.StorageBufferBit | BufferUsageFlags.TransferSrcBit | BufferUsageFlags.TransferDstBit,
                MemoryPropertyFlags.HostVisibleBit | MemoryPropertyFlags.HostCoherentBit, out var outBuf, out var outMem);

            // 2. Upload Data (Map Memory)
            void* pData;
            _vk.MapMemory(_device, inMem, 0, (ulong)compressedData.Length, 0, &pData);
            fixed (byte* ptr = compressedData)
            {
                System.Buffer.MemoryCopy(ptr, pData, compressedData.Length, compressedData.Length);
            }
            _vk.UnmapMemory(_device, inMem);

            // 3. Allocate Descriptor Set
            DescriptorSet set;
            var layout = _descriptorSetLayout;
            var allocInfo = new DescriptorSetAllocateInfo
            {
                SType = StructureType.DescriptorSetAllocateInfo,
                DescriptorPool = _descriptorPool,
                DescriptorSetCount = 1,
                PSetLayouts = &layout
            };
            _vk.AllocateDescriptorSets(_device, &allocInfo, &set);

            // 4. Update Descriptors
            var inInfo = new DescriptorBufferInfo { Buffer = inBuf, Offset = 0, Range = (ulong)compressedData.Length };
            var outInfo = new DescriptorBufferInfo { Buffer = outBuf, Offset = 0, Range = (ulong)outputData.Length };

            var writes = stackalloc WriteDescriptorSet[2];
            writes[0] = new WriteDescriptorSet
            {
                SType = StructureType.WriteDescriptorSet,
                DstSet = set,
                DstBinding = 0,
                DescriptorType = DescriptorType.StorageBuffer,
                DescriptorCount = 1,
                PBufferInfo = &inInfo
            };
            writes[1] = new WriteDescriptorSet
            {
                SType = StructureType.WriteDescriptorSet,
                DstSet = set,
                DstBinding = 1,
                DescriptorType = DescriptorType.StorageBuffer,
                DescriptorCount = 1,
                PBufferInfo = &outInfo
            };
            _vk.UpdateDescriptorSets(_device, 2, writes, 0, null);

            // 5. Record Command Buffer
            var cmdInfo = new CommandBufferAllocateInfo
            {
                SType = StructureType.CommandBufferAllocateInfo,
                CommandPool = _commandPool,
                Level = CommandBufferLevel.Primary,
                CommandBufferCount = 1
            };
            CommandBuffer cmd;
            _vk.AllocateCommandBuffers(_device, &cmdInfo, &cmd);

            var beginInfo = new CommandBufferBeginInfo { SType = StructureType.CommandBufferBeginInfo, Flags = CommandBufferUsageFlags.OneTimeSubmitBit };
            _vk.BeginCommandBuffer(cmd, &beginInfo);

            _vk.CmdBindPipeline(cmd, PipelineBindPoint.Compute, _pipeline);
            _vk.CmdBindDescriptorSets(cmd, PipelineBindPoint.Compute, _pipelineLayout, 0, 1, &set, 0, null);

            // Push Constants (e.g., offsets)
            // _vk.CmdPushConstants(...)

            // Dispatch
            uint groupSize = 64; // Depends on shader
            uint groups = ((uint)compressedData.Length + groupSize - 1) / groupSize;
            _vk.CmdDispatch(cmd, groups, 1, 1);

            _vk.EndCommandBuffer(cmd);

            // 6. Submit & Wait
            var submitInfo = new SubmitInfo
            {
                SType = StructureType.SubmitInfo,
                CommandBufferCount = 1,
                PCommandBuffers = &cmd
            };

            // Sync is simplified here. In async engine, use Fences.
            _vk.QueueSubmit(_computeQueue, 1, &submitInfo, default);
            _vk.QueueWaitIdle(_computeQueue);

            // 7. Download Data
            _vk.MapMemory(_device, outMem, 0, (ulong)outputData.Length, 0, &pData);
            fixed (byte* ptr = outputData)
            {
                System.Buffer.MemoryCopy(pData, ptr, outputData.Length, outputData.Length);
            }
            _vk.UnmapMemory(_device, outMem);

            // Cleanup resources
            _vk.FreeCommandBuffers(_device, _commandPool, 1, &cmd);
            _vk.DestroyBuffer(_device, inBuf, null);
            _vk.FreeMemory(_device, inMem, null);
            _vk.DestroyBuffer(_device, outBuf, null);
            _vk.FreeMemory(_device, outMem, null);
        }

        private void CreateBuffer(ulong size, BufferUsageFlags usage, MemoryPropertyFlags properties, out Buffer buffer, out DeviceMemory memory)
        {
            var bufferInfo = new BufferCreateInfo
            {
                SType = StructureType.BufferCreateInfo,
                Size = size,
                Usage = usage,
                SharingMode = SharingMode.Exclusive
            };

            // buffer is an out param (stack variable), no need to fix
            fixed (Buffer* pBuffer = &buffer)
            {
                // Wait, buffer is on stack, can't fix stack variables using fixed statement.
                // However, Silk.NET CreateBuffer takes a pointer.
                // We should just pass the address.
                // NOTE: Using a local variable 'localBuffer' inside fixed block is cleaner if out param causes issues,
                // but simply passing &buffer works in unsafe context if we don't use 'fixed'.
            }

            // Correct approach for out parameter in unsafe context:
            _vk.CreateBuffer(_device, &bufferInfo, null, out buffer);

            MemoryRequirements memReq;
            _vk.GetBufferMemoryRequirements(_device, buffer, &memReq);

            var allocInfo = new MemoryAllocateInfo
            {
                SType = StructureType.MemoryAllocateInfo,
                AllocationSize = memReq.Size,
                MemoryTypeIndex = FindMemoryType(memReq.MemoryTypeBits, properties)
            };

            _vk.AllocateMemory(_device, &allocInfo, null, out memory);

            _vk.BindBufferMemory(_device, buffer, memory, 0);
        }

        private uint FindMemoryType(uint typeFilter, MemoryPropertyFlags properties)
        {
            PhysicalDeviceMemoryProperties memProps;
            _vk.GetPhysicalDeviceMemoryProperties(_physicalDevice, &memProps);

            for (int i = 0; i < memProps.MemoryTypeCount; i++)
            {
                if ((typeFilter & (1 << i)) != 0 &&
                    (memProps.MemoryTypes[i].PropertyFlags & properties) == properties)
                {
                    return (uint)i;
                }
            }
            throw new Exception("Failed to find suitable memory type");
        }

        public void Dispose()
        {
            if (_vk == null) return;
            _vk.DeviceWaitIdle(_device);

            if (_descriptorPool.Handle != 0) _vk.DestroyDescriptorPool(_device, _descriptorPool, null);
            if (_commandPool.Handle != 0) _vk.DestroyCommandPool(_device, _commandPool, null);
            if (_pipeline.Handle != 0) _vk.DestroyPipeline(_device, _pipeline, null);
            if (_pipelineLayout.Handle != 0) _vk.DestroyPipelineLayout(_device, _pipelineLayout, null);
            if (_descriptorSetLayout.Handle != 0) _vk.DestroyDescriptorSetLayout(_device, _descriptorSetLayout, null);
            if (_device.Handle != 0) _vk.DestroyDevice(_device, null);
            if (_instance.Handle != 0) _vk.DestroyInstance(_instance, null);
            _vk.Dispose();
        }
    }
}
