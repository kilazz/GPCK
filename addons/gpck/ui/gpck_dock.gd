# addons/gpck/ui/gpck_dock.gd
@tool
extends VBoxContainer

var in_dir_edit: LineEdit
var out_path_edit: LineEdit
var method_opt: OptionButton
var level_slider: HSlider
var level_label: Label
var pass_edit: LineEdit
var use_matrix_check: CheckBox
var pack_btn: Button
var status_label: Label

var tree: Tree
var refresh_btn: Button
var stats_label: Label

func _init() -> void:
	custom_minimum_size = Vector2(0, 240)

	var tabs = TabContainer.new()
	tabs.size_flags_vertical = Control.SIZE_EXPAND_FILL
	add_child(tabs)

	# --- TAB 1: Package Builder ---
	var pack_tab = VBoxContainer.new()
	pack_tab.name = "📦 Package Builder"
	tabs.add_child(pack_tab)

	var grid = GridContainer.new()
	grid.columns = 2
	pack_tab.add_child(grid)

	grid.add_child(_make_label("Source Directory (res://):"))
	in_dir_edit = LineEdit.new()
	in_dir_edit.text = ProjectSettings.get_setting("gpck/export/source_directory", "res://gc/")
	in_dir_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	grid.add_child(in_dir_edit)

	grid.add_child(_make_label("Output Archive (res://):"))
	out_path_edit = LineEdit.new()
	out_path_edit.text = "res://" + ProjectSettings.get_setting("gpck/export/archive_name", "game_data.gtoc")
	out_path_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	grid.add_child(out_path_edit)

	grid.add_child(_make_label("Compression Codec:"))
	method_opt = OptionButton.new()
	method_opt.add_item("Zstd (DirectStorage ATG)", 0)
	method_opt.add_item("GDeflate (GPU Metacommand)", 1)
	method_opt.add_item("Brotli-G (AMD GPUOpen)", 2)
	method_opt.add_item("LZ4 (Fast Mobile)", 3)
	method_opt.add_item("rANS (4-Way Interleaved)", 4)
	method_opt.add_item("Store (Uncompressed)", 5)

	var current_codec = ProjectSettings.get_setting("gpck/export/compression_codec", "zstd").to_lower()
	match current_codec:
		"gdeflate": method_opt.selected = 1
		"brotlig", "brotli": method_opt.selected = 2
		"lz4": method_opt.selected = 3
		"rans": method_opt.selected = 4
		"store": method_opt.selected = 5
		_: method_opt.selected = 0
	grid.add_child(method_opt)

	grid.add_child(_make_label("Compression Level:"))
	var slider_box = HBoxContainer.new()
	level_slider = HSlider.new()
	level_slider.min_value = 1
	level_slider.max_value = 22
	level_slider.value = ProjectSettings.get_setting("gpck/export/compression_level", 9)
	level_slider.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	level_label = Label.new()
	level_label.text = " %d" % level_slider.value
	level_slider.value_changed.connect(func(v): level_label.text = " %d" % v)
	slider_box.add_child(level_slider)
	slider_box.add_child(level_label)
	grid.add_child(slider_box)

	grid.add_child(_make_label("Passphrase (AES-256):"))
	pass_edit = LineEdit.new()
	pass_edit.text = ProjectSettings.get_setting("gpck/export/encryption_passphrase", "")
	pass_edit.placeholder_text = "Leave empty for unencrypted package"
	grid.add_child(pass_edit)

	use_matrix_check = CheckBox.new()
	use_matrix_check.text = "Apply Full ProjectSettings Matrix (GACL Shuffling, RDO & 64KB Tiles)"
	use_matrix_check.button_pressed = true
	pack_tab.add_child(use_matrix_check)

	pack_btn = Button.new()
	pack_btn.text = "🚀 Pack Archive Now"
	pack_btn.pressed.connect(_on_pack_pressed)
	pack_tab.add_child(pack_btn)

	status_label = Label.new()
	status_label.text = "Ready."
	pack_tab.add_child(status_label)

	# --- TAB 2: VFS Inspector ---
	var inspect_tab = VBoxContainer.new()
	inspect_tab.name = "🔍 VFS Inspector"
	tabs.add_child(inspect_tab)

	var top_bar = HBoxContainer.new()
	inspect_tab.add_child(top_bar)

	refresh_btn = Button.new()
	refresh_btn.text = "🔄 Refresh Archive Entries"
	refresh_btn.pressed.connect(_on_refresh_inspector)
	top_bar.add_child(refresh_btn)

	stats_label = Label.new()
	stats_label.text = " Click refresh to inspect mounted archive."
	top_bar.add_child(stats_label)

	tree = Tree.new()
	tree.size_flags_vertical = Control.SIZE_EXPAND_FILL
	tree.columns = 6
	tree.set_column_titles_visible(true)
	tree.set_column_title(0, "Asset Path")
	tree.set_column_title(1, "Method")
	tree.set_column_title(2, "GACL")
	tree.set_column_title(3, "Original Size")
	tree.set_column_title(4, "Packed Size")
	tree.set_column_title(5, "Ratio")
	inspect_tab.add_child(tree)

func _make_label(txt: String) -> Label:
	var l = Label.new()
	l.text = txt
	return l

func _on_pack_pressed() -> void:
	status_label.text = "Packing assets into archive..."
	var methods = ["zstd", "gdeflate", "brotlig", "lz4", "rans", "store"]
	var selected_method = methods[method_opt.selected]

	var in_global: String = ProjectSettings.globalize_path(in_dir_edit.text)
	var out_global: String = ProjectSettings.globalize_path(out_path_edit.text)

	var vfs = GpckVfs.new()
	var success = false

	if use_matrix_check.button_pressed:
		var options: Dictionary = {
			"method": selected_method,
			"level": int(level_slider.value),
			"partition_size_mb": int(ProjectSettings.get_setting("gpck/export/partition_size_mb", 64)),
			"passphrase": pass_edit.text,
			"atg_profile": bool(ProjectSettings.get_setting("gpck/compression/atg_profile", true)),
			"enable_deduplication": bool(ProjectSettings.get_setting("gpck/compression/enable_deduplication", true)),
			"validate_chunks": bool(ProjectSettings.get_setting("gpck/compression/validate_chunks", true)),
			"chunk_size_kb": int(ProjectSettings.get_setting("gpck/compression/chunk_size_kb", 64)),
			"tiled_streaming": bool(ProjectSettings.get_setting("gpck/streaming/tiled_streaming", true)),
			"mip_split": bool(ProjectSettings.get_setting("gpck/streaming/mip_split", true)),
			"max_tail_dimension": int(ProjectSettings.get_setting("gpck/streaming/max_tail_dimension", 128)),
			"gacl_enabled": bool(ProjectSettings.get_setting("gpck/gacl/enabled", true)),
			"gacl_auto_mode": bool(ProjectSettings.get_setting("gpck/gacl/auto_mode", true)),
			"bc1_transform": ProjectSettings.get_setting("gpck/gacl/bc1_transform", "Auto"),
			"bc2_transform": ProjectSettings.get_setting("gpck/gacl/bc2_transform", "Auto"),
			"bc3_transform": ProjectSettings.get_setting("gpck/gacl/bc3_transform", "Auto"),
			"bc4_transform": ProjectSettings.get_setting("gpck/gacl/bc4_transform", "Auto"),
			"bc5_transform": ProjectSettings.get_setting("gpck/gacl/bc5_transform", "Auto"),
			"bc6h_transform": ProjectSettings.get_setting("gpck/gacl/bc6h_transform", "Auto"),
			"bc7_transform": ProjectSettings.get_setting("gpck/gacl/bc7_transform", "Auto"),
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
		success = vfs.pack_directory_with_options(in_global, out_global, options)
	else:
		success = vfs.pack_directory(
			in_global,
			out_global,
			selected_method,
			int(level_slider.value),
			pass_edit.text
		)

	if success:
		status_label.text = "✅ Archive built successfully: %s" % out_path_edit.text
		_on_refresh_inspector()
	else:
		status_label.text = "❌ Packing failed. Check Godot Output/Console for details."

func _on_refresh_inspector() -> void:
	tree.clear()
	var root = tree.create_item()

	var out_global: String = ProjectSettings.globalize_path(out_path_edit.text)

	var vfs = GpckVfs.new()
	vfs.mount_archive(out_global, pass_edit.text)
	var entries = vfs.get_archive_entries()

	var total_orig = 0
	var total_comp = 0

	for e in entries:
		var item = tree.create_item(root)
		item.set_text(0, e.get("path", ""))
		item.set_text(1, e.get("method", ""))
		item.set_text(2, e.get("gacl", ""))

		var orig = e.get("original_size", 0)
		var comp = e.get("compressed_size", 0)
		total_orig += orig
		total_comp += comp

		item.set_text(3, _format_bytes(orig))
		item.set_text(4, _format_bytes(comp))
		item.set_text(5, "%.1f%%" % e.get("ratio", 100.0))

	var overall_ratio = (float(total_comp) / float(total_orig) * 100.0) if total_orig > 0 else 100.0
	stats_label.text = " Assets: %d | Orig: %s | Packed: %s | Ratio: %.1f%%" % [
		entries.size(), _format_bytes(total_orig), _format_bytes(total_comp), overall_ratio
	]

func _format_bytes(b: int) -> String:
	if b < 1024: return "%d B" % b
	if b < 1024 * 1024: return "%.1f KB" % (b / 1024.0)
	return "%.2f MB" % (b / (1024.0 * 1024.0))
