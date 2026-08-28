# addons/gpck/gpck_plugin.gd
## Main EditorPlugin registering the complete GPCK ProjectSettings matrix and bottom dock panel.
@tool
extends EditorPlugin

const DOCK_SCRIPT_PATH = "res://addons/gpck/ui/gpck_dock.gd"

var dock_instance: Control
var export_plugin: EditorExportPlugin

func _enter_tree() -> void:
	# Register all GPCK configuration properties in ProjectSettings
	_register_project_settings()

	# Register automated build export plugin
	var ExportScript = load("res://addons/gpck/gpck_export_plugin.gd")
	if ExportScript:
		export_plugin = ExportScript.new()
		add_export_plugin(export_plugin)

	# Register bottom dock panel
	var DockScript = load(DOCK_SCRIPT_PATH)
	if DockScript:
		dock_instance = DockScript.new()
		add_control_to_bottom_panel(dock_instance, "GPCK Studio")

	print("[GPCK] Editor Integration & Full ProjectSettings matrix active.")

func _exit_tree() -> void:
	if export_plugin:
		remove_export_plugin(export_plugin)
		export_plugin = null
	if dock_instance:
		remove_control_from_bottom_panel(dock_instance)
		dock_instance.queue_free()
		dock_instance = null
	print("[GPCK] Plugin disabled.")

func _register_project_settings() -> void:
	# =========================================================================
	# Export & Packaging Pipeline
	# =========================================================================
	_add_setting("gpck/export/source_directory", "res://gc/", TYPE_STRING, PROPERTY_HINT_DIR, "Source directory containing assets to pack into VFS")
	_add_setting("gpck/export/archive_name", "game_data.gtoc", TYPE_STRING, PROPERTY_HINT_NONE, "Output archive TOC file name")
	_add_setting("gpck/export/compression_codec", "zstd", TYPE_STRING, PROPERTY_HINT_ENUM, "zstd,gdeflate,brotlig,lz4,rans,store", "Primary compression algorithm")
	_add_setting("gpck/export/compression_level", 9, TYPE_INT, PROPERTY_HINT_RANGE, "1,22,1", "Compression level (1..22 for Zstd, 1..11 for Brotli-G, 1..9 for GDeflate/LZ4)")
	_add_setting("gpck/export/partition_size_mb", 64, TYPE_INT, PROPERTY_HINT_ENUM, "32,64,128,256,512", "Maximum NVMe partition boundary in MB")
	_add_setting("gpck/export/encryption_passphrase", "", TYPE_STRING, PROPERTY_HINT_PASSWORD, "Master passphrase for AES-256-GCM metadata table encryption")
	_add_setting("gpck/export/strip_source_from_pck", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Exclude source folder files from standard .pck to minimize build size")

	# =========================================================================
	# Compression & Chunking
	# =========================================================================
	_add_setting("gpck/compression/atg_profile", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Enforce 256KB DirectStorage cache bounds for Zstd (WindowLog=18)")
	_add_setting("gpck/compression/enable_deduplication", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Enable content-defined chunk deduplication (XxHash64)")
	_add_setting("gpck/compression/validate_chunks", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Verify chunk decompression integrity immediately after packing")
	_add_setting("gpck/compression/chunk_size_kb", 64, TYPE_INT, PROPERTY_HINT_ENUM, "64,128,256", "Hardware sparse tile chunk size in KB (64 KB matches DirectStorage/Vulkan sparse page)")

	# =========================================================================
	# Texture Streaming & 64KB Sparse Tiles
	# =========================================================================
	_add_setting("gpck/streaming/tiled_streaming", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Enable 64KB Sparse Tile Packaging for Sampler Feedback Virtual Texturing")
	_add_setting("gpck/streaming/mip_split", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Split DDS textures into base tail mips and streamable .highmips files (when tiled_streaming is disabled)")
	_add_setting("gpck/streaming/max_tail_dimension", 128, TYPE_INT, PROPERTY_HINT_ENUM, "64,128,256,512", "Max resolution for instant-render boot partition tail mips")

	# =========================================================================
	# GACL (Game Asset Conditioning Library) & Texture Matrix
	# =========================================================================
	_add_setting("gpck/gacl/enabled", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Master Switch: Enable GACL bit-shuffling & space-curves")
	_add_setting("gpck/gacl/auto_mode", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Automatically benchmark and pick the optimal GACL transform per texture")

	# Granular Per-Format Overrides (Active when auto_mode is false)
	_add_setting("gpck/gacl/bc1_transform", "Auto", TYPE_STRING, PROPERTY_HINT_ENUM, "Auto,Disabled,BC1 Linear (v1),BC1 Linear + Z-Curve,BC1 5:6:5 Split (v2),BC1 5:6:5 + Z-Curve", "Transform override for BC1 (DXT1) RGB textures")
	_add_setting("gpck/gacl/bc2_transform", "Auto", TYPE_STRING, PROPERTY_HINT_ENUM, "Auto,Disabled,BC2 Alpha Nibble Split", "Transform override for BC2 (DXT3) textures")
	_add_setting("gpck/gacl/bc3_transform", "Auto", TYPE_STRING, PROPERTY_HINT_ENUM, "Auto,Disabled,BC3 Linear (v1),BC3 Linear + Z-Curve,BC3 6:6:4 Split (v2),BC3 6:6:4 + Z-Curve", "Transform override for BC3 (DXT5) RGBA textures")
	_add_setting("gpck/gacl/bc4_transform", "Auto", TYPE_STRING, PROPERTY_HINT_ENUM, "Auto,Disabled,BC4 Linear,BC4 Linear + Z-Curve", "Transform override for BC4 (ATI1) Grayscale/Height textures")
	_add_setting("gpck/gacl/bc5_transform", "Auto", TYPE_STRING, PROPERTY_HINT_ENUM, "Auto,Disabled,BC5 Dual Channel,BC5 Dual Channel + Z-Curve", "Transform override for BC5 (ATI2) Tangent Normal textures")
	_add_setting("gpck/gacl/bc6h_transform", "Auto", TYPE_STRING, PROPERTY_HINT_ENUM, "Auto,Disabled,BC6H Header/Index Join", "Transform override for BC6H HDR environment textures")
	_add_setting("gpck/gacl/bc7_transform", "Auto", TYPE_STRING, PROPERTY_HINT_ENUM, "Auto,Disabled,BC7 Mode-Split (3-Stream),BC7 Mode-Join (24-bit)", "Transform override for BC7 PBR ORM textures")

	# RDO (Rate-Distortion Optimization / BLER) Settings
	_add_setting("gpck/gacl/rdo_enabled", false, TYPE_BOOL, PROPERTY_HINT_NONE, "Enable Block-Level Entropy Reduction (BLER) via Lagrangian RDO")
	_add_setting("gpck/gacl/rdo_reduction_pct", 5.0, TYPE_FLOAT, PROPERTY_HINT_RANGE, "1,100,1", "Target entropy reduction percentage (higher = smaller file, more compression)")
	_add_setting("gpck/gacl/rdo_use_ycocg", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Use perceptual YCoCg color transform for Albedo/Diffuse channels")

	# Per-Format RDO Filters
	_add_setting("gpck/gacl/rdo_bc1", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Allow RDO on BC1 textures")
	_add_setting("gpck/gacl/rdo_bc2", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Allow RDO on BC2 textures")
	_add_setting("gpck/gacl/rdo_bc3", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Allow RDO on BC3 textures")
	_add_setting("gpck/gacl/rdo_bc4", false, TYPE_BOOL, PROPERTY_HINT_NONE, "Allow RDO on BC4 textures (Disabled by default for precision)")
	_add_setting("gpck/gacl/rdo_bc5", false, TYPE_BOOL, PROPERTY_HINT_NONE, "Allow RDO on BC5 Normal maps (Disabled by default to prevent normal distortion)")
	_add_setting("gpck/gacl/rdo_bc6h", false, TYPE_BOOL, PROPERTY_HINT_NONE, "Allow RDO on BC6H HDR maps (Disabled by default)")
	_add_setting("gpck/gacl/rdo_bc7", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Allow RDO on BC7 PBR maps")

	# =========================================================================
	# DirectStorage & GPU Acceleration
	# =========================================================================
	_add_setting("gpck/directstorage/prefer_gpu_decompression", true, TYPE_BOOL, PROPERTY_HINT_NONE, "Enable Direct-to-VRAM GPU Decompression over PCIe DMA")
	_add_setting("gpck/directstorage/staging_buffer_size_mb", 256, TYPE_INT, PROPERTY_HINT_ENUM, "64,128,256,512", "DirectStorage Staging Buffer capacity in MB")
	_add_setting("gpck/directstorage/default_queue_priority", "Normal", TYPE_STRING, PROPERTY_HINT_ENUM, "Normal,High,Low", "Default hardware queue priority tier")

func _add_setting(name: String, default_val: Variant, type: int, hint: int = PROPERTY_HINT_NONE, hint_string: String = "", description: String = "") -> void:
	if not ProjectSettings.has_setting(name):
		ProjectSettings.set_setting(name, default_val)
	ProjectSettings.set_initial_value(name, default_val)

	var prop_info: Dictionary = {
		"name": name,
		"type": type,
		"hint": hint,
		"hint_string": hint_string
	}
	if not description.is_empty():
		prop_info["description"] = description

	ProjectSettings.add_property_info(prop_info)
