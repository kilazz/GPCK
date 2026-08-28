// crates/gpck_core/src/graphics/dxbc.rs
//! # DXBC / Shader Model 4.x - 5.x Bytecode Parser & Reflection
//!
//! Provides zero-allocation, sub-microsecond binary reflection for compiled DirectX
//! shader bytecode (`.cso` / `.dxbc`), outputting unified `ShaderReflectionInfo`.

pub use super::reflection::{ShaderReflectionInfo, ShaderStage};
use crate::core::error::{GpckError, GpckResult};

/// Magic FourCC for DirectX Bytecode container (`"DXBC"` in Little Endian).
pub const DXBC_MAGIC: u32 = 0x43425844;

#[derive(Debug, Clone, Default)]
pub struct ShaderReflection {
    pub stage: ShaderStage,
    pub major_version: u8,
    pub minor_version: u8,
    pub thread_group_size: Option<(u32, u32, u32)>,
    pub constant_buffer_count: usize,
    pub resource_srv_count: usize,
    pub uav_count: usize,
    pub sampler_count: usize,
}

impl ShaderReflection {
    #[inline(always)]
    pub fn is_dxbc(data: &[u8]) -> bool {
        if data.len() < 32 {
            return false;
        }
        u32::from_le_bytes(data[0..4].try_into().unwrap_or_default()) == DXBC_MAGIC
    }

    pub fn parse(data: &[u8]) -> GpckResult<Self> {
        if !Self::is_dxbc(data) {
            return Err(GpckError::DxbcParseError(
                "Invalid DXBC shader container magic".to_string(),
            ));
        }

        let total_size = u32::from_le_bytes(
            data[24..28]
                .try_into()
                .map_err(|_| GpckError::DxbcParseError("Truncated size field".to_string()))?,
        ) as usize;

        let chunk_count =
            u32::from_le_bytes(data[28..32].try_into().map_err(|_| {
                GpckError::DxbcParseError("Truncated chunk count field".to_string())
            })?) as usize;

        if data.len() < total_size || data.len() < 32 + chunk_count * 4 {
            return Err(GpckError::DxbcParseError(
                "Truncated DXBC bytecode".to_string(),
            ));
        }

        let mut reflection = Self::default();

        for i in 0..chunk_count {
            let offset_pos = 32 + i * 4;
            let chunk_offset = u32::from_le_bytes(
                data[offset_pos..offset_pos + 4]
                    .try_into()
                    .map_err(|_| GpckError::DxbcParseError("Corrupted chunk offset".to_string()))?,
            ) as usize;

            if chunk_offset + 8 > data.len() {
                continue;
            }

            let fourcc = &data[chunk_offset..chunk_offset + 4];
            let chunk_size = u32::from_le_bytes(
                data[chunk_offset + 4..chunk_offset + 8]
                    .try_into()
                    .map_err(|_| GpckError::DxbcParseError("Corrupted chunk size".to_string()))?,
            ) as usize;
            let payload_start = chunk_offset + 8;
            let payload_end = payload_start + chunk_size;

            if payload_end > data.len() {
                continue;
            }

            if fourcc == b"SHDR" || fourcc == b"SHEX" {
                Self::parse_shader_token_stream(
                    &data[payload_start..payload_end],
                    &mut reflection,
                )?;
                break;
            }
        }

        Ok(reflection)
    }

    fn parse_shader_token_stream(
        payload: &[u8],
        reflection: &mut ShaderReflection,
    ) -> GpckResult<()> {
        if payload.len() < 8 {
            return Err(GpckError::DxbcParseError(
                "SHDR chunk payload is too small".to_string(),
            ));
        }

        let ver_tok = u32::from_le_bytes(
            payload[0..4]
                .try_into()
                .map_err(|_| GpckError::DxbcParseError("Invalid version token".to_string()))?,
        );
        let prog_type = ((ver_tok >> 16) & 0xFFFF) as u16;
        let major = ((ver_tok >> 4) & 0x0F) as u8;
        let minor = (ver_tok & 0x0F) as u8;

        reflection.stage = ShaderStage::from_dxbc_u16(prog_type);
        reflection.major_version = major;
        reflection.minor_version = minor;

        let total_dwords = u32::from_le_bytes(
            payload[4..8]
                .try_into()
                .map_err(|_| GpckError::DxbcParseError("Invalid dword count".to_string()))?,
        ) as usize;
        let dwords_available = payload.len() / 4;
        let max_dwords = total_dwords.min(dwords_available);

        let mut dword_idx = 2usize;

        while dword_idx < max_dwords {
            let opcode_tok = u32::from_le_bytes(
                payload[dword_idx * 4..(dword_idx + 1) * 4]
                    .try_into()
                    .map_err(|_| GpckError::DxbcParseError("Invalid opcode token".to_string()))?,
            );
            let opcode_type = opcode_tok & 0x7FF;
            let inst_len = ((opcode_tok >> 24) & 0x7F) as usize;

            if inst_len == 0 {
                break;
            }

            match opcode_type {
                88 | 161 | 162 => {
                    reflection.resource_srv_count += 1;
                }
                89 => {
                    reflection.constant_buffer_count += 1;
                }
                90 => {
                    reflection.sampler_count += 1;
                }
                156..=158 => {
                    reflection.uav_count += 1;
                }
                155 if dword_idx + 3 < max_dwords => {
                    let tg_x = u32::from_le_bytes(
                        payload[(dword_idx + 1) * 4..(dword_idx + 2) * 4]
                            .try_into()
                            .map_err(|_| {
                                GpckError::DxbcParseError("Invalid TG X dimension".to_string())
                            })?,
                    );
                    let tg_y = u32::from_le_bytes(
                        payload[(dword_idx + 2) * 4..(dword_idx + 3) * 4]
                            .try_into()
                            .map_err(|_| {
                                GpckError::DxbcParseError("Invalid TG Y dimension".to_string())
                            })?,
                    );
                    let tg_z = u32::from_le_bytes(
                        payload[(dword_idx + 3) * 4..(dword_idx + 4) * 4]
                            .try_into()
                            .map_err(|_| {
                                GpckError::DxbcParseError("Invalid TG Z dimension".to_string())
                            })?,
                    );
                    reflection.thread_group_size = Some((tg_x, tg_y, tg_z));
                }
                _ => {}
            }

            dword_idx += inst_len;
        }

        Ok(())
    }

    pub fn to_unified_info(&self) -> ShaderReflectionInfo {
        ShaderReflectionInfo {
            entry_point_name: "main".to_string(),
            stage: self.stage,
            major_version: self.major_version,
            minor_version: self.minor_version,
            thread_group_size: self.thread_group_size.unwrap_or((1, 1, 1)),
            bindings: Vec::new(),
            push_constants: Vec::new(),
            constant_buffer_count: self.constant_buffer_count,
            srv_count: self.resource_srv_count,
            uav_count: self.uav_count,
            sampler_count: self.sampler_count,
        }
    }
}
