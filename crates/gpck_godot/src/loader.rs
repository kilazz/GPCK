// crates/gpck_godot/src/loader.rs
//! # GPCK Native Godot 4 ResourceFormatLoader
//!
//! Intercepts Godot's `load()` and `ResourceLoader::load_threaded_request()`,
//! streaming textures (BC1–BC7/HDR), meshlet geometry, audio, and JSON data
//! directly from mounted GPCK VFS archives without CPU staging bottlenecks.

use crate::vfs::get_global_vfs;
use godot::classes::image::Format;
use godot::classes::mesh::{ArrayType, PrimitiveType};
use godot::classes::{
    ArrayMesh, AudioStreamMp3, AudioStreamOggVorbis, AudioStreamWav, IResourceFormatLoader, Image,
    ImageTexture, Json, Resource, ResourceFormatLoader,
};
use godot::prelude::*;

use gpck_core::core::asset_id::AssetIdGenerator;
use gpck_core::format::dds::DdsUtils;
use gpck_core::geometry::dgf::{DGF_BLOCK_SIZE, DgfDecoder};
use gpck_core::geometry::meshlet::{
    MESHLET_MAGIC, MeshletContainerHeader, MeshletDescriptor, MeshletTriangle, QuantizedVertex,
};
use gpck_core::geometry::octahedral::{decode_octahedral_normal, f16_to_f32};
use gpck_core::graphics::dxgi_format::dxgi;
use gpck_core::graphics::recombine::TextureRecombiner;

#[derive(GodotClass)]
#[class(base=ResourceFormatLoader)]
pub struct GpckResourceFormatLoader {
    base: Base<ResourceFormatLoader>,
}

#[godot_api]
impl IResourceFormatLoader for GpckResourceFormatLoader {
    fn init(base: Base<ResourceFormatLoader>) -> Self {
        Self { base }
    }

    fn get_recognized_extensions(&self) -> PackedStringArray {
        let mut exts = PackedStringArray::new();
        exts.push(&GString::from("dds"));
        exts.push(&GString::from("ktx2"));
        exts.push(&GString::from("gmesh"));
        exts.push(&GString::from("gdmm"));
        exts.push(&GString::from("dgf"));
        exts.push(&GString::from("highmips"));
        exts.push(&GString::from("wav"));
        exts.push(&GString::from("ogg"));
        exts.push(&GString::from("mp3"));
        exts.push(&GString::from("json"));
        exts.push(&GString::from("txt"));
        exts
    }

    fn handles_type(&self, type_name: StringName) -> bool {
        let name_str = type_name.to_string();
        name_str.is_empty()
            || name_str == "Resource"
            || name_str == "Texture"
            || name_str == "Texture2D"
            || name_str == "ImageTexture"
            || name_str == "CompressedTexture2D"
            || name_str == "Mesh"
            || name_str == "ArrayMesh"
            || name_str == "AudioStream"
            || name_str == "AudioStreamWav"
            || name_str == "AudioStreamOggVorbis"
            || name_str == "AudioStreamMP3"
            || name_str == "JSON"
    }

    fn get_resource_type(&self, path: GString) -> GString {
        let path_lower = path.to_string().to_lowercase();
        if path_lower.ends_with(".dds")
            || path_lower.ends_with(".ktx2")
            || path_lower.ends_with(".highmips")
        {
            GString::from("Texture2D")
        } else if path_lower.ends_with(".gmesh")
            || path_lower.ends_with(".gdmm")
            || path_lower.ends_with(".dgf")
            || path_lower.ends_with(".obj")
        {
            GString::from("ArrayMesh")
        } else if path_lower.ends_with(".wav") {
            GString::from("AudioStreamWav")
        } else if path_lower.ends_with(".ogg") {
            GString::from("AudioStreamOggVorbis")
        } else if path_lower.ends_with(".mp3") {
            GString::from("AudioStreamMP3")
        } else if path_lower.ends_with(".json") {
            GString::from("JSON")
        } else {
            GString::new()
        }
    }

    fn get_resource_uid(&self, path: GString) -> i64 {
        let path_str = path.to_string();
        let id = AssetIdGenerator::generate(&path_str);
        let id_bytes = id.as_bytes();
        let uid_raw = u64::from_le_bytes(id_bytes[0..8].try_into().unwrap_or([0; 8]));
        (uid_raw & 0x7FFF_FFFF_FFFF_FFFF) as i64
    }

    fn recognize_path(&self, path: GString, _type_hint: StringName) -> bool {
        let path_str = path.to_string().to_lowercase();
        path_str.ends_with(".dds")
            || path_str.ends_with(".ktx2")
            || path_str.ends_with(".gmesh")
            || path_str.ends_with(".gdmm")
            || path_str.ends_with(".dgf")
            || path_str.ends_with(".highmips")
            || path_str.ends_with(".wav")
            || path_str.ends_with(".ogg")
            || path_str.ends_with(".mp3")
            || path_str.ends_with(".json")
            || path_str.ends_with(".txt")
    }

    fn exists(&self, path: GString) -> bool {
        let path_str = path.to_string();
        let vfs = get_global_vfs();
        if let Ok(guard) = vfs.read() {
            guard.find_entry_relaxed(&path_str).is_some()
        } else {
            false
        }
    }

    fn get_dependencies(&self, path: GString, _add_types: bool) -> PackedStringArray {
        let mut deps = PackedStringArray::new();
        let path_str = path.to_string();
        if path_str.to_lowercase().ends_with(".dds") {
            let highmips_path = format!("{}.highmips", path_str);
            deps.push(&GString::from(highmips_path));
        }
        deps
    }

    fn load(
        &self,
        path: GString,
        _original_path: GString,
        _use_sub_threads: bool,
        _cache_mode: i32,
    ) -> Variant {
        let path_str = path.to_string();
        let path_lower = path_str.to_lowercase();

        // 1. Textures (.dds / .ktx2 / .highmips)
        if (path_lower.ends_with(".dds")
            || path_lower.ends_with(".ktx2")
            || path_lower.ends_with(".highmips"))
            && let Some(tex) = Self::load_texture_from_vfs(&path_str)
        {
            return tex.to_variant();
        }

        // 2. Geometry & Meshlets (.gmesh / .dgf / .gdmm)
        if (path_lower.ends_with(".gmesh")
            || path_lower.ends_with(".dgf")
            || path_lower.ends_with(".gdmm"))
            && let Some(mesh) = Self::load_mesh_from_vfs(&path_str)
        {
            return mesh.to_variant();
        }

        // 3. Audio Streams (.wav / .ogg / .mp3)
        if (path_lower.ends_with(".wav")
            || path_lower.ends_with(".ogg")
            || path_lower.ends_with(".mp3"))
            && let Some(audio) = Self::load_audio_from_vfs(&path_str)
        {
            return audio.to_variant();
        }

        // 4. JSON Data (.json)
        if path_lower.ends_with(".json")
            && let Some(json_res) = Self::load_json_from_vfs(&path_str)
        {
            return json_res.to_variant();
        }

        Variant::nil()
    }
}

impl GpckResourceFormatLoader {
    fn load_texture_from_vfs(virtual_path: &str) -> Option<Gd<ImageTexture>> {
        let clean_path = virtual_path.trim_start_matches("res://");
        let vfs = get_global_vfs();
        let guard = vfs.read().ok()?;

        let (entry, archive) = guard.find_entry_relaxed(virtual_path)?;
        let raw_base = archive.read_asset(&entry).ok()?;

        let highmips_path = format!("{}.highmips", virtual_path);
        let (high_raw, high_transform) = if let Some((high_entry, high_arch)) =
            guard.find_entry_relaxed(&highmips_path)
            && let Ok(high_bytes) = high_arch.read_asset(&high_entry)
        {
            (Some(high_bytes), high_entry.gacl_transform())
        } else {
            (None, 0)
        };

        let full_dds = TextureRecombiner::recombine_dds(
            clean_path,
            &raw_base,
            high_raw.as_deref(),
            &entry,
            high_transform,
            true,
        )
        .ok()?;

        let (dxgi_fmt, header_len) = DdsUtils::detect_dxgi_format(&full_dds);
        let h_info = DdsUtils::get_header_info(&full_dds)?;

        let godot_format = match dxgi_fmt {
            dxgi::BC1_UNORM | dxgi::BC1_UNORM_SRGB => Some(Format::DXT1),
            dxgi::BC2_UNORM | dxgi::BC2_UNORM_SRGB => Some(Format::DXT3),
            dxgi::BC3_UNORM | dxgi::BC3_UNORM_SRGB => Some(Format::DXT5),
            dxgi::BC4_UNORM => Some(Format::RGTC_R),
            dxgi::BC5_UNORM => Some(Format::RGTC_RG),
            dxgi::BC7_UNORM | dxgi::BC7_UNORM_SRGB => Some(Format::BPTC_RGBA),
            dxgi::BC6H_UF16 | dxgi::BC6H_SF16 => Some(Format::BPTC_RGBF),
            _ => None,
        };

        let fmt = godot_format?;
        let payload = &full_dds[header_len..];
        let packed = PackedByteArray::from(payload);
        let has_mips = h_info.mip_count > 1;

        let image = Image::create_from_data(
            h_info.width as i32,
            h_info.height as i32,
            has_mips,
            fmt,
            &packed,
        )?;

        ImageTexture::create_from_image(&image)
    }

    fn load_mesh_from_vfs(virtual_path: &str) -> Option<Gd<ArrayMesh>> {
        let vfs = get_global_vfs();
        let guard = vfs.read().ok()?;
        let raw_bytes = guard.read_file_relaxed(virtual_path).ok()?;
        let data = raw_bytes.as_slice();

        if virtual_path.to_lowercase().ends_with(".dgf") {
            if data.len() < DGF_BLOCK_SIZE {
                return None;
            }
            let block: &[u8; DGF_BLOCK_SIZE] = data[0..DGF_BLOCK_SIZE].try_into().ok()?;
            let (verts, indices) = DgfDecoder::decode_block(block).ok()?;

            let mut out_positions = PackedVector3Array::new();
            let mut out_indices = PackedInt32Array::new();

            for v in verts {
                out_positions.push(Vector3::new(v[0], v[1], v[2]));
            }
            for idx in indices {
                out_indices.push(idx as i32);
            }

            let mut arrays = VariantArray::new();
            arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
            arrays.set(
                ArrayType::VERTEX.ord() as usize,
                &out_positions.to_variant(),
            );
            arrays.set(ArrayType::INDEX.ord() as usize, &out_indices.to_variant());

            let mut mesh = ArrayMesh::new_gd();
            mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
            return Some(mesh);
        }

        let header_size = std::mem::size_of::<MeshletContainerHeader>();
        if data.len() < header_size {
            return None;
        }

        let header: &MeshletContainerHeader = bytemuck::from_bytes(&data[0..header_size]);
        if header.magic != MESHLET_MAGIC {
            return None;
        }

        let desc_size = std::mem::size_of::<MeshletDescriptor>();
        let desc_start = header_size;
        let desc_end = desc_start + (header.meshlet_count as usize * desc_size);

        let vert_size = std::mem::size_of::<QuantizedVertex>();
        let vert_start = desc_end;
        let vert_end = vert_start + (header.total_vertex_count as usize * vert_size);

        let tri_size = std::mem::size_of::<MeshletTriangle>();
        let tri_start = vert_end;
        let tri_end = tri_start + (header.total_triangle_count as usize * tri_size);

        if data.len() < tri_end {
            return None;
        }

        let descriptors: &[MeshletDescriptor] = bytemuck::cast_slice(&data[desc_start..desc_end]);
        let vertices: &[QuantizedVertex] = bytemuck::cast_slice(&data[vert_start..vert_end]);
        let triangles: &[MeshletTriangle] = bytemuck::cast_slice(&data[tri_start..tri_end]);

        let mut out_positions = PackedVector3Array::new();
        let mut out_normals = PackedVector3Array::new();
        let mut out_uvs = PackedVector2Array::new();
        let mut out_indices = PackedInt32Array::new();

        let mut vertex_offset_counter = 0i32;

        for desc in descriptors {
            let v_start = desc.vertex_offset as usize;
            let v_count = desc.vertex_count as usize;
            let t_start = desc.triangle_offset as usize;
            let t_count = desc.triangle_count as usize;

            for v in &vertices[v_start..v_start + v_count] {
                let local_int = [
                    (desc.quant_offset[0] + v.position_quant[0] as u32) as f32,
                    (desc.quant_offset[1] + v.position_quant[1] as u32) as f32,
                    (desc.quant_offset[2] + v.position_quant[2] as u32) as f32,
                ];

                let pos = Vector3::new(
                    header.global_min[0] + local_int[0] * header.dequant_factor[0],
                    header.global_min[1] + local_int[1] * header.dequant_factor[1],
                    header.global_min[2] + local_int[2] * header.dequant_factor[2],
                );

                let norm_arr = decode_octahedral_normal(v.normal_oct);
                let norm = Vector3::new(norm_arr[0], norm_arr[1], norm_arr[2]);
                let uv = Vector2::new(f16_to_f32(v.uv_half[0]), f16_to_f32(v.uv_half[1]));

                out_positions.push(pos);
                out_normals.push(norm);
                out_uvs.push(uv);
            }

            for t in &triangles[t_start..t_start + t_count] {
                out_indices.push(vertex_offset_counter + t.i0 as i32);
                out_indices.push(vertex_offset_counter + t.i1 as i32);
                out_indices.push(vertex_offset_counter + t.i2 as i32);
            }

            vertex_offset_counter += v_count as i32;
        }

        let mut arrays = VariantArray::new();
        arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
        arrays.set(
            ArrayType::VERTEX.ord() as usize,
            &out_positions.to_variant(),
        );
        arrays.set(ArrayType::NORMAL.ord() as usize, &out_normals.to_variant());
        arrays.set(ArrayType::TEX_UV.ord() as usize, &out_uvs.to_variant());
        arrays.set(ArrayType::INDEX.ord() as usize, &out_indices.to_variant());

        let mut mesh = ArrayMesh::new_gd();
        mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
        Some(mesh)
    }

    fn load_audio_from_vfs(virtual_path: &str) -> Option<Gd<Resource>> {
        let vfs = get_global_vfs();
        let guard = vfs.read().ok()?;
        let raw_bytes = guard.read_file_relaxed(virtual_path).ok()?;
        let packed = PackedByteArray::from(raw_bytes.as_slice());
        let path_lower = virtual_path.to_lowercase();

        if path_lower.ends_with(".ogg") {
            return AudioStreamOggVorbis::load_from_buffer(&packed).map(|s| s.upcast());
        }
        if path_lower.ends_with(".mp3") {
            let mut mp3 = AudioStreamMp3::new_gd();
            mp3.set_data(&packed);
            return Some(mp3.upcast());
        }
        if path_lower.ends_with(".wav") {
            let mut wav = AudioStreamWav::new_gd();
            wav.set_data(&packed);
            return Some(wav.upcast());
        }
        None
    }

    fn load_json_from_vfs(virtual_path: &str) -> Option<Gd<Json>> {
        let vfs = get_global_vfs();
        let guard = vfs.read().ok()?;
        let raw_bytes = guard.read_file_relaxed(virtual_path).ok()?;
        let text = std::str::from_utf8(&raw_bytes).ok()?;

        let mut json = Json::new_gd();
        if json.parse(text) == godot::global::Error::OK {
            Some(json)
        } else {
            None
        }
    }
}
