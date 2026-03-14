using System.Runtime.InteropServices;

namespace GPCK.Core
{
    public static partial class CodecGDeflate
    {
        private const string DllName = "GDeflate";

        static CodecGDeflate()
        {
            // With the standard 'runtimes/win-x64/native/' structure,
            // .NET handles resolution automatically. Custom resolver removed.
        }

        public static bool IsAvailable()
        {
            try
            {
                // Verify we can resolve the bound function
                CompressBound(0);
                return true;
            }
            catch (DllNotFoundException)
            {
                return false;
            }
            catch (Exception)
            {
                return false;
            }
        }

        [LibraryImport(DllName, EntryPoint = "GDeflateCompressBound")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        public static partial ulong CompressBound(ulong size);

        [LibraryImport(DllName, EntryPoint = "GDeflateCompress")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        [return: MarshalAs(UnmanagedType.I1)]
        public static unsafe partial bool Compress(
            void* output,
            ref ulong outputSize,
            void* input,
            ulong inputSize,
            uint level,
            uint flags);

        [LibraryImport(DllName, EntryPoint = "GDeflateDecompress")]
        [UnmanagedCallConv(CallConvs = new Type[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
        [return: MarshalAs(UnmanagedType.I1)]
        public static unsafe partial bool Decompress(
            void* output,
            ulong outputSize,
            void* input,
            ulong inputSize,
            uint numWorkers);
    }
}
