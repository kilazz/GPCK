using System.Runtime.InteropServices;

namespace GPCK.Core
{
    public static partial class CodecLZ4
    {
        private const string DllName = "liblz4.dll";

        [LibraryImport(DllName, EntryPoint = "LZ4_compressBound")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        public static partial int LZ4_compressBound(int inputSize);

        [LibraryImport(DllName, EntryPoint = "LZ4_compress_default")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        public static partial int LZ4_compress_default(
            IntPtr src,
            IntPtr dst,
            int srcSize,
            int dstCapacity);

        [LibraryImport(DllName, EntryPoint = "LZ4_compress_HC")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        public static partial int LZ4_compress_HC(
            IntPtr src,
            IntPtr dst,
            int srcSize,
            int dstCapacity,
            int compressionLevel);

        [LibraryImport(DllName, EntryPoint = "LZ4_decompress_safe")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        public static partial int LZ4_decompress_safe(
            IntPtr src,
            IntPtr dst,
            int compressedSize,
            int dstCapacity);

        public static bool IsAvailable()
        {
            try
            {
                LZ4_compressBound(0);
                return true;
            }
            catch
            {
                return false;
            }
        }
    }
}