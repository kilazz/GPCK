// crates/gpck_core/src/geometry/octahedral.rs
//! # Octahedral Vector Encoding & IEEE-754 Half-Precision Conversions
//!
//! Encodes 3D unit normal and tangent vectors (12 bytes each) into compact 2-byte signed
//! octahedral representations (`snorm8x2`) with under 0.4° angular error. Also provides
//! zero-allocation bitwise IEEE-754 single-to-half precision float conversions.

/// Encodes a 3D unit vector into a 2-byte signed octahedral representation `[-127..127]`.
#[inline]
pub fn encode_octahedral_normal(n: [f32; 3]) -> [i8; 2] {
    let l1_norm = n[0].abs() + n[1].abs() + n[2].abs();
    if l1_norm < 1e-6 {
        return [0, 127]; // Default fallback: +Z unit vector
    }

    let mut ox = n[0] / l1_norm;
    let mut oy = n[1] / l1_norm;

    if n[2] < 0.0 {
        let old_ox = ox;
        ox = (1.0 - oy.abs()) * if old_ox >= 0.0 { 1.0 } else { -1.0 };
        oy = (1.0 - old_ox.abs()) * if oy >= 0.0 { 1.0 } else { -1.0 };
    }

    let qx = (ox * 127.0).round().clamp(-127.0, 127.0) as i8;
    let qy = (oy * 127.0).round().clamp(-127.0, 127.0) as i8;

    [qx, qy]
}

/// Decodes a 2-byte signed octahedral normal back into a normalized 3D unit vector.
#[inline]
pub fn decode_octahedral_normal(oct: [i8; 2]) -> [f32; 3] {
    let mut ox = oct[0] as f32 / 127.0;
    let mut oy = oct[1] as f32 / 127.0;
    let oz = 1.0 - ox.abs() - oy.abs();

    if oz < 0.0 {
        let old_ox = ox;
        ox = (1.0 - oy.abs()) * if old_ox >= 0.0 { 1.0 } else { -1.0 };
        oy = (1.0 - old_ox.abs()) * if oy >= 0.0 { 1.0 } else { -1.0 };
    }

    let len = (ox * ox + oy * oy + oz * oz).sqrt().max(1e-6);
    [ox / len, oy / len, oz / len]
}

/// Encodes a 3D unit tangent vector and bitangent sign (+1.0 / -1.0) into 2 bytes.
#[inline]
pub fn encode_octahedral_tangent(tangent: [f32; 3], bitangent_sign: f32) -> [i8; 2] {
    let oct = encode_octahedral_normal(tangent);
    let sign_bit = if bitangent_sign < 0.0 { -1i8 } else { 1i8 };
    [oct[0], (oct[1].abs() * sign_bit).clamp(-127, 127)]
}

/// Converts a 32-bit single-precision float to an IEEE-754 16-bit half-precision float (binary16).
#[inline]
pub fn f32_to_f16(val: f32) -> u16 {
    let bits = val.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x007f_ffff;

    if exp == 255 {
        // NaN or Infinity
        return sign
            | 0x7c00
            | if mant != 0 {
                ((mant >> 13) as u16).max(1)
            } else {
                0
            };
    }

    let new_exp = exp - 127 + 15;

    if new_exp >= 31 {
        // Overflow to Infinity
        return sign | 0x7c00;
    }

    if new_exp <= 0 {
        // Subnormal or zero
        if new_exp < -10 {
            return sign;
        }
        let full_mant = mant | 0x0080_0000;
        let shift = (14 - new_exp) as u32;
        let round_bit = 1 << (shift - 1);
        let rounded_mant = (full_mant + round_bit) >> shift;
        return sign | (rounded_mant as u16);
    }

    // Normal numbers
    let round_bit = 1 << 12;
    let rounded = (mant + round_bit) >> 13;
    let result = ((new_exp as u32) << 10) + rounded;
    if result >= (31 << 10) {
        return sign | 0x7c00;
    }
    sign | (result as u16)
}

/// Converts an IEEE-754 16-bit half-precision float back to a 32-bit single-precision float.
#[inline]
pub fn f16_to_f32(val: u16) -> f32 {
    let sign = ((val & 0x8000) as u32) << 16;
    let exp = ((val & 0x7c00) >> 10) as u32;
    let mant = (val & 0x03ff) as u32;

    let f_bits = if exp == 0 {
        if mant == 0 {
            sign
        } else {
            // Subnormal f16 -> Normal f32
            let mut m = mant << 13;
            let mut e: i32 = 127 - 15 + 1; // 113
            while (m & 0x0080_0000) == 0 {
                m <<= 1;
                e -= 1;
            }
            m &= 0x007f_ffff;
            sign | ((e as u32) << 23) | m
        }
    } else if exp == 31 {
        // Infinity or NaN
        sign | 0x7f80_0000 | (mant << 13)
    } else {
        // Normal numbers: (exp + 127 - 15) avoids unsigned underflow
        let new_exp = exp + 127 - 15;
        sign | (new_exp << 23) | (mant << 13)
    };

    f32::from_bits(f_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_octahedral_normal_roundtrip() {
        let directions = [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [-1.0, 0.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, -1.0],
            [0.57735, 0.57735, 0.57735],
            [-0.57735, 0.57735, -0.57735],
        ];

        for &dir in &directions {
            let encoded = encode_octahedral_normal(dir);
            let decoded = decode_octahedral_normal(encoded);

            let dot = dir[0] * decoded[0] + dir[1] * decoded[1] + dir[2] * decoded[2];
            assert!(
                dot > 0.995,
                "Vector {:?} lost precision: decoded as {:?} (dot = {})",
                dir,
                decoded,
                dot
            );
        }
    }

    #[test]
    fn test_f16_f32_conversion() {
        let values = [0.0f32, 1.0, -1.0, 0.5, 65504.0, -65504.0, 0.000061035];
        for &val in &values {
            let h = f32_to_f16(val);
            let restored = f16_to_f32(h);
            assert!(
                (val - restored).abs() <= val.abs() * 0.002 + 1e-6,
                "Failed f16 conversion for value: expected {}, got {}",
                val,
                restored
            );
        }
    }
}
