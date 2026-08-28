GPCK (Game Package & DirectStorage VFS)

Asset archive and VFS in Rust for Direct-to-VRAM GPU streaming via
DirectStorage 1.4 (BypassIO) and Vulkan 1.3 Compute.

Specifications

| Component               | Implementation                                     | Details                                                                                  |
| :---------------------- | :------------------------------------------------- | :--------------------------------------------------------------------------------------- |
| **Archive Format**      | `.gtoc` (TOC/mmap) + `.gdat` (64KB alignment)      | Lock-free positional I/O (`pread` / `seek_read`), zero mutexes.                          |
| **Indexing**            | CHD Minimal Perfect Hash (1.0 load factor)         | $O(1)$ lookup by 128-bit UUID/PathHash without collisions.                               |
| **Deduplication**       | Content-Defined (XxHash64)                         | Chunk-level deduplication across package archives.                                       |
| **Codecs**              | GDeflate, Brotli-G, Zstd (ATG 256KB), LZ4-HC, rANS | GPU compute decompression up to $7.0\text{ GB/s}$; CPU L2/L3 bounded.                    |
| **GACL Conditioning**   | Mode-Split + 2D Morton Space Curves (`Bc1`–`Bc7`)  | De-interleaves block texture entropy layers prior to LZ.                                 |
| **RDO & Decorrelation** | Lagrangian RDO + YCoCg                             | Reduces high-frequency entropy for BC compressed formats.                                |
| **Virtual Texturing**   | 64KB Sparse Tiles + Mip-Splitting                  | Tail partition ($\le 128\times 128$) loaded in $<1\ \mu\text{s}$; `.highmips` on demand. |
| **Geometry**            | `.gmesh` Meshlets + Task/Mesh Shaders              | 16B vertex + 3B micro-index; cone culling descriptors; 11.4M tris/s clustering.          |
| **Delta Patching**      | Patch-in-Place (PiP) + BFD Bin-Packing             | In-place gap filling without full package rebuilds.                                      |
| **GPU Ingestion**       | DirectStorage 1.4 (Agility 721) / Vulkan 1.3       | Direct stream to `ID3D12Resource` / `VkImage` bypassing host RAM.                        |
| **Encryption**          | AES-256-GCM + PBKDF2-HMAC-SHA256                   | Authenticated metadata and chunk table encryption.                                       |

CLI
```
# Build
cargo build --release
# Pack (DirectStorage + 64KB Sparse Tiles)
gpck pack ./Assets ./Build/Game.gtoc --preset "GPU Streaming"
# Pack (Brotli-G L11 + GACL + RDO + AES-256-GCM)
gpck pack ./Assets ./Build/Game.gtoc -m brotli-g -l 11 --gacl --rdo 100 --sparse-tiles 64k --key "Passphrase"
# Pack (GDeflate + ATG 256KB Bounds)
gpck pack ./Assets ./Build/Game.gtoc -m gdeflate -l 9 --atg-bounds 256k
# Unpack
gpck unpack ./Build/Game.gtoc ./Extracted/ --recombine-mips
# Verify
gpck verify ./Build/Game.gtoc --key "Passphrase"
# Delta Patch
gpck patch ./Game_v1.gtoc ./NewFiles/ ./Patch_v2.gtoc
# Diagnostic Benchmark
gpck bench --full
```

Structure
```
+-----------------------------------------------------------------------------------+
|                            GPCK Interfaces & Studio                               |
|        [gpck_cli]        [gpck_gui]        [gpck_godot]       [C / FFI]           |
+------------------------------------------+----------------------------------------+
                                           |
+------------------------------------------v----------------------------------------+
|                                      gpck_core                                    |
+----+---------------------+--------------------+--------------------+--------------+
     |                     |                    |                    |
+----v-------------+ +-----v------------+ +-----v------------+ +-----v--------------+
| VFS / Storage    | | Codecs           | | GACL / Textures  | | GPU & Hardware     |
| - .gtoc / .gdat  | | - GDeflate (D3D) | | - Bc1–Bc7 Splits | | - DS 1.4 BypassIO  |
| - CHD O(1) Hash  | | - Brotli-G (GPU) | | - Morton Curves  | | - Vulkan Compute   |
| - XxHash64 Dedupe| | - Zstd (ATG/Std) | | - RDO / YCoCg    | | - 64KB Sparse Tile |
| - 32-Shard LRU   | | - AES-256-GCM    | | - .gmesh Meshlets| | - Timeline Sem.    |
+------------------+ +------------------+ +------------------+ +--------------------+
```
