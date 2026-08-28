# addons/gpck/gpck_export_plugin.gd
## Automated export plugin that packs VFS archives using all ProjectSettings options and strips raw source assets from .pck.
@tool
extends EditorExportPlugin

func _get_name() -> String:
	return "GPCKAutomatedExportPlugin"

func _export_begin(features: PackedStringArray, is_debug: bool, path: String, flags: int) -> void:
	var export_dir: String = path.get_base_dir()

	# Read target archive and source directory
	var arch_name: String = ProjectSettings.get_setting("gpck/export/archive_name", "game_data.gtoc")
	var out_gtoc: String = export_dir.path_join(arch_name)
	var source_dir_setting: String = ProjectSettings.get_setting("gpck/export/source_directory", "res://gc/")
	var in_dir: String = ProjectSettings.globalize_path(source_dir_setting)

	if not DirAccess.dir_exists_absolute(in_dir):
		printerr("[GPCK Export] Source directory does not exist on disk: ", in_dir)
		return

	# Build full configuration dictionary from ProjectSettings
	var options: Dictionary = {
		# Export & Codec
		"method": ProjectSettings.get_setting("gpck/export/compression_codec", "zstd"),
		"level": int(ProjectSettings.get_setting("gpck/export/compression_level", 9)),
		"partition_size_mb": int(ProjectSettings.get_setting("gpck/export/partition_size_mb", 64)),
		"passphrase": ProjectSettings.get_setting("gpck/export/encryption_passphrase", ""),

		# Compression & Chunks
		"atg_profile": bool(ProjectSettings.get_setting("gpck/compression/atg_profile", true)),
		"enable_deduplication": bool(ProjectSettings.get_setting("gpck/compression/enable_deduplication", true)),
		"validate_chunks": bool(ProjectSettings.get_setting("gpck/compression/validate_chunks", true)),
		"chunk_size_kb": int(ProjectSettings.get_setting("gpck/compression/chunk_size_kb", 64)),

		# Streaming
		"tiled_streaming": bool(ProjectSettings.get_setting("gpck/streaming/tiled_streaming", true)),
		"mip_split": bool(ProjectSettings.get_setting("gpck/streaming/mip_split", true)),
		"max_tail_dimension": int(ProjectSettings.get_setting("gpck/streaming/max_tail_dimension", 128)),

		# GACL Matrix & Master Switch
		"gacl_enabled": bool(ProjectSettings.get_setting("gpck/gacl/enabled", true)),
		"gacl_auto_mode": bool(ProjectSettings.get_setting("gpck/gacl/auto_mode", true)),
		"bc1_transform": ProjectSettings.get_setting("gpck/gacl/bc1_transform", "Auto"),
		"bc2_transform": ProjectSettings.get_setting("gpck/gacl/bc2_transform", "Auto"),
		"bc3_transform": ProjectSettings.get_setting("gpck/gacl/bc3_transform", "Auto"),
		"bc4_transform": ProjectSettings.get_setting("gpck/gacl/bc4_transform", "Auto"),
		"bc5_transform": ProjectSettings.get_setting("gpck/gacl/bc5_transform", "Auto"),
		"bc6h_transform": ProjectSettings.get_setting("gpck/gacl/bc6h_transform", "Auto"),
		"bc7_transform": ProjectSettings.get_setting("gpck/gacl/bc7_transform", "Auto"),

		# RDO & Format Filters
		"rdo_enabled": bool(ProjectSettings.get_setting("gpck/gacl/rdo_enabled", false)),
		"rdo_reduction_pct": float(ProjectSettings.get_setting("gpck/gacl/rdo_reduction_pct", 5.0)),
		"rdo_use_ycocg": bool(ProjectSettings.get_setting("gpck/gacl/rdo_use_ycocg", true)),
		"rdo_bc1": bool(ProjectSettings.get_setting("gpck/gacl/rdo_bc1", true)),
		"rdo_bc2": bool(ProjectSettings.get_setting("gpck/gacl/rdo_bc2", true)),
		"rdo_bc3": bool(ProjectSettings.get_setting("gpck/gacl/rdo_bc3", true)),
		"rdo_bc4": bool(ProjectSettings.get_setting("gpck/gacl/rdo_bc4", false)),
		"rdo_bc5": bool(ProjectSettings.get_setting("gpck/gacl/rdo_bc5", false)),
		"rdo_bc6h": bool(ProjectSettings.get_setting("gpck/gacl/rdo_bc6h", false)),
		"rdo_bc7": bool(ProjectSettings.get_setting("gpck/gacl/rdo_bc7", true)),
	}

	print("[GPCK Export] ==========================================")
	print("[GPCK Export] Automating GPCK asset package compilation...")
	print("[GPCK Export] Source Directory : ", in_dir)
	print("[GPCK Export] Output Archive   : ", out_gtoc)
	print("[GPCK Export] Codec / Level    : ", options["method"].to_upper(), " (Level ", options["level"], ")")
	print("[GPCK Export] 64KB Tile Stream : ", "Enabled" if options["tiled_streaming"] else "Disabled")
	print("[GPCK Export] GACL Shuffling   : ", "Enabled" if options["gacl_enabled"] else "Disabled (Raw Passthrough)")
	print("[GPCK Export] RDO Reduction    : ", "Enabled (" + str(options["rdo_reduction_pct"]) + "%)" if options["rdo_enabled"] else "Disabled")
	print("[GPCK Export] AES Encryption   : ", "Enabled" if options["passphrase"] != "" else "Disabled")

	var vfs: GpckVfs = GpckVfs.new()
	var success: bool = vfs.pack_directory_with_options(in_dir, out_gtoc, options)

	if success:
		print("[GPCK Export] Archive package created successfully in release directory.")
	else:
		printerr("[GPCK Export] Failed to build asset package! Check console output for details.")
	print("[GPCK Export] ==========================================")

func _export_file(path: String, type: String, features: PackedStringArray) -> void:
	var should_strip: bool = ProjectSettings.get_setting("gpck/export/strip_source_from_pck", true)
	if not should_strip:
		return

	var source_dir: String = ProjectSettings.get_setting("gpck/export/source_directory", "res://gc/")
	if path.begins_with(source_dir):
		# Exclude raw asset files from standard .pck since they are bundled in .gtoc/.gdat
		skip()
