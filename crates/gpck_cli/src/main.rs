// crates/gpck_cli/src/main.rs
use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use gpck_core::benchmark;
use gpck_core::compression::codecs::CompressionMethod;
use gpck_core::core::{crash_handler, logger};
use gpck_core::crypto::aes_gcm::derive_key;
use gpck_core::format::archive::{GameArchive, TAG_BASE_GAME};
use gpck_core::packer::{
    AssetPacker, DEFAULT_MAX_PARTITION_SIZE, GaclFormatOverrides, NtcPackerOptions, PackerOptions,
};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "gpck",
    author = "GPCK Contributors",
    version = "0.2.0",
    about = "GPCK High-Performance Asset Packaging Toolkit & VFS Engine"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(alias = "pack")]
    Compress {
        #[arg(value_name = "INPUT")]
        input: PathBuf,
        #[arg(value_name = "OUTPUT")]
        output: Option<PathBuf>,
        #[arg(short, long, default_value = "zstd")]
        method: String,
        #[arg(short, long, default_value_t = 9)]
        level: i32,
        #[arg(long, default_value_t = true)]
        mip_split: bool,
        #[arg(long, default_value_t = true)]
        tiled: bool,
        #[arg(long, default_value_t = 2048)]
        min_tiled_res: usize,
        #[arg(long, default_value_t = 8)]
        min_tiled_count: u32,
        #[arg(long, default_value_t = true)]
        validate: bool,
        #[arg(long, default_value_t = true)]
        atg: bool,
        #[arg(long, default_value_t = DEFAULT_MAX_PARTITION_SIZE)]
        partition_size: usize,
        #[arg(long, default_value_t = TAG_BASE_GAME)]
        tags: u32,
        #[arg(short, long)]
        key: Option<String>,
    },
    #[command(alias = "unpack")]
    Decompress {
        #[arg(value_name = "ARCHIVE")]
        archive: PathBuf,
        #[arg(value_name = "OUTPUT")]
        output: Option<PathBuf>,
        #[arg(short, long)]
        key: Option<String>,
    },
    Verify {
        #[arg(value_name = "ARCHIVE")]
        archive: PathBuf,
        #[arg(short, long)]
        key: Option<String>,
    },
    Info {
        #[arg(value_name = "ARCHIVE")]
        archive: PathBuf,
    },
    Bench {
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let _log_guard = logger::init_logger();
    crash_handler::setup_crash_handler();

    let args = Cli::parse();

    match args.command {
        Commands::Compress {
            input,
            output,
            method,
            level,
            mip_split,
            tiled,
            min_tiled_res,
            min_tiled_count,
            validate,
            atg,
            partition_size,
            tags,
            key,
        } => {
            let output_path = output.unwrap_or_else(|| input.with_extension("gtoc"));
            let key_bytes = key.as_deref().map(derive_key);
            let comp_method = match method.to_lowercase().as_str() {
                "store" => CompressionMethod::Store,
                "lz4" => CompressionMethod::Lz4,
                "zstd" => CompressionMethod::Zstd,
                "rans" => CompressionMethod::Rans,
                "brotli" | "brotlig" | "brotli_g" => CompressionMethod::BrotliG,
                _ => CompressionMethod::GDeflate,
            };

            let options = PackerOptions {
                method: comp_method,
                level,
                chunk_size: gpck_core::packer::DEFAULT_CHUNK_SIZE,
                enable_dedup: true,
                key: key_bytes,
                mip_split,
                max_tail_dim: 128,
                tags,
                validate_chunks: validate,
                max_partition_size: partition_size,
                gacl: GaclFormatOverrides::default(),
                ntc: NtcPackerOptions::default(),
                atg_profile: atg,
                tiled_streaming: tiled,
                min_tiled_resolution: min_tiled_res,
                min_tiled_tile_count: min_tiled_count,
            };

            let file_map = AssetPacker::build_file_map(&input)?;
            AssetPacker::compress_files_to_archive(&file_map, &output_path, &options, |msg| {
                println!("{}", msg);
            })?;
            println!("Packaging completed successfully: {:?}", output_path);
            Ok(())
        }
        Commands::Decompress {
            archive,
            output,
            key,
        } => {
            let target_dir = output.unwrap_or_else(|| PathBuf::from("extracted"));
            let key_bytes = key.as_deref().map(derive_key);

            let mut arch = GameArchive::open(&archive)?;
            arch.decryption_key = key_bytes;

            fs::create_dir_all(&target_dir)?;
            let entries = arch.get_all_entries()?;

            for entry in &entries {
                let rel_path = arch
                    .get_path_for_asset(entry)
                    .unwrap_or_else(|| Uuid::from_bytes(entry.asset_id).to_string());
                let out_file = target_dir.join(&rel_path);
                if let Some(parent) = out_file.parent() {
                    fs::create_dir_all(parent)?;
                }
                let data = arch.read_asset(entry)?;
                fs::write(out_file, data)?;
            }
            println!("Decompressed {} files into {:?}", entries.len(), target_dir);
            Ok(())
        }
        Commands::Verify { archive, key } => {
            let key_bytes = key.as_deref().map(derive_key);
            let mut arch = GameArchive::open(&archive)?;
            arch.decryption_key = key_bytes;

            let entries = arch.get_all_entries()?;
            let mut errors = 0;

            for entry in &entries {
                if arch.read_asset(entry).is_err() {
                    errors += 1;
                }
            }

            if errors == 0 {
                println!(
                    "[SUCCESS] Verification PASSED. All {} assets intact.",
                    entries.len()
                );
                Ok(())
            } else {
                bail!(
                    "[FAILED] Verification FAILED with {} corrupted assets.",
                    errors
                );
            }
        }
        Commands::Info { archive } => {
            let arch = GameArchive::open(&archive)?;
            let entries = arch.get_all_entries()?;
            println!("Archive: {:?}", archive);
            println!(
                "Total Uncompressed Size: {} bytes",
                arch.total_uncompressed_size()
            );
            println!("Total Assets in TOC: {}", entries.len());
            Ok(())
        }
        Commands::Bench { path } => {
            let res = benchmark::run_benchmark_suite_string(path.as_deref())?;
            println!("{}", res);
            Ok(())
        }
    }
}
