using System.Buffers.Binary;
using System.Text;

namespace GPCK.Core
{
    /// <summary>
    /// Represents a 128-bit Global Asset Identifier (GUID).
    /// Optimized to use XXHash64 for high-performance generation.
    /// </summary>
    public static class AssetIdGenerator
    {
        /// <summary>
        /// Generates a deterministic GUID based on the file path using XXHash64.
        /// Note: Standard GUID is 128-bit. We use two passes of XXHash64 (seeded) to fill it.
        /// </summary>
        public static Guid Generate(string path)
        {
            if (string.IsNullOrEmpty(path)) return Guid.Empty;

            // Normalize path
            string normalized = path.Replace('\\', '/').ToLowerInvariant();
            byte[] bytes = Encoding.UTF8.GetBytes(normalized);

            // Pass 1: Seed 0
            ulong h1 = XxHash64.Compute(bytes, 0);
            // Pass 2: Seed using h1
            ulong h2 = XxHash64.Compute(bytes, h1);

            // Construct Guid from two 64-bit hashes
            byte[] guidBytes = new byte[16];
            BinaryPrimitives.WriteUInt64LittleEndian(guidBytes.AsSpan(0, 8), h1);
            BinaryPrimitives.WriteUInt64LittleEndian(guidBytes.AsSpan(8, 8), h2);

            // Set UUID Version 4 (Random/Pseudo-Random) bits just to be valid GUID
            guidBytes[6] = (byte)((guidBytes[6] & 0x0F) | (4 << 4));
            guidBytes[8] = (byte)((guidBytes[8] & 0x3F) | 0x80);

            return new Guid(guidBytes);
        }
    }
}