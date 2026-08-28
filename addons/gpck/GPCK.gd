# res://addons/gpck/GPCK.gd
## High-Level Global VFS Facade & Singleton for GPCK
## Provides fast, single-call access to streaming textures, meshlets, audio, JSON data, and DirectStorage GPU queues.
@tool
extends Node

var _vfs_instance: GpckVfs

## Returns the underlying GpckVfs reference instance.
func get_vfs() -> GpckVfs:
	if not _vfs_instance:
		_vfs_instance = GpckVfs.new()
	return _vfs_instance

## Mounts a .gtoc archive into the virtual filesystem with an optional AES-256-GCM passphrase.
func mount_archive(path: String, passphrase: String = "") -> Error:
	var global_path: String = ProjectSettings.globalize_path(path)
	return get_vfs().mount_archive(global_path, passphrase) as Error

## Mounts a loose directory on disk into the virtual filesystem.
func mount_directory(path: String) -> Error:
	var global_path: String = ProjectSettings.globalize_path(path)
	return get_vfs().mount_directory(global_path) as Error

## Checks if an asset exists in any mounted VFS archive or directory.
func has_file(virtual_path: String) -> bool:
	return get_vfs().has_file(virtual_path)

## Reads raw uncompressed asset bytes synchronously into a PackedByteArray.
func read_file(virtual_path: String) -> PackedByteArray:
	return get_vfs().read_file(virtual_path)

## Reads a text-based asset as a UTF-8 string.
func read_text(virtual_path: String) -> String:
	return get_vfs().read_text(virtual_path)

## Reads and parses a JSON asset directly from the VFS archive.
func read_json(virtual_path: String) -> Variant:
	var text: String = read_text(virtual_path)
	if text.is_empty():
		return null
	var json: JSON = JSON.new()
	var error: Error = json.parse(text)
	if error == OK:
		return json.data
	push_error("[GPCK] Failed to parse JSON '%s': %s" % [virtual_path, json.get_error_message()])
	return null

## Loads a Texture2D (BC1-BC7, KTX2, or Mip-Recombined DDS) with automatic relaxed path resolution.
func get_texture(virtual_path: String) -> Texture2D:
	var res: Resource = load(virtual_path)
	if res is Texture2D:
		return res as Texture2D
	return null

## Loads an ArrayMesh (.gmesh / .dgf / .gdmm) with automatic relaxed path resolution.
func get_mesh(virtual_path: String) -> ArrayMesh:
	var res: Resource = load(virtual_path)
	if res is ArrayMesh:
		return res as ArrayMesh
	return null

## Loads an audio stream (.wav / .ogg / .mp3) with automatic relaxed path resolution.
func get_audio(virtual_path: String) -> Resource:
	return load(virtual_path)

## Initiates non-blocking background streaming for an asset via Godot's ResourceLoader.
func preload_asset_async(virtual_path: String) -> Error:
	var path_to_load: String = virtual_path if virtual_path.begins_with("res://") else ("res://" + virtual_path)
	return ResourceLoader.load_threaded_request(path_to_load)

# =============================================================================
# DirectStorage 1.4 GPU Streaming APIs
# =============================================================================

## Returns true if DirectStorage 1.4 hardware offload (BypassIO) is active on the current host.
func is_directstorage_supported() -> bool:
	return get_vfs().is_directstorage_supported()

## Streams a specific 64KB sparse tile directly from NVMe into a D3D12 Reserved Tiled Resource.
func stream_tile_to_d3d12(
	virtual_path: String,
	d3d12_texture_ptr: int,
	subresource: int,
	tile_x: int,
	tile_y: int,
	tile_z: int = 0,
	priority: int = 1
) -> Dictionary:
	return get_vfs().stream_tile_to_d3d12(virtual_path, d3d12_texture_ptr, subresource, tile_x, tile_y, tile_z, priority)

## Waits on the CPU until the specified DirectStorage hardware queue signals the fence.
func wait_for_d3d12_fence(priority: int, fence_value: int) -> bool:
	return get_vfs().wait_for_d3d12_fence(priority, fence_value)

# =============================================================================
# Packaging & Telemetry
# =============================================================================

## Packs a source directory into a .gtoc + .gdat package with standard options.
func pack_directory(
	in_dir: String,
	out_archive: String,
	method: String = "zstd",
	level: int = 9,
	passphrase: String = ""
) -> bool:
	var in_global: String = ProjectSettings.globalize_path(in_dir)
	var out_global: String = ProjectSettings.globalize_path(out_archive)
	return get_vfs().pack_directory(in_global, out_global, method, level, passphrase)

## Packs a source directory into a .gtoc + .gdat package with custom Dictionary options.
func pack_directory_with_options(
	in_dir: String,
	out_archive: String,
	options_dict: Dictionary
) -> bool:
	var in_global: String = ProjectSettings.globalize_path(in_dir)
	var out_global: String = ProjectSettings.globalize_path(out_archive)
	return get_vfs().pack_directory_with_options(in_global, out_global, options_dict)

## Returns metadata dictionaries for all files across currently mounted packages.
func get_archive_entries() -> Array:
	return get_vfs().get_archive_entries()
