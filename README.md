# GPCK (Game Package)

**High-performance asset management for .NET 10 & DirectStorage.**

GPCK is a archive format game engines. It leverages **DirectStorage** and **GDeflate** to eliminate loading bottlenecks and maximize NVMe throughput.

---

## 🚀 Key Features

*   **Split Archive Architecture:** Separates the Table of Contents (`.gtoc`) from the actual data (`.gdat`), allowing the TOC to stay in memory while data is streamed asynchronously from disk.
*   **Hardware Decompression (Path B):** Full support for **GDeflate** and DirectStorage, allowing compressed assets to be sent directly to the GPU, bypassing the CPU entirely.
*   **Software Decompression (Path A):** Includes **Zstd** (Ultra compression for logic/data) and **LZ4** (High-speed for real-time streaming).
*   **Zero-Copy Ready:** Strict 4KB alignment for DirectStorage and Vulkan Compute pipelines with minimal alignment slack (< 0.1%).
*   **Smart Deduplication:** Integrated **xxHash64** fingerprinting removes redundant assets automatically.
*   **Data Locality:** Physical sorting by original path ensures sequential read patterns on disk, while the TOC is sorted by AssetId for $O(\log N)$ lookups.

---

## 🛠 Components

| Name | Role |
| :--- | :--- |
| **GPCK.Core** | The heart of the system: VFS, packing engine, and native codec interop. |
| **GPCK.Avalonia** | Cross-platform GUI with an **Archive Fragmentation Map** visualizer, multi-selection extraction, and asset inspection. |
| **GPCK.Benchmark** | Integrated hardware and format validation suite. |
| **GPCK.CLI** | Headless tool for CI/CD build pipelines. |

---

## 📦 Quick Start (CLI)

```bash
# Pack folder into an optimized split archive (.gtoc / .gdat)
gpck pack "C:\Source\Assets" "Data.gtoc" --method Auto --level 9

# Unpack archive
gpck unpack "Data.gtoc" "C:\Output"

# Technical inspection
gpck info "Data.gtoc"
```

---

## 🔧 Building

*   **SDK:** .NET 10.0
*   **Platform:** x64 (Required for GDeflate/DirectStorage)
*   **Deps:** Included in `runtimes/` (`dstorage.dll`, `libzstd.dll`, etc.)

```bash
dotnet build -c Release
```
