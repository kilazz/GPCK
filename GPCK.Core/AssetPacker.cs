using System.Collections.Concurrent;
using System.Security.Cryptography;
using System.Text;

namespace GPCK.Core
{
    public class AssetPacker
    {
        private const int ChunkSize = 131072; // 128KB
        private const int DefaultAlignment = 16;
        private const int GpuAlignment = 4096;

        public enum CompressionMethod { Auto, Store, GDeflate, Zstd, LZ4 }

        private class ProcessedFile
        {
            public Guid AssetId;
            public required string OriginalPath;
            public uint OriginalSize;
            public uint CompressedSize;
            public byte[]? CompressedData;
            public uint Flags;
            public int Alignment;
            public uint Meta1;
            public uint Meta2;
            public byte[]? ChunkTableData;
            public int ChunkCount;
        }

        public static Dictionary<string, string> BuildFileMap(string inputPath)
        {
            var map = new Dictionary<string, string>();
            if (File.Exists(inputPath))
            {
                map[inputPath] = Path.GetFileName(inputPath);
                return map;
            }

            string root = Path.GetFullPath(inputPath);
            if (!Directory.Exists(root)) return map;

            foreach (var file in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories))
                map[file] = Path.GetRelativePath(root, file).Replace('\\', '/');
            return map;
        }

        public async ValueTask CompressFilesToArchiveAsync(IDictionary<string, string> fileMap, string outputPath, bool enableDedup, int level, byte[]? key, bool mipSplit, IProgress<int>? progress, CancellationToken token, CompressionMethod forceMethod = CompressionMethod.Auto)
        {
            var processed = new ConcurrentBag<ProcessedFile>();
            int count = 0;

            await Parallel.ForEachAsync(fileMap, new ParallelOptions { MaxDegreeOfParallelism = Environment.ProcessorCount, CancellationToken = token }, async (kvp, ct) =>
            {
                await ProcessFile(kvp.Key, kvp.Value, level, key, mipSplit, forceMethod, processed, ct);
                progress?.Report((int)(Interlocked.Increment(ref count) / (float)fileMap.Count * 80));
            });

            // Sort by OriginalPath to ensure files in the same folder are physically adjacent in the archive (Data Locality).
            await WriteArchive(processed.OrderBy(f => f.OriginalPath, StringComparer.OrdinalIgnoreCase).ToList(), outputPath, enableDedup, key, progress);
        }

        private async ValueTask ProcessFile(string input, string rel, int level, byte[]? key, bool mipSplit, CompressionMethod force, ConcurrentBag<ProcessedFile> outBag, CancellationToken ct)
        {
            byte[] raw = await File.ReadAllBytesAsync(input, ct);
            CompressionMethod method = force == CompressionMethod.Auto ? (input.EndsWith(".dds", StringComparison.OrdinalIgnoreCase) ? CompressionMethod.GDeflate : CompressionMethod.Zstd) : force;

            uint m1 = 0, m2 = 0;
            if (input.EndsWith(".dds", StringComparison.OrdinalIgnoreCase))
            {
                var h = DdsUtils.GetHeaderInfo(raw);
                if (h.HasValue)
                {
                    m1 = ((uint)h.Value.Width << 16) | (uint)h.Value.Height;

                    if (mipSplit)
                    {
                        raw = DdsUtils.ProcessTextureForStreaming(raw, out int tail);
                        // Store MipCount in high 8 bits, Tail Size in low 24 bits
                        m2 = ((uint)h.Value.MipCount << 24) | (uint)(tail & 0xFFFFFF);
                    }
                    else
                    {
                        m2 = (uint)h.Value.MipCount << 24;
                    }
                }
            }

            var (compressed, table, chunkCount) = await CompressToChunksAsync(raw, level, key, method);

            uint flags = GameArchive.FLAG_STREAMING | (method != CompressionMethod.Store ? GameArchive.FLAG_IS_COMPRESSED : 0);

            // Add method flags
            flags |= method switch
            {
                CompressionMethod.GDeflate => GameArchive.METHOD_GDEFLATE,
                CompressionMethod.Zstd => GameArchive.METHOD_ZSTD,
                CompressionMethod.LZ4 => GameArchive.METHOD_LZ4,
                _ => GameArchive.METHOD_STORE
            };

            if (key != null) flags |= GameArchive.FLAG_ENCRYPTED_META;
            if (m1 != 0) flags |= GameArchive.TYPE_TEXTURE;

            int align = method == CompressionMethod.GDeflate ? GpuAlignment : DefaultAlignment;
            int alignPower = (int)Math.Log2(align);
            flags |= (uint)(alignPower << GameArchive.SHIFT_ALIGNMENT);

            outBag.Add(new ProcessedFile
            {
                AssetId = AssetIdGenerator.Generate(rel),
                OriginalPath = rel,
                OriginalSize = (uint)raw.Length,
                CompressedSize = (uint)compressed.Length,
                CompressedData = compressed,
                Flags = flags,
                Alignment = align,
                Meta1 = m1,
                Meta2 = m2,
                ChunkTableData = table,
                ChunkCount = chunkCount
            });
        }

        private async Task<(byte[] Data, byte[] Table, int ChunkCount)> CompressToChunksAsync(byte[] input, int level, byte[]? key, CompressionMethod method)
        {
            using var ms = new MemoryStream();
            using var bw = new BinaryWriter(ms);
            int blocks = input.Length == 0 ? 1 : (input.Length + ChunkSize - 1) / ChunkSize;

            var entries = new List<ChunkTable.ChunkInfo>();
            for (int i = 0; i < blocks; i++)
            {
                int size = Math.Min(ChunkSize, input.Length - i * ChunkSize);
                byte[] chunk = new byte[size];
                Array.Copy(input, i * ChunkSize, chunk, 0, size);
                byte[] proc = method switch
                {
                    CompressionMethod.GDeflate => CompressGDeflate(chunk, level),
                    CompressionMethod.Zstd => CompressZstd(chunk, level),
                    CompressionMethod.LZ4 => CompressLZ4(chunk, level),
                    _ => chunk
                };
                entries.Add(new ChunkTable.ChunkInfo { CompressedSize = (uint)proc.Length, OriginalSize = (uint)size });
                bw.Write(proc);
            }

            byte[] table = ChunkTable.Write(entries);
            if (key != null) table = Encrypt(table, key);

            return (ms.ToArray(), table, blocks);
        }

        private byte[] CompressGDeflate(byte[] input, int level)
        {
            if (input.Length == 0) return Array.Empty<byte>();
            ulong bound = CodecGDeflate.CompressBound((ulong)input.Length);
            byte[] output = new byte[bound];
            unsafe
            {
                fixed (byte* pI = input, pO = output)
                {
                    ulong outS = bound;
                    bool success = CodecGDeflate.Compress(pO, ref outS, pI, (ulong)input.Length, (uint)level, 0);
                    if (!success || outS >= (ulong)input.Length) return input;
                    Array.Resize(ref output, (int)outS);
                    return output;
                }
            }
        }

        private byte[] CompressZstd(byte[] input, int level)
        {
            if (input.Length == 0) return Array.Empty<byte>();
            ulong bound = CodecZstd.ZSTD_compressBound((ulong)input.Length);
            byte[] output = new byte[bound];
            unsafe
            {
                fixed (byte* pI = input, pO = output)
                {
                    ulong outS = CodecZstd.ZSTD_compress((IntPtr)pO, bound, (IntPtr)pI, (ulong)input.Length, level);
                    if (CodecZstd.ZSTD_isError(outS) != 0 || outS >= (ulong)input.Length) return input;
                    Array.Resize(ref output, (int)outS);
                    return output;
                }
            }
        }

        private byte[] CompressLZ4(byte[] input, int level)
        {
            if (input.Length == 0) return Array.Empty<byte>();
            int bound = CodecLZ4.LZ4_compressBound(input.Length);
            byte[] output = new byte[bound];
            unsafe
            {
                fixed (byte* pI = input, pO = output)
                {
                    int outS = level > 3
                        ? CodecLZ4.LZ4_compress_HC((IntPtr)pI, (IntPtr)pO, input.Length, bound, level)
                        : CodecLZ4.LZ4_compress_default((IntPtr)pI, (IntPtr)pO, input.Length, bound);
                    if (outS <= 0 || outS >= input.Length) return input;
                    Array.Resize(ref output, outS);
                    return output;
                }
            }
        }

        private byte[] Encrypt(byte[] data, byte[] key)
        {
            if (key.Length != 32) throw new ArgumentException("Key must be 32 bytes for AES-256-GCM");
            byte[] output = new byte[28 + data.Length];
            RandomNumberGenerator.Fill(output.AsSpan(0, 12));
            using var aes = new AesGcm(key, 16);
            aes.Encrypt(output.AsSpan(0, 12), data, output.AsSpan(28), output.AsSpan(12, 16));
            return output;
        }

        private async Task WriteArchive(List<ProcessedFile> files, string path, bool dedup, byte[]? key, IProgress<int>? progress)
        {
            string gtocPath = Path.ChangeExtension(path, ".gtoc");
            string gdatPath = Path.ChangeExtension(path, ".gdat");

            using var fsGtoc = new FileStream(gtocPath, FileMode.Create);
            using var bwGtoc = new BinaryWriter(fsGtoc);

            using var fsGdat = new FileStream(gdatPath, FileMode.Create);

            // 1. Calculate table offsets
            long fileTableOffset = 64;
            long nameTableOffset = fileTableOffset + (files.Count * 56);
            long chunkTableOffset = nameTableOffset + files.Sum(f => 16 + 2 + Encoding.UTF8.GetByteCount(f.OriginalPath));
            long dataStartOffset = 0;

            // 2. Write Header
            bwGtoc.Write(0x4B435047); // "GPCK" in little-endian
            bwGtoc.Write(1); // Version
            bwGtoc.Write(files.Count);
            bwGtoc.Write(0); // Padding
            bwGtoc.Write(fileTableOffset);
            bwGtoc.Write(nameTableOffset);
            bwGtoc.Seek(64, SeekOrigin.Begin);

            // 3. Pre-calculate offsets with deduplication
            // Note: 'files' is sorted by Path for Data Locality.
            var contentMap = new Dictionary<ulong, long>();
            var uniqueDataToWrite = new List<(ProcessedFile File, long Offset)>();
            long currentDataPtr = dataStartOffset;
            long currentChunkTablePtr = chunkTableOffset;

            var finalEntries = new List<(ProcessedFile File, long DataOffset, long ChunkOffset)>();

            foreach (var f in files)
            {
                long dOffset;
                if (dedup)
                {
                    ulong hash = XxHash64.Compute(f.CompressedData!);
                    if (contentMap.TryGetValue(hash, out long existing))
                    {
                        dOffset = existing;
                    }
                    else
                    {
                        dOffset = (currentDataPtr + f.Alignment - 1) & ~(f.Alignment - 1);
                        contentMap[hash] = dOffset;
                        uniqueDataToWrite.Add((f, dOffset));
                        currentDataPtr = dOffset + f.CompressedData!.Length;
                    }
                }
                else
                {
                    dOffset = (currentDataPtr + f.Alignment - 1) & ~(f.Alignment - 1);
                    uniqueDataToWrite.Add((f, dOffset));
                    currentDataPtr = dOffset + f.CompressedData!.Length;
                }

                long cOffset = currentChunkTablePtr;
                currentChunkTablePtr += f.ChunkTableData!.Length;

                finalEntries.Add((f, dOffset, cOffset));
            }

            // CRITICAL: The File Table MUST be sorted by AssetId because the reader (GameArchive)
            // uses Binary Search to find files. The Data blocks can remain sorted by Path for locality.
            var tableEntries = finalEntries.OrderBy(x => x.File.AssetId).ToList();

            // 4. Write File Table
            foreach (var (f, dOffset, cOffset) in tableEntries)
            {
                bwGtoc.Write(f.AssetId.ToByteArray());
                bwGtoc.Write(dOffset);
                bwGtoc.Write(cOffset);
                bwGtoc.Write(f.CompressedSize);
                bwGtoc.Write(f.OriginalSize);
                bwGtoc.Write(f.Flags);
                bwGtoc.Write(f.Meta1);
                bwGtoc.Write(f.Meta2);
                bwGtoc.Write(f.ChunkCount);
            }

            // 5. Write Name Table
            // Sorting Name Table by AssetId is cleaner and keeps it consistent with File Table index.
            foreach (var (f, _, _) in tableEntries)
            {
                bwGtoc.Write(f.AssetId.ToByteArray());
                byte[] n = Encoding.UTF8.GetBytes(f.OriginalPath);
                bwGtoc.Write((ushort)n.Length);
                bwGtoc.Write(n);
            }

            // 6. Write Chunk Tables to .gtoc
            foreach (var f in files)
            {
                bwGtoc.Write(f.ChunkTableData!);
            }

            // 7. Write Data Blocks (in the order optimized for locality)
            progress?.Report(90);
            foreach (var (f, offset) in uniqueDataToWrite)
            {
                fsGdat.Seek(offset, SeekOrigin.Begin);
                await fsGdat.WriteAsync(f.CompressedData!);
            }
            progress?.Report(100);
        }

        public static bool IsCpuLibraryAvailable() => CodecGDeflate.IsAvailable();
        public PackageInfo InspectPackage(string path) { using var arch = new GameArchive(path); return arch.GetPackageInfo(); }
        public async Task DecompressArchiveAsync(string path, string outDir, byte[]? key, IProgress<int>? progress)
        {
            using var arch = new GameArchive(path) { DecryptionKey = key };
            foreach (var e in arch.GetPackageInfo().Entries)
            {
                if (arch.TryGetEntry(e.AssetId, out var entry))
                {
                    string p = Path.Combine(outDir, e.Path); Directory.CreateDirectory(Path.GetDirectoryName(p)!);
                    using var s = arch.OpenRead(entry); using var df = File.Create(p); await s.CopyToAsync(df);
                }
            }
        }

        // Multi-threaded verification using RandomAccess for NVMe speeds.
        public bool VerifyArchive(string path, byte[]? key)
        {
            try
            {
                using var arch = new GameArchive(path) { DecryptionKey = key };
                bool allGood = true;

                Parallel.For(0, arch.FileCount, new ParallelOptions { MaxDegreeOfParallelism = Environment.ProcessorCount }, (i, state) =>
                {
                    try
                    {
                        using var s = arch.OpenRead(arch.GetEntryByIndex(i));
                        s.CopyTo(Stream.Null);
                    }
                    catch
                    {
                        allGood = false;
                        state.Stop();
                    }
                });

                return allGood;
            }
            catch { return false; }
        }
    }
}
