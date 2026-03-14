cmake_minimum_required(VERSION 3.19)

project(GDeflateWrapper)

# 1. Add the original GDeflate project as a subdirectory.
#    This will build the 'GDeflate' target as a STATIC library (per its default).
#    The path is relative to this CMakeLists.txt.
add_subdirectory(../../DirectStorage/GDeflate/GDeflate GDeflateStaticBuild)

# 2. Create our shared library (DLL) using the shim source
add_library(GDeflateWrapper SHARED GDeflateShim.cpp)

# 3. Link the static GDeflate library into our DLL.
#    This pulls all the code from the static lib into the new DLL.
target_link_libraries(GDeflateWrapper PRIVATE GDeflate)

# 4. Rename the output file to GDeflate.dll (instead of GDeflateWrapper.dll)
#    This matches the DllImport name expected by the C# application.
set_target_properties(GDeflateWrapper PROPERTIES OUTPUT_NAME "GDeflate")

# Ensure we use C++17 as required by GDeflate
target_compile_features(GDeflateWrapper PRIVATE cxx_std_17)
