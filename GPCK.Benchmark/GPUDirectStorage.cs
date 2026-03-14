using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using TerraFX.Interop.DirectX;
using TerraFX.Interop.Windows;
using static TerraFX.Interop.DirectX.DirectX;
using static TerraFX.Interop.Windows.Windows;

namespace GPCK.Benchmark
{
    internal static class DStorageConstants
    {
        public const uint DSTORAGE_REQUEST_SOURCE_MEMORY = 1;
        public const uint DSTORAGE_REQUEST_DESTINATION_BUFFER = 1;
        public const uint DSTORAGE_COMPRESSION_FORMAT_GDEFLATE = 1;
        public const ushort DSTORAGE_MAX_QUEUE_CAPACITY = 1024;
        public const int DSTORAGE_STAGING_BUFFER_SIZE_32MB = 32 * 1024 * 1024;
    }

    [StructLayout(LayoutKind.Explicit, Size = 32)]
    internal struct DSTORAGE_QUEUE_DESC
    {
        [FieldOffset(0)] public ulong SourceType;
        [FieldOffset(8)] public ushort Capacity;
        [FieldOffset(10)] public sbyte Priority; // DSTORAGE_PRIORITY (INT8)
        [FieldOffset(16)] public IntPtr Name;
        [FieldOffset(24)] public IntPtr Device;
    }

    [StructLayout(LayoutKind.Sequential, Pack = 8)]
    internal unsafe struct DSTORAGE_REQUEST
    {
        public ulong Options1; // CompressionFormat (8) + Reserved (56)
        public ulong Options2; // SourceType (1) + DestType (7) + Reserved (48)
        public DSTORAGE_SOURCE Source;      // 24 bytes
        public DSTORAGE_DESTINATION Destination; // 40 bytes
        public uint UncompressedSize;       // 4 bytes
        private uint _padding;              // 4 bytes
        public ulong CancellationTag;       // 8 bytes
        public byte* Name;                  // 8 bytes

        public void SetOptions(uint sourceType, uint destType, uint compressionFormat)
        {
            Options1 = (uint)(compressionFormat & 0xFF);
            Options2 = (uint)((sourceType & 0x01UL) | ((destType & 0x7FUL) << 1));
        }
    }

    [StructLayout(LayoutKind.Explicit, Size = 24)]
    internal unsafe struct DSTORAGE_SOURCE
    {
        [FieldOffset(0)] public DSTORAGE_SOURCE_MEMORY Memory;
        [FieldOffset(0)] public IntPtr _file_Source;
        [FieldOffset(8)] public ulong _file_Offset;
        [FieldOffset(16)] public uint _file_Size;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal unsafe struct DSTORAGE_SOURCE_MEMORY
    {
        public void* Source;
        public uint Size;
    }

    [StructLayout(LayoutKind.Explicit, Size = 40)]
    internal unsafe struct DSTORAGE_DESTINATION
    {
        [FieldOffset(0)] public DSTORAGE_DESTINATION_BUFFER Buffer;
        [FieldOffset(0)] public IntPtr _tiles_Resource;
        [FieldOffset(8)] public uint _tiles_X;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal unsafe struct DSTORAGE_DESTINATION_BUFFER
    {
        public ID3D12Resource* Resource;
        public ulong Offset;
        public uint Size;
    }

    [Guid("6924ea0c-c3cd-4826-b10a-f64f4ed927c1")]
    internal unsafe struct IDStorageFactory
    {
#pragma warning disable CS0649
        public void** lpVtbl;
#pragma warning restore CS0649
        public int CreateQueue(DSTORAGE_QUEUE_DESC* desc, Guid* riid, void** ppv) => ((delegate* unmanaged[Stdcall]<IDStorageFactory*, DSTORAGE_QUEUE_DESC*, Guid*, void**, int>)lpVtbl[3])((IDStorageFactory*)Unsafe.AsPointer(ref this), desc, riid, ppv);
        public int SetStagingBufferSize(uint size) => ((delegate* unmanaged[Stdcall]<IDStorageFactory*, uint, int>)lpVtbl[7])((IDStorageFactory*)Unsafe.AsPointer(ref this), size);
        public uint Release() => ((delegate* unmanaged[Stdcall]<IDStorageFactory*, uint>)lpVtbl[2])((IDStorageFactory*)Unsafe.AsPointer(ref this));
    }

    [Guid("cfdbd83f-9e06-4fda-8ea5-69042137f49b")]
    internal unsafe struct IDStorageQueue
    {
#pragma warning disable CS0649
        public void** lpVtbl;
#pragma warning restore CS0649
        public void EnqueueRequest(DSTORAGE_REQUEST* request) => ((delegate* unmanaged[Stdcall]<IDStorageQueue*, DSTORAGE_REQUEST*, void>)lpVtbl[3])((IDStorageQueue*)Unsafe.AsPointer(ref this), request);
        public void EnqueueSignal(ID3D12Fence* fence, ulong value) => ((delegate* unmanaged[Stdcall]<IDStorageQueue*, ID3D12Fence*, ulong, void>)lpVtbl[5])((IDStorageQueue*)Unsafe.AsPointer(ref this), fence, value);
        public void Submit() => ((delegate* unmanaged[Stdcall]<IDStorageQueue*, void>)lpVtbl[6])((IDStorageQueue*)Unsafe.AsPointer(ref this));
        public uint Release() => ((delegate* unmanaged[Stdcall]<IDStorageQueue*, uint>)lpVtbl[2])((IDStorageQueue*)Unsafe.AsPointer(ref this));
    }

    [Guid("e9eb5314-33aa-42b2-a718-d77f58b1f1c7")]
    internal unsafe struct ID3D12SDKConfiguration
    {
#pragma warning disable CS0649
        public void** lpVtbl;
#pragma warning restore CS0649
        public int SetSDKVersion(uint SDKVersion, byte* SDKPath) => ((delegate* unmanaged[Stdcall]<ID3D12SDKConfiguration*, uint, byte*, int>)lpVtbl[3])((ID3D12SDKConfiguration*)Unsafe.AsPointer(ref this), SDKVersion, SDKPath);
        public uint Release() => ((delegate* unmanaged[Stdcall]<ID3D12SDKConfiguration*, uint>)lpVtbl[2])((ID3D12SDKConfiguration*)Unsafe.AsPointer(ref this));
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct DSTORAGE_CONFIGURATION
    {
        public uint NumSubmitThreads;
        public int NumBuiltInCpuDecompressionThreads;
        public int ForceMappingLayer;
        public int DisableBypassIO;
        public int DisableTelemetry;
        public int DisableGpuDecompressionMetacommand;
        public int DisableGpuDecompression;
    }

    public unsafe class GpuDirectStorage : IDisposable
    {
        [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool SetDllDirectoryW(string lpPathName);

        [DllImport("dstorage.dll", EntryPoint = "DStorageGetFactory", ExactSpelling = true)]
        private static extern int DStorageGetFactory(Guid* riid, void** ppv);

        [DllImport("dstorage.dll", EntryPoint = "DStorageSetConfiguration", ExactSpelling = true)]
        private static extern int DStorageSetConfiguration(DSTORAGE_CONFIGURATION* configuration);

        private ComPtr<ID3D12Device> _device = default;
        private IDStorageFactory* _factory = null;
        private IDStorageQueue* _queue = null;
        private ComPtr<ID3D12Fence> _fence = default;
        private ulong _fenceValue = 0;
        private HANDLE _fenceEvent = default;

        public bool IsSupported { get; private set; }
        public bool IsHardwareAccelerated { get; private set; }
        public string InitError { get; private set; } = "";

        static GpuDirectStorage()
        {
            string baseDir = AppContext.BaseDirectory;
            SetDllDirectoryW(baseDir);

            if (NativeLibrary.TryLoad("d3d12.dll", out IntPtr hD3d12))
            {
                if (NativeLibrary.TryGetExport(hD3d12, "D3D12GetInterface", out IntPtr pGetInterface))
                {
                    var getInterface = (delegate* unmanaged[Stdcall]<Guid*, Guid*, void**, int>)pGetInterface;
                    Guid clsid = new Guid("7cda6aca-a03e-49c8-9458-0334d20e07ce");
                    Guid iid = new Guid("e9eb5314-33aa-42b2-a718-d77f58b1f1c7");
                    void* pConfig = null;

                    if (getInterface(&clsid, &iid, &pConfig) == 0)
                    {
                        var config = (ID3D12SDKConfiguration*)pConfig;
                        string sdkPath = Path.Combine(baseDir, "D3D12") + "\\";

                        byte[] pathBytes = System.Text.Encoding.UTF8.GetBytes(sdkPath + "\0");
                        fixed (byte* pPath = pathBytes)
                        {
                            config->SetSDKVersion(614, pPath);
                        }
                        config->Release();
                    }
                }
            }
        }

        public GpuDirectStorage()
        {
            try
            {
                if (!File.Exists(Path.Combine(AppContext.BaseDirectory, "dstorage.dll")))
                {
                    InitError = "dstorage.dll not found in application directory.";
                    return;
                }

                Guid uuidDevice = typeof(ID3D12Device).GUID;
                int hr = D3D12CreateDevice(null, D3D_FEATURE_LEVEL.D3D_FEATURE_LEVEL_12_0, &uuidDevice, (void**)_device.GetAddressOf());
                if (FAILED(hr)) { InitError = $"D3D12 Device Error: 0x{hr:X}"; return; }

                D3D12_FEATURE_DATA_SHADER_MODEL sm = new() { HighestShaderModel = D3D_SHADER_MODEL.D3D_SHADER_MODEL_6_0 };
                _device.Get()->CheckFeatureSupport(D3D12_FEATURE.D3D12_FEATURE_SHADER_MODEL, &sm, (uint)sizeof(D3D12_FEATURE_DATA_SHADER_MODEL));
                if (sm.HighestShaderModel < D3D_SHADER_MODEL.D3D_SHADER_MODEL_6_0) { InitError = "GPU must support Shader Model 6.0"; return; }

                DSTORAGE_CONFIGURATION dsConfig = new()
                {
                    NumBuiltInCpuDecompressionThreads = 0,
                    NumSubmitThreads = 1, // Use at least 1 submission thread
                    ForceMappingLayer = 0,
                    DisableBypassIO = 0,
                    DisableTelemetry = 1,
                    DisableGpuDecompressionMetacommand = 0,
                    DisableGpuDecompression = 0
                };
                DStorageSetConfiguration(&dsConfig);

                Guid uuidFactory = new Guid("6924ea0c-c3cd-4826-b10a-f64f4ed927c1");
                fixed (IDStorageFactory** ppFactory = &_factory)
                {
                    hr = DStorageGetFactory(&uuidFactory, (void**)ppFactory);
                }
                if (FAILED(hr)) { InitError = $"DStorage Factory Error: 0x{hr:X}"; return; }

                _factory->SetStagingBufferSize(DStorageConstants.DSTORAGE_STAGING_BUFFER_SIZE_32MB);

                DSTORAGE_QUEUE_DESC qDesc = new()
                {
                    SourceType = DStorageConstants.DSTORAGE_REQUEST_SOURCE_MEMORY,
                    Capacity = DStorageConstants.DSTORAGE_MAX_QUEUE_CAPACITY,
                    Priority = 0, // DSTORAGE_PRIORITY_NORMAL
                    Device = (IntPtr)_device.Get()
                };
                Guid uuidQueue = new Guid("cfdbd83f-9e06-4fda-8ea5-69042137f49b");
                fixed (IDStorageQueue** ppQueue = &_queue)
                {
                    hr = _factory->CreateQueue(&qDesc, &uuidQueue, (void**)ppQueue);
                }

                if (FAILED(hr)) { InitError = $"Queue Creation Error: 0x{hr:X}"; return; }

                Guid uuidFence = typeof(ID3D12Fence).GUID;
                _device.Get()->CreateFence(0, D3D12_FENCE_FLAGS.D3D12_FENCE_FLAG_NONE, &uuidFence, (void**)_fence.GetAddressOf());
                _fenceEvent = CreateEventW(null, FALSE, FALSE, null);

                IsSupported = true;
                IsHardwareAccelerated = true;
            }
            catch (Exception ex) { InitError = ex.Message; }
        }

        public double RunDecompressionBatch(byte[] compressedData, int[] chunkSizes, long[] chunkOffsets, int totalOriginalSize, int headerSize = 4)
        {
            if (!IsSupported) return 0;

            ComPtr<ID3D12Resource> dstBuffer = default;
            D3D12_HEAP_PROPERTIES heapProps = new(D3D12_HEAP_TYPE.D3D12_HEAP_TYPE_DEFAULT);

            D3D12_RESOURCE_DESC resDesc = new()
            {
                Dimension = D3D12_RESOURCE_DIMENSION.D3D12_RESOURCE_DIMENSION_BUFFER,
                Alignment = 0,
                Width = (ulong)totalOriginalSize,
                Height = 1,
                DepthOrArraySize = 1,
                MipLevels = 1,
                Format = DXGI_FORMAT.DXGI_FORMAT_UNKNOWN,
                SampleDesc = new DXGI_SAMPLE_DESC { Count = 1, Quality = 0 },
                Layout = D3D12_TEXTURE_LAYOUT.D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                Flags = D3D12_RESOURCE_FLAGS.D3D12_RESOURCE_FLAG_NONE
            };

            Guid uuidRes = typeof(ID3D12Resource).GUID;
            _device.Get()->CreateCommittedResource(&heapProps, D3D12_HEAP_FLAGS.D3D12_HEAP_FLAG_NONE, &resDesc, D3D12_RESOURCE_STATES.D3D12_RESOURCE_STATE_COMMON, null, &uuidRes, (void**)dstBuffer.GetAddressOf());

            long startTicks = Stopwatch.GetTimestamp();
            fixed (byte* pData = compressedData)
            {
                ulong outOffset = 0;
                for (int i = 0; i < chunkSizes.Length; i++)
                {
                    DSTORAGE_REQUEST req = new();
                    req.SetOptions(DStorageConstants.DSTORAGE_REQUEST_SOURCE_MEMORY, DStorageConstants.DSTORAGE_REQUEST_DESTINATION_BUFFER, DStorageConstants.DSTORAGE_COMPRESSION_FORMAT_GDEFLATE);

                    req.Source.Memory = new DSTORAGE_SOURCE_MEMORY { Source = pData + chunkOffsets[i] + headerSize, Size = (uint)chunkSizes[i] };
                    req.Destination.Buffer = new DSTORAGE_DESTINATION_BUFFER { Resource = dstBuffer.Get(), Offset = outOffset, Size = 131072 };
                    req.UncompressedSize = 131072;

                    if (outOffset + 131072 > (ulong)totalOriginalSize)
                        req.UncompressedSize = req.Destination.Buffer.Size = (uint)((ulong)totalOriginalSize - outOffset);

                    _queue->EnqueueRequest(&req);
                    outOffset += 131072;
                }

                _fenceValue++;
                _queue->EnqueueSignal(_fence.Get(), _fenceValue);
                _queue->Submit();

                if (_fence.Get()->GetCompletedValue() < _fenceValue)
                {
                    _fence.Get()->SetEventOnCompletion(_fenceValue, _fenceEvent);
                    WaitForSingleObject(_fenceEvent, INFINITE);
                }
            }

            double time = (double)(Stopwatch.GetTimestamp() - startTicks) / Stopwatch.Frequency;
            dstBuffer.Dispose();
            return time;
        }

        public void Dispose()
        {
            if (_queue != null) _queue->Release();
            if (_factory != null) _factory->Release();
            if (_fenceEvent.Value != null) CloseHandle(_fenceEvent);
            _fence.Dispose();
            _device.Dispose();
        }
    }
}
