// crates/gpck_core/src/compression/rans.rs
//! # Interleaved 4-Way rANS (Asymmetric Numeral Systems) Codec
//!
//! Provides SIMD-friendly 4-way interleaved entropy encoding and decoding
//! with automatic RLE fallback and normalized probability distribution tables.

use crate::core::error::{GpckError, GpckResult};

const RANS_BYTE_LOWER: u32 = 1 << 23;
const PROB_BITS: u32 = 12;
const PROB_SCALE: u32 = 1 << PROB_BITS; // 4096
const PROB_MASK: u32 = PROB_SCALE - 1;

#[derive(Clone, Copy, Default)]
struct RansSymbol {
    start: u16,
    freq: u16,
}

pub struct RansCodec;

impl RansCodec {
    /// Compresses source bytes into an interleaved 4-way rANS byte stream.
    pub fn compress(src: &[u8]) -> GpckResult<Vec<u8>> {
        if src.is_empty() {
            return Ok(Vec::new());
        }

        let mut freqs = [0u32; 256];
        for &b in src {
            freqs[b as usize] += 1;
        }

        let unique_symbols = freqs.iter().filter(|&&f| f > 0).count();
        if unique_symbols <= 1 {
            let symbol = src.first().copied().unwrap_or(0);
            let mut out = Vec::with_capacity(6);
            out.push(0x01); // RLE Magic
            out.push(symbol);
            out.extend_from_slice(&(src.len() as u32).to_le_bytes());
            return Ok(out);
        }

        let norm_freqs = normalize_probabilities(&freqs, src.len(), PROB_SCALE as usize);

        let mut symbols = [RansSymbol::default(); 256];
        let mut curr_cum = 0u16;

        for i in 0..256 {
            let f = norm_freqs[i];
            symbols[i] = RansSymbol {
                start: curr_cum,
                freq: f,
            };
            curr_cum += f;
        }

        let mut ptrs0 = Vec::with_capacity(src.len() / 4);
        let mut ptrs1 = Vec::with_capacity(src.len() / 4);
        let mut ptrs2 = Vec::with_capacity(src.len() / 4);
        let mut ptrs3 = Vec::with_capacity(src.len() / 4);

        let mut x0 = RANS_BYTE_LOWER;
        let mut x1 = RANS_BYTE_LOWER;
        let mut x2 = RANS_BYTE_LOWER;
        let mut x3 = RANS_BYTE_LOWER;

        let chunks_4 = src.chunks_exact(4);
        let remainder = chunks_4.remainder();

        for &b in remainder.iter().rev() {
            let sym = &symbols[b as usize];
            x0 = rans_enc_symbol(x0, sym.start as u32, sym.freq as u32, &mut ptrs0);
        }

        for chunk in chunks_4.rev() {
            let sym3 = &symbols[chunk[3] as usize];
            let sym2 = &symbols[chunk[2] as usize];
            let sym1 = &symbols[chunk[1] as usize];
            let sym0 = &symbols[chunk[0] as usize];

            x3 = rans_enc_symbol(x3, sym3.start as u32, sym3.freq as u32, &mut ptrs3);
            x2 = rans_enc_symbol(x2, sym2.start as u32, sym2.freq as u32, &mut ptrs2);
            x1 = rans_enc_symbol(x1, sym1.start as u32, sym1.freq as u32, &mut ptrs1);
            x0 = rans_enc_symbol(x0, sym0.start as u32, sym0.freq as u32, &mut ptrs0);
        }

        let mut output = Vec::with_capacity(src.len() + 1024);
        output.push(0x02); // 4-Stream Interleaved rANS Magic

        let non_zero_count = norm_freqs.iter().filter(|&&f| f > 0).count();
        output.push(if non_zero_count == 256 {
            0
        } else {
            non_zero_count as u8
        });

        for (sym, &f) in norm_freqs.iter().enumerate() {
            if f > 0 {
                output.push(sym as u8);
                output.extend_from_slice(&f.to_le_bytes());
            }
        }

        output.extend_from_slice(&(src.len() as u32).to_le_bytes());
        output.extend_from_slice(&x0.to_le_bytes());
        output.extend_from_slice(&x1.to_le_bytes());
        output.extend_from_slice(&x2.to_le_bytes());
        output.extend_from_slice(&x3.to_le_bytes());

        ptrs0.reverse();
        ptrs1.reverse();
        ptrs2.reverse();
        ptrs3.reverse();

        output.extend_from_slice(&(ptrs0.len() as u32).to_le_bytes());
        output.extend_from_slice(&ptrs0);
        output.extend_from_slice(&(ptrs1.len() as u32).to_le_bytes());
        output.extend_from_slice(&ptrs1);
        output.extend_from_slice(&(ptrs2.len() as u32).to_le_bytes());
        output.extend_from_slice(&ptrs2);
        output.extend_from_slice(&(ptrs3.len() as u32).to_le_bytes());
        output.extend_from_slice(&ptrs3);

        Ok(output)
    }

    /// Decompresses an interleaved 4-way rANS byte stream.
    pub fn decompress(src: &[u8], target_len: usize) -> GpckResult<Vec<u8>> {
        if src.is_empty() || target_len == 0 {
            return Ok(Vec::new());
        }

        let magic = src[0];
        if magic == 0x01 {
            if src.len() < 6 {
                return Err(GpckError::DecompressionFailed {
                    method: "rANS",
                    message: "Corrupted rANS RLE header: buffer too small".to_string(),
                });
            }
            let symbol = src[1];
            let len = u32::from_le_bytes(src[2..6].try_into().map_err(|_| {
                GpckError::DecompressionFailed {
                    method: "rANS",
                    message: "Invalid slice conversion".to_string(),
                }
            })?) as usize;
            return Ok(vec![symbol; len.min(target_len)]);
        }

        if magic != 0x02 || src.len() < 26 {
            return Err(GpckError::DecompressionFailed {
                method: "rANS",
                message: "Invalid 4-way rANS stream header or corrupted magic".to_string(),
            });
        }

        let count_byte = src[1] as usize;
        let non_zero_count = if count_byte == 0 { 256 } else { count_byte };
        let mut offset = 2;

        let mut norm_freqs = [0u16; 256];
        let mut total_freq_sum = 0u32;
        for _ in 0..non_zero_count {
            if offset + 3 > src.len() {
                return Err(GpckError::DecompressionFailed {
                    method: "rANS",
                    message: "Corrupted rANS frequency table: unexpected end of stream".to_string(),
                });
            }
            let sym = src[offset] as usize;
            let freq =
                u16::from_le_bytes(src[offset + 1..offset + 3].try_into().map_err(|_| {
                    GpckError::DecompressionFailed {
                        method: "rANS",
                        message: "Slice conversion error".to_string(),
                    }
                })?);
            norm_freqs[sym] = freq;
            total_freq_sum += freq as u32;
            offset += 3;
        }

        if total_freq_sum != PROB_SCALE {
            return Err(GpckError::DecompressionFailed {
                method: "rANS",
                message: format!(
                    "Corrupted rANS probability table: sum is {}, expected {}",
                    total_freq_sum, PROB_SCALE
                ),
            });
        }

        if offset + 20 > src.len() {
            return Err(GpckError::DecompressionFailed {
                method: "rANS",
                message: "Corrupted rANS stream: missing state headers".to_string(),
            });
        }

        let decompressed_len =
            u32::from_le_bytes(src[offset..offset + 4].try_into().map_err(|_| {
                GpckError::DecompressionFailed {
                    method: "rANS",
                    message: "Slice conversion error".to_string(),
                }
            })?) as usize;
        offset += 4;

        let mut x0 = u32::from_le_bytes(src[offset..offset + 4].try_into().unwrap());
        let mut x1 = u32::from_le_bytes(src[offset + 4..offset + 8].try_into().unwrap());
        let mut x2 = u32::from_le_bytes(src[offset + 8..offset + 12].try_into().unwrap());
        let mut x3 = u32::from_le_bytes(src[offset + 12..offset + 16].try_into().unwrap());
        offset += 16;

        let len0 = u32::from_le_bytes(src[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if offset + len0 > src.len() {
            return Err(GpckError::DecompressionFailed {
                method: "rANS",
                message: "Corrupted stream 0 length".to_string(),
            });
        }
        let p0 = &src[offset..offset + len0];
        offset += len0;

        let len1 = u32::from_le_bytes(src[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if offset + len1 > src.len() {
            return Err(GpckError::DecompressionFailed {
                method: "rANS",
                message: "Corrupted stream 1 length".to_string(),
            });
        }
        let p1 = &src[offset..offset + len1];
        offset += len1;

        let len2 = u32::from_le_bytes(src[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if offset + len2 > src.len() {
            return Err(GpckError::DecompressionFailed {
                method: "rANS",
                message: "Corrupted stream 2 length".to_string(),
            });
        }
        let p2 = &src[offset..offset + len2];
        offset += len2;

        let len3 = u32::from_le_bytes(src[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if offset + len3 > src.len() {
            return Err(GpckError::DecompressionFailed {
                method: "rANS",
                message: "Corrupted stream 3 length".to_string(),
            });
        }
        let p3 = &src[offset..offset + len3];

        let mut lut_symbols = [0u8; PROB_SCALE as usize];
        let mut lut_starts = [0u16; PROB_SCALE as usize];
        let mut lut_freqs = [0u16; PROB_SCALE as usize];

        let mut curr_slot = 0usize;
        for (sym, &f) in norm_freqs.iter().enumerate() {
            let freq = f as usize;
            for i in 0..freq {
                if curr_slot < PROB_SCALE as usize {
                    lut_symbols[curr_slot] = sym as u8;
                    lut_starts[curr_slot] = (curr_slot - i) as u16;
                    lut_freqs[curr_slot] = f;
                    curr_slot += 1;
                }
            }
        }

        let mut b0 = 0usize;
        let mut b1 = 0usize;
        let mut b2 = 0usize;
        let mut b3 = 0usize;

        let mut dst = vec![0u8; target_len.min(decompressed_len)];
        let num_quads = dst.len() / 4;
        let remainder_len = dst.len() % 4;
        let mut dst_ptr = 0usize;

        for _ in 0..num_quads {
            let slot0 = (x0 & PROB_MASK) as usize;
            let slot1 = (x1 & PROB_MASK) as usize;
            let slot2 = (x2 & PROB_MASK) as usize;
            let slot3 = (x3 & PROB_MASK) as usize;

            let freq0 = lut_freqs[slot0] as u32;
            let freq1 = lut_freqs[slot1] as u32;
            let freq2 = lut_freqs[slot2] as u32;
            let freq3 = lut_freqs[slot3] as u32;

            if freq0 == 0 || freq1 == 0 || freq2 == 0 || freq3 == 0 {
                return Err(GpckError::DecompressionFailed {
                    method: "rANS",
                    message:
                        "Corrupted rANS stream: zero symbol frequency encountered during decoding"
                            .to_string(),
                });
            }

            dst[dst_ptr] = lut_symbols[slot0];
            dst[dst_ptr + 1] = lut_symbols[slot1];
            dst[dst_ptr + 2] = lut_symbols[slot2];
            dst[dst_ptr + 3] = lut_symbols[slot3];
            dst_ptr += 4;

            x0 = freq0 * (x0 >> PROB_BITS) + (slot0 as u32 - lut_starts[slot0] as u32);
            x1 = freq1 * (x1 >> PROB_BITS) + (slot1 as u32 - lut_starts[slot1] as u32);
            x2 = freq2 * (x2 >> PROB_BITS) + (slot2 as u32 - lut_starts[slot2] as u32);
            x3 = freq3 * (x3 >> PROB_BITS) + (slot3 as u32 - lut_starts[slot3] as u32);

            rans_dec_renorm(&mut x0, p0, &mut b0);
            rans_dec_renorm(&mut x1, p1, &mut b1);
            rans_dec_renorm(&mut x2, p2, &mut b2);
            rans_dec_renorm(&mut x3, p3, &mut b3);
        }

        for _ in 0..remainder_len {
            let slot0 = (x0 & PROB_MASK) as usize;
            let freq0 = lut_freqs[slot0] as u32;
            if freq0 == 0 {
                return Err(GpckError::DecompressionFailed {
                    method: "rANS",
                    message: "Corrupted rANS stream: zero remainder frequency".to_string(),
                });
            }
            dst[dst_ptr] = lut_symbols[slot0];
            dst_ptr += 1;
            x0 = freq0 * (x0 >> PROB_BITS) + (slot0 as u32 - lut_starts[slot0] as u32);
            rans_dec_renorm(&mut x0, p0, &mut b0);
        }

        Ok(dst)
    }
}

#[inline(always)]
fn rans_enc_symbol(mut x: u32, start: u32, freq: u32, ptrs: &mut Vec<u8>) -> u32 {
    let max_x = ((RANS_BYTE_LOWER >> PROB_BITS) << 8) * freq;
    while x >= max_x {
        ptrs.push((x & 0xFF) as u8);
        x >>= 8;
    }
    ((x / freq) << PROB_BITS) + (x % freq) + start
}

#[inline(always)]
fn rans_dec_renorm(x: &mut u32, payload: &[u8], byte_ptr: &mut usize) {
    while *x < RANS_BYTE_LOWER {
        if *byte_ptr < payload.len() {
            *x = (*x << 8) | (payload[*byte_ptr] as u32);
            *byte_ptr += 1;
        } else {
            if *x == 0 {
                *x = RANS_BYTE_LOWER;
                break;
            }
            *x <<= 8;
        }
    }
}

fn normalize_probabilities(
    freqs: &[u32; 256],
    total_count: usize,
    target_sum: usize,
) -> [u16; 256] {
    let mut norm = [0u16; 256];
    let mut sum = 0usize;

    for i in 0..256 {
        if freqs[i] == 0 {
            continue;
        }
        let scaled = ((freqs[i] as u64 * target_sum as u64) / total_count as u64) as u16;
        let f = scaled.max(1);
        norm[i] = f;
        sum += f as usize;
    }

    while sum != target_sum {
        if sum < target_sum {
            for val in &mut norm {
                if *val > 0 {
                    *val += 1;
                    sum += 1;
                    if sum == target_sum {
                        break;
                    }
                }
            }
        } else {
            for val in &mut norm {
                if *val > 1 {
                    *val -= 1;
                    sum -= 1;
                    if sum == target_sum {
                        break;
                    }
                }
            }
        }
    }

    norm
}
