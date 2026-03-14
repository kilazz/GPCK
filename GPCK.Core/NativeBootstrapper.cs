using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace GPCK.Core
{
    internal static class NativeBootstrapper
    {
#pragma warning disable CA2255 // The 'ModuleInitializer' attribute is only intended to be used in application code or advanced source generator scenarios
        [ModuleInitializer]
#pragma warning restore CA2255
        internal static void Initialize()
        {
            NativeLibrary.SetDllImportResolver(typeof(NativeBootstrapper).Assembly, ResolveNativeLibrary);
        }

        private static IntPtr ResolveNativeLibrary(string libraryName, Assembly assembly, DllImportSearchPath? searchPath)
        {
            // 1. Try standard load first (PATH, app dir, etc)
            if (NativeLibrary.TryLoad(libraryName, assembly, searchPath, out IntPtr handle))
                return handle;

            // 2. Try looking in the runtimes/{rid}/native folder structure
            string rid = RuntimeInformation.IsOSPlatform(OSPlatform.Windows) ? "win-x64" :
                         RuntimeInformation.IsOSPlatform(OSPlatform.Linux) ? "linux-x64" :
                         RuntimeInformation.IsOSPlatform(OSPlatform.OSX) ? "osx-x64" : "unknown";

            string root = AppContext.BaseDirectory;
            string fileName = libraryName;

            // Ensure we have a file extension for the file check
            if (!Path.HasExtension(fileName))
            {
                if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows)) fileName += ".dll";
                else if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux)) fileName += ".so";
                else if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX)) fileName += ".dylib";
            }

            // Check runtimes path
            string runtimesPath = Path.Combine(root, "runtimes", rid, "native", fileName);
            if (File.Exists(runtimesPath))
            {
                if (NativeLibrary.TryLoad(runtimesPath, out handle))
                    return handle;
            }

            // 3. Fallback: Try looking in the root directory with the platform-specific extension appended
            // (Standard loading might fail for "GDeflate" if it strictly looks for that exact name,
            // so we try "GDeflate.dll" manually in root)
            if (fileName != libraryName)
            {
                string rootPath = Path.Combine(root, fileName);
                if (File.Exists(rootPath) && NativeLibrary.TryLoad(rootPath, out handle))
                    return handle;
            }

            return IntPtr.Zero;
        }
    }
}
