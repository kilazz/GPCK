using System.Runtime.InteropServices;

namespace GPCK.Core
{
    /// <summary>
    /// Native wrapper for Zstandard (libzstd.dll).
    /// Used for high-ratio CPU compression of non-GPU assets (Scripts, JSON, Physics).
    /// </summary>
    public static partial class CodecZstd
    {
        private const string DllName = "libzstd.dll";

        [LibraryImport(DllName, EntryPoint = "ZSTD_compressBound")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        public static partial ulong ZSTD_compressBound(ulong srcSize);

        [LibraryImport(DllName, EntryPoint = "ZSTD_compress")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        public static partial ulong ZSTD_compress(IntPtr dst, ulong dstCapacity, IntPtr src, ulong srcSize, int compressionLevel);

        [LibraryImport(DllName, EntryPoint = "ZSTD_decompress")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        public static partial ulong ZSTD_decompress(IntPtr dst, ulong dstCapacity, IntPtr src, ulong compressedSize);

        [LibraryImport(DllName, EntryPoint = "ZSTD_isError")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        public static partial uint ZSTD_isError(ulong code);

        public static bool IsAvailable()
        {
            try
            {
                // Dummy call to check if DLL loads
                ZSTD_compressBound(0);
                return true;
            }
            catch
            {
                return false;
            }
        }
    }
}