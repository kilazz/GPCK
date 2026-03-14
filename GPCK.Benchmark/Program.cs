using GPCK.Core;
using Spectre.Console;
using System.Diagnostics;
using System.Runtime.InteropServices;

namespace GPCK.Benchmark
{
    class Program
    {
        private const int AlgorithmPayloadSize = 128 * 1024 * 1024; // 128MB
        private const string TempArchiveName = "format_vfs_stress.gtoc";

        [STAThread]
        static void Main(string[] args)
        {
            try
            {
                AnsiConsole.Write(new FigletText("GPCK SYSTEM").Color(Color.SpringGreen3));
                AnsiConsole.MarkupLine("[bold yellow]Integrated Format & Hardware Decompression Benchmark[/]");
                AnsiConsole.MarkupLine($"[gray]Execution Path: {AppContext.BaseDirectory}[/]");
                AnsiConsole.WriteLine();

                if (args.Length > 0 && File.Exists(args[0]))
                {
                    RunCustomFileBenchmark(args[0]);
                    return;
                }

                AnsiConsole.MarkupLine("Select Mode:");
                AnsiConsole.MarkupLine("[[1]] Standard Benchmark Suite (Synthetic Data)");
                AnsiConsole.MarkupLine("[[2]] Custom File Benchmark (Real Data)");
                AnsiConsole.Write("Choice: ");
                var key = Console.ReadKey().KeyChar;
                AnsiConsole.WriteLine();
                AnsiConsole.WriteLine();

                if (key == '2')
                {
                    AnsiConsole.Markup("Enter file path: ");
                    string path = Console.ReadLine()?.Trim('"') ?? "";
                    RunCustomFileBenchmark(path);
                    return;
                }

                PrintSystemReport();

                double hostMemorySpeed = MeasureHostMemoryBandwidth();
                AnsiConsole.MarkupLine($"[bold]Host Memory Bandwidth:[/] [cyan]{hostMemorySpeed:F1} GB/s[/] (Hardware Limit)");
                AnsiConsole.WriteLine();

                // --- Part 1: Raw Algorithm Performance ---
                RunAlgorithmSuite();

                // --- Part 2: Archive Format & VFS Integrity ---
                RunFormatSuite();

                AnsiConsole.WriteLine();
                AnsiConsole.MarkupLine("[gray]Benchmark completed. Press Enter to exit...[/]");
                Console.ReadLine();
            }
            catch (Exception ex)
            {
                AnsiConsole.WriteException(ex);
                AnsiConsole.MarkupLine("[bold red]FATAL ERROR:[/] Process crashed.");
                Console.ReadLine();
            }
            finally
            {
                if (File.Exists(TempArchiveName)) File.Delete(TempArchiveName);
                string gdatName = Path.ChangeExtension(TempArchiveName, ".gdat");
                if (File.Exists(gdatName)) File.Delete(gdatName);
            }
        }

        static void RunAlgorithmSuite()
        {
            AnsiConsole.MarkupLine("[bold white]--- Part 1: Raw Algorithm Throughput (In-Memory) ---[/]");
            byte[] rawData = GenerateRealisticGameData(AlgorithmPayloadSize);

            var table = new Table().Border(TableBorder.Rounded);
            table.AddColumn("Method");
            table.AddColumn("Ratio");
            table.AddColumn("Compress Speed");
            table.AddColumn("Decompress Speed");

            AnsiConsole.Live(table).Start(ctx =>
            {
                RunTest("Store", rawData, 0, (inB, l) => inB, (inB, s) => { byte[] b = new byte[s]; Array.Copy(inB, b, Math.Min(inB.Length, s)); return b; }, table);
                ctx.Refresh();

                if (CodecLZ4.IsAvailable()) { RunTest("LZ4 (HC L9)", rawData, 9, CompressLZ4, DecompressLZ4, table); ctx.Refresh(); }
                if (CodecGDeflate.IsAvailable())
                {
                    RunTest("GDeflate (CPU L12)", rawData, 12, CompressGDeflate, DecompressGDeflate, table); ctx.Refresh();
                    RunGpuBenchmark(rawData, 12, table); ctx.Refresh();
                }
                if (CodecZstd.IsAvailable()) { RunTest("Zstd (Ultra L22)", rawData, 22, CompressZstd, DecompressZstd, table); ctx.Refresh(); }
            });
            AnsiConsole.WriteLine();
        }

        static void RunFormatSuite()
        {
            AnsiConsole.MarkupLine("[bold white]--- Part 2: VFS Format Validation (Real-World IO) ---[/]");

            string dummyDir = Path.Combine(AppContext.BaseDirectory, "integrity_stress_src");
            if (Directory.Exists(dummyDir)) Directory.Delete(dummyDir, true);
            Directory.CreateDirectory(dummyDir);

            int smallFileCount = 3000;
            int largeFileCount = 30; // Increased for better IO saturation

            AnsiConsole.Status().Start($"Building complex package ({smallFileCount} JSON, {largeFileCount} DDS)...", ctx =>
            {
                for (int i = 0; i < smallFileCount; i++)
                {
                    string p = Path.Combine(dummyDir, $"metadata/node_{i:D4}.json");
                    string? dir = Path.GetDirectoryName(p);
                    if (dir != null) Directory.CreateDirectory(dir);
                    File.WriteAllText(p, "{ \"id\": " + i + ", \"salt\": \"" + Guid.NewGuid() + "\", \"payload\": \"STRESS_METADATA_LATENCY\" }");
                }
                for (int i = 0; i < largeFileCount; i++)
                {
                    string p = Path.Combine(dummyDir, $"textures/tex_4k_aligned_{i:D2}.dds");
                    string? dir = Path.GetDirectoryName(p);
                    if (dir != null) Directory.CreateDirectory(dir);
                    File.WriteAllBytes(p, GenerateRealisticGameData(1024 * 1024 * 4)); // 4MB each
                }
            });

            var packer = new AssetPacker();
            var fileMap = AssetPacker.BuildFileMap(dummyDir);
            var sw = Stopwatch.StartNew();

            // 1. Build Time (No dedup for clean slack metrics)
            sw.Restart();
            packer.CompressFilesToArchiveAsync(fileMap, TempArchiveName, false, 6, null, false, null, CancellationToken.None).AsTask().Wait();
            var packTime = sw.ElapsedMilliseconds;
            var archiveSize = new FileInfo(Path.ChangeExtension(TempArchiveName, ".gdat")).Length;

            using var archive = new GameArchive(TempArchiveName);
            var keys = fileMap.Values.ToList();
            var ddsEntries = keys.Where(k => k.EndsWith(".dds")).ToList();

            // 2. Raw Disk IO (Isolating disk speed from decompression)
            long rawIoRead = 0;
            sw.Restart();
            Parallel.ForEach(ddsEntries, new ParallelOptions { MaxDegreeOfParallelism = 8 }, rel =>
            {
                if (archive.TryGetEntry(AssetIdGenerator.Generate(rel), out var entry))
                {
                    byte[] buffer = new byte[entry.CompressedSize];
                    RandomAccess.Read(archive.GetFileHandle(), buffer, entry.DataOffset);
                    Interlocked.Add(ref rawIoRead, buffer.Length);
                }
            });
            var rawDiskSpeed = (rawIoRead / 1024.0 / 1024.0) / sw.Elapsed.TotalSeconds;

            // 3. Parallel VFS (With Decompression overhead)
            long vfsRead = 0;
            sw.Restart();
            Parallel.ForEach(ddsEntries, new ParallelOptions { MaxDegreeOfParallelism = 8 }, rel =>
            {
                if (archive.TryGetEntry(AssetIdGenerator.Generate(rel), out var entry))
                {
                    using var stream = archive.OpenRead(entry);
                    byte[] buffer = new byte[128 * 1024];
                    int read;
                    while ((read = stream.Read(buffer)) > 0) Interlocked.Add(ref vfsRead, read);
                }
            });
            var vfsSpeedPar = (vfsRead / 1024.0 / 1024.0) / sw.Elapsed.TotalSeconds;

            // 4. Alignment & Integrity
            long slackSpace = 0;
            int misalignedCount = 0;
            var entriesByOffset = new List<GameArchive.FileEntry>();
            for (int i = 0; i < archive.FileCount; i++)
            {
                entriesByOffset.Add(archive.GetEntryByIndex(i));
            }
            entriesByOffset.Sort((a, b) => a.DataOffset.CompareTo(b.DataOffset));

            for (int i = 0; i < entriesByOffset.Count; i++)
            {
                var entry = entriesByOffset[i];
                if ((entry.Flags & GameArchive.MASK_METHOD) == GameArchive.METHOD_GDEFLATE && (entry.DataOffset % 4096 != 0)) misalignedCount++;
                if (i > 0)
                {
                    var prev = entriesByOffset[i - 1];
                    long gap = entry.DataOffset - (prev.DataOffset + prev.CompressedSize);
                    if (gap > 0 && gap < 131072) slackSpace += gap;
                }
            }

            // Results Table
            var resTable = new Table().Border(TableBorder.DoubleEdge).Title("[bold cyan]GPCK Pipeline Efficiency Report[/]");
            resTable.AddColumn("Metric");
            resTable.AddColumn("Measured Value");
            resTable.AddColumn("Efficiency Status");

            resTable.AddRow("Pure Disk Throughput", $"[bold white]{rawDiskSpeed:F1} MB/s[/]", "[gray]HARDWARE_CAP[/]");
            resTable.AddRow("VFS Parallel (Path A)", $"[bold cyan]{vfsSpeedPar:F1} MB/s[/]", vfsSpeedPar > (rawDiskSpeed * 0.7) ? "[green]OPTIMAL[/]" : "[yellow]CPU_TAX_HIGH[/]");
            resTable.AddRow("Decompression Tax", $"{((rawDiskSpeed - vfsSpeedPar) / rawDiskSpeed * 100):F1}%", "[dim]CPU Overhead[/]");
            resTable.AddRow("GPU Ready (Path B)", misalignedCount == 0 ? "[green]0 Alignment Errors[/]" : $"[red]{misalignedCount} ERRORS[/]", "[bold green]VALID[/]");
            resTable.AddRow("Alignment Slack", $"{slackSpace / 1024} KB", $"{(double)slackSpace / archiveSize * 100:F2}% ([green]IDEAL[/])");

            AnsiConsole.Write(resTable);
            if (Directory.Exists(dummyDir)) Directory.Delete(dummyDir, true);
        }

        static void PrintSystemReport()
        {
            var grid = new Grid();
            grid.AddColumn(new GridColumn().NoWrap());
            grid.AddColumn();
            grid.AddRow("[bold]Operating System[/]", $"{RuntimeInformation.OSDescription}");
            grid.AddRow("[bold]DirectStorage SDK[/]", NativeLibrary.TryLoad("dstorage.dll", out _) ? "[green]ACTIVE[/]" : "[red]MISSING[/]");
            grid.AddRow("[bold]GPCK Codec Core[/]", CodecGDeflate.IsAvailable() ? "[green]HW_ACCEL READY[/]" : "[yellow]SOFT_ONLY[/]");
            AnsiConsole.Write(new Panel(grid).Header("Environment Hardware Report").BorderColor(Color.Grey));
        }

        static double MeasureHostMemoryBandwidth()
        {
            long size = 512 * 1024 * 1024;
            byte[] src = new byte[size]; byte[] dst = new byte[size];
            new Random().NextBytes(src);
            var sw = Stopwatch.StartNew();
            Parallel.For(0, 128, i => { int chunkSize = (int)(size / 128); Array.Copy(src, i * chunkSize, dst, i * chunkSize, chunkSize); });
            sw.Stop();
            return (size / 1024.0 / 1024.0 / 1024.0) / sw.Elapsed.TotalSeconds;
        }

        static void RunTest(string name, byte[] input, int level, Func<byte[], int, byte[]> compressor, Func<byte[], int, byte[]> decompressor, Table table)
        {
            try
            {
                var sw = Stopwatch.StartNew();
                byte[] compressed = compressor(input, level);
                double compTime = sw.Elapsed.TotalSeconds;
                decompressor(compressed, input.Length);
                sw.Restart();
                for (int i = 0; i < 3; i++) decompressor(compressed, input.Length);
                double decompSpeed = (input.Length / 1024.0 / 1024.0) / (sw.Elapsed.TotalSeconds / 3.0);
                table.AddRow(name, $"{(double)compressed.Length / input.Length * 100:F1}%", $"{(input.Length / 1024.0 / 1024.0) / compTime:F0} MB/s", $"[bold green]{decompSpeed:F0} MB/s[/]");
            }
            catch (Exception ex) { table.AddRow(name, "ERR", "ERR", $"[red]{Markup.Escape(ex.Message)}[/]"); }
        }

        static void RunGpuBenchmark(byte[] input, int level, Table table)
        {
            try
            {
                byte[] compressed = CompressGDeflate(input, level, false);
                using var gpu = new GpuDirectStorage();
                if (!gpu.IsSupported) { table.AddRow("GDeflate (GPU)", "-", "N/A", $"[dim yellow]Unavailable[/]"); return; }
                using var ms = new MemoryStream(compressed); using var br = new BinaryReader(ms);
                int numChunks = br.ReadInt32(); int[] sizes = new int[numChunks]; long[] offsets = new long[numChunks]; long curr = ms.Position;
                for (int i = 0; i < numChunks; i++) { offsets[i] = curr; int s = br.ReadInt32(); if (s == -1) { int rs = br.ReadInt32(); br.BaseStream.Seek(rs, SeekOrigin.Current); curr += 8 + rs; sizes[i] = rs; } else { br.BaseStream.Seek(s, SeekOrigin.Current); curr += 4 + s; sizes[i] = s; } }
                gpu.RunDecompressionBatch(compressed, sizes, offsets, input.Length);
                double t = 0; for (int i = 0; i < 3; i++) t += gpu.RunDecompressionBatch(compressed, sizes, offsets, input.Length);
                double speed = (input.Length / 1024.0 / 1024.0) / (t / 3.0);
                table.AddRow("GDeflate (GPU)", $"{(double)compressed.Length / input.Length * 100:F1}%", "N/A", $"[bold cyan]{speed:F0} MB/s[/]");
            }
            catch (Exception ex) { table.AddRow("GDeflate (GPU)", "ERR", "N/A", $"[red]{Markup.Escape(ex.Message)}[/]"); }
        }

        static byte[] CompressLZ4(byte[] input, int level)
        {
            int bound = CodecLZ4.LZ4_compressBound(input.Length); byte[] output = new byte[bound];
            unsafe { fixed (byte* pI = input, pO = output) { int sz = (level > 3) ? CodecLZ4.LZ4_compress_HC((IntPtr)pI, (IntPtr)pO, input.Length, bound, level) : CodecLZ4.LZ4_compress_default((IntPtr)pI, (IntPtr)pO, input.Length, bound); Array.Resize(ref output, sz); return output; } }
        }
        static byte[] DecompressLZ4(byte[] input, int outS)
        {
            byte[] output = new byte[outS]; unsafe { fixed (byte* pI = input, pO = output) { CodecLZ4.LZ4_decompress_safe((IntPtr)pI, (IntPtr)pO, input.Length, outS); } }
            return output;
        }
        static byte[] CompressZstd(byte[] input, int level)
        {
            ulong bound = CodecZstd.ZSTD_compressBound((ulong)input.Length); byte[] output = new byte[bound];
            unsafe { fixed (byte* pI = input, pO = output) { ulong sz = CodecZstd.ZSTD_compress((IntPtr)pO, bound, (IntPtr)pI, (ulong)input.Length, level); Array.Resize(ref output, (int)sz); return output; } }
        }
        static byte[] DecompressZstd(byte[] input, int outS)
        {
            byte[] output = new byte[outS]; unsafe { fixed (byte* pI = input, pO = output) { CodecZstd.ZSTD_decompress((IntPtr)pO, (ulong)outS, (IntPtr)pI, (ulong)input.Length); } }
            return output;
        }
        static byte[] CompressGDeflate(byte[] input, int level) => CompressGDeflate(input, level, true);
        static byte[] CompressGDeflate(byte[] input, int level, bool bypass)
        {
            using var ms = new MemoryStream(); using var bw = new BinaryWriter(ms);
            int numChunks = (input.Length + 131071) / 131072; bw.Write(numChunks);
            byte[] scratch = new byte[CodecGDeflate.CompressBound(131072)];
            unsafe
            {
                fixed (byte* pI = input, pS = scratch)
                {
                    for (int i = 0; i < numChunks; i++)
                    {
                        int off = i * 131072; int sz = Math.Min(131072, input.Length - off); ulong outS = (ulong)scratch.Length;
                        if (CodecGDeflate.Compress(pS, ref outS, pI + off, (ulong)sz, (uint)level, 0) && (!bypass || outS < (ulong)sz)) { bw.Write((int)outS); bw.Write(new ReadOnlySpan<byte>(scratch, 0, (int)outS)); }
                        else { bw.Write(-1); bw.Write(sz); bw.Write(new ReadOnlySpan<byte>(input, off, sz)); }
                    }
                }
            }
            return ms.ToArray();
        }
        static byte[] DecompressGDeflate(byte[] input, int outS)
        {
            byte[] output = new byte[outS]; using var ms = new MemoryStream(input); using var br = new BinaryReader(ms);
            int chunks = br.ReadInt32(); int[] sizes = new int[chunks]; long[] offsets = new long[chunks]; long curr = ms.Position;
            for (int i = 0; i < chunks; i++) { offsets[i] = curr; int s = br.ReadInt32(); if (s == -1) { int rs = br.ReadInt32(); br.BaseStream.Seek(rs, SeekOrigin.Current); curr += 8 + rs; } else { br.BaseStream.Seek(s, SeekOrigin.Current); curr += 4 + s; } sizes[i] = s; }
            Parallel.For(0, chunks, i =>
            {
                unsafe
                {
                    fixed (byte* pIn = input, pOut = output)
                    {
                        int target = Math.Min(131072, outS - (i * 131072));
                        if (sizes[i] == -1) Buffer.MemoryCopy(pIn + offsets[i] + 8, pOut + (i * 131072), target, target);
                        else CodecGDeflate.Decompress(pOut + (i * 131072), (ulong)target, pIn + offsets[i] + 4, (ulong)sizes[i], 1);
                    }
                }
            });
            return output;
        }

        static byte[] GenerateRealisticGameData(int size)
        {
            byte[] data = new byte[size]; Random rnd = new Random(42);
            for (int k = 0; k < size; k++) { double pattern = Math.Sin(k * 0.05) * 60 + Math.Cos(k * 0.001) * 40; data[k] = (byte)(128 + (int)pattern + rnd.Next(0, 48)); }
            return data;
        }
        static void RunCustomFileBenchmark(string filePath)
        {
            if (filePath.EndsWith(".gtoc", StringComparison.OrdinalIgnoreCase))
            {
                RunArchiveBenchmark(filePath);
                return;
            }

            AnsiConsole.MarkupLine($"[bold white]--- Custom File Benchmark: {Path.GetFileName(filePath)} ---[/]");

            if (!File.Exists(filePath))
            {
                AnsiConsole.MarkupLine($"[red]File not found: {filePath}[/]");
                return;
            }

            byte[] rawData = File.ReadAllBytes(filePath);
            string sizeStr = rawData.Length < 1024 * 1024
                ? $"{rawData.Length / 1024.0:F2} KB"
                : $"{rawData.Length / 1024.0 / 1024.0:F2} MB";

            AnsiConsole.MarkupLine($"File Size: [cyan]{sizeStr}[/]");

            if (rawData.Length < 1024 * 1024)
            {
                AnsiConsole.MarkupLine("[yellow]WARNING: File is too small for accurate GPU benchmarking.[/]");
                AnsiConsole.MarkupLine("[gray]GPU decompression has initialization overhead. Use files > 10 MB for realistic results.[/]");
            }

            var table = new Table().Border(TableBorder.Rounded);
            table.AddColumn("Method");
            table.AddColumn("Ratio");
            table.AddColumn("Compress Speed");
            table.AddColumn("Decompress Speed");

            AnsiConsole.Live(table).Start(ctx =>
            {
                // GDeflate CPU
                if (CodecGDeflate.IsAvailable())
                {
                    RunTest("GDeflate (CPU L12)", rawData, 12, CompressGDeflate, DecompressGDeflate, table);
                    ctx.Refresh();

                    // GDeflate GPU
                    RunGpuBenchmark(rawData, 12, table);
                    ctx.Refresh();
                }
                else
                {
                    table.AddRow("GDeflate", "N/A", "N/A", "[yellow]Not Available[/]");
                }
            });

            AnsiConsole.WriteLine();
            AnsiConsole.MarkupLine("[gray]Press Enter to exit...[/]");
            Console.ReadLine();
        }

        static void RunArchiveBenchmark(string gtocPath)
        {
            AnsiConsole.MarkupLine($"[bold white]--- Archive Benchmark: {Path.GetFileName(gtocPath)} ---[/]");

            string gdatPath = Path.ChangeExtension(gtocPath, ".gdat");
            if (!File.Exists(gdatPath))
            {
                AnsiConsole.MarkupLine($"[red]Error: Data file not found: {gdatPath}[/]");
                return;
            }

            using var archive = new GameArchive(gtocPath);
            AnsiConsole.MarkupLine($"Files in Archive: [cyan]{archive.FileCount}[/]");

            // Find GDeflate entries
            var entries = new List<GameArchive.FileEntry>();
            long totalCompSize = 0;
            long totalOrigSize = 0;

            for (int i = 0; i < archive.FileCount; i++)
            {
                var entry = archive.GetEntryByIndex(i);
                if ((entry.Flags & GameArchive.MASK_METHOD) == GameArchive.METHOD_GDEFLATE)
                {
                    entries.Add(entry);
                    totalCompSize += entry.CompressedSize;
                    totalOrigSize += entry.OriginalSize;
                }
            }

            if (entries.Count == 0)
            {
                AnsiConsole.MarkupLine("[yellow]No GDeflate compressed files found in this archive.[/]");
                return;
            }

            AnsiConsole.MarkupLine($"GDeflate Assets: [cyan]{entries.Count}[/] (Comp: {totalCompSize / 1024.0 / 1024.0:F2} MB, Orig: {totalOrigSize / 1024.0 / 1024.0:F2} MB)");
            AnsiConsole.WriteLine();

            // Limit to 512MB to avoid OOM
            long limit = 512 * 1024 * 1024;
            long currentSize = 0;
            var testEntries = new List<GameArchive.FileEntry>();
            foreach (var e in entries)
            {
                if (currentSize + e.CompressedSize > limit) break;
                testEntries.Add(e);
                currentSize += e.CompressedSize;
            }

            AnsiConsole.MarkupLine($"Benchmarking subset: [cyan]{testEntries.Count}[/] files ({currentSize / 1024.0 / 1024.0:F2} MB compressed)");

            // Preload data into RAM to isolate decompression speed
            var loadedData = new List<(byte[] Data, ChunkTable.ChunkInfo[] Chunks, long[] Offsets)>();

            AnsiConsole.Status().Start("Preloading data...", ctx =>
            {
                using var handle = archive.GetFileHandle();
                foreach (var entry in testEntries)
                {
                    // Read Chunk Table
                    byte[] tableBuffer = new byte[entry.ChunkCount * 8];
                    archive.ReadGtoc(tableBuffer, entry.ChunkTableOffset);
                    var chunks = ChunkTable.Read(tableBuffer, entry.ChunkCount);

                    // Read Data
                    byte[] data = new byte[entry.CompressedSize];
                    RandomAccess.Read(handle, data, entry.DataOffset);

                    // Calculate offsets
                    long[] offsets = new long[entry.ChunkCount];
                    long acc = 0;
                    for (int k = 0; k < entry.ChunkCount; k++)
                    {
                        offsets[k] = acc;
                        acc += chunks[k].CompressedSize;
                    }

                    loadedData.Add((data, chunks, offsets));
                }
            });

            var table = new Table().Border(TableBorder.Rounded);
            table.AddColumn("Method");
            table.AddColumn("Decompress Speed");

            AnsiConsole.Live(table).Start(ctx =>
            {
                // CPU Benchmark
                var sw = Stopwatch.StartNew();
                long totalDec = 0;
                foreach (var item in loadedData)
                {
                    byte[] outBuf = new byte[item.Chunks.Sum(c => c.OriginalSize)];
                    unsafe
                    {
                        fixed (byte* pIn = item.Data, pOut = outBuf)
                        {
                            for (int k = 0; k < item.Chunks.Length; k++)
                            {
                                CodecGDeflate.Decompress(pOut + (k * 131072), item.Chunks[k].OriginalSize, pIn + item.Offsets[k], item.Chunks[k].CompressedSize, 1);
                            }
                        }
                    }
                    totalDec += outBuf.Length;
                }
                sw.Stop();
                double cpuSpeed = (totalDec / 1024.0 / 1024.0) / sw.Elapsed.TotalSeconds;
                table.AddRow("GDeflate (CPU)", $"[green]{cpuSpeed:F0} MB/s[/]");
                ctx.Refresh();

                // GPU Benchmark (Batched)
                try
                {
                    using var gpu = new GpuDirectStorage();
                    if (gpu.IsSupported)
                    {
                        // Prepare Batch: Merge all files into one request to simulate level load
                        // and minimize D3D12 resource creation overhead.
                        long totalBatchCompSize = loadedData.Sum(x => x.Data.Length);
                        int totalBatchChunks = loadedData.Sum(x => x.Chunks.Length);
                        int totalBatchOrigSize = loadedData.Sum(x => x.Chunks.Sum(c => (int)c.OriginalSize));

                        byte[] batchData = new byte[totalBatchCompSize];
                        int[] batchSizes = new int[totalBatchChunks];
                        long[] batchOffsets = new long[totalBatchChunks];

                        long dataPtr = 0;
                        int chunkPtr = 0;

                        foreach (var item in loadedData)
                        {
                            Array.Copy(item.Data, 0, batchData, dataPtr, item.Data.Length);

                            for (int k = 0; k < item.Chunks.Length; k++)
                            {
                                batchSizes[chunkPtr] = (int)item.Chunks[k].CompressedSize;
                                batchOffsets[chunkPtr] = dataPtr + item.Offsets[k]; // Offset relative to start of batch
                                chunkPtr++;
                            }
                            dataPtr += item.Data.Length;
                        }

                        // Warmup
                        gpu.RunDecompressionBatch(batchData, batchSizes, batchOffsets, totalBatchOrigSize, 0);

                        // Measure
                        sw.Restart();
                        // Run 3 times to get stable average
                        double t = 0;
                        for (int i = 0; i < 3; i++)
                            t += gpu.RunDecompressionBatch(batchData, batchSizes, batchOffsets, totalBatchOrigSize, 0);
                        sw.Stop();

                        double gpuSpeed = (totalBatchOrigSize / 1024.0 / 1024.0) / (t / 3.0);
                        table.AddRow("GDeflate (GPU)", $"[bold cyan]{gpuSpeed:F0} MB/s[/] (Batched)");
                    }
                    else
                    {
                        table.AddRow("GDeflate (GPU)", "[dim]Unavailable[/]");
                    }
                }
                catch (Exception ex)
                {
                    table.AddRow("GDeflate (GPU)", $"[red]Error: {ex.Message}[/]");
                }
            });

            AnsiConsole.WriteLine();
            AnsiConsole.MarkupLine("[gray]Press Enter to exit...[/]");
            Console.ReadLine();
        }
    }
}
