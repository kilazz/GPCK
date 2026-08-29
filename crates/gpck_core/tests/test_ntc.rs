// crates/gpck_core/tests/test_ntc.rs
//! # Native GPCK Neural Texture Tests (MiniDXNN DP4a Engine)

use gpck_core::compression::ntc::{NtcContext, NtcPbrMaterialBundle, Xoshiro128Plus};
use gpck_core::format::archive::TYPE_NEURAL_TEXTURE;
use gpck_core::packer::PackerOptions;
use gpck_core::packer::ntc_packer::NtcBundlePacker;

#[test]
fn test_native_ntc_prng_and_latent_shapes() {
    let mut rng = Xoshiro128Plus::new(12345);
    for _ in 0..100 {
        let v = rng.draw_f32();
        assert!((0.0..1.0).contains(&v));
    }

    let ctx = NtcContext::new().unwrap();
    let (chosen_bpp, res, dim) = ctx.pick_latent_shape(5.0).unwrap();
    assert_eq!(chosen_bpp, 5.0);
    assert_eq!(res, 64);
    assert_eq!(dim, 8);
}

#[test]
fn test_native_ntc_pbr_bundle_packer_roundtrip() {
    let width = 512u32;
    let height = 512u32;
    let total_pixels = (width * height) as usize;

    let mut bundle = NtcPbrMaterialBundle::new(width, height);
    bundle.albedo = Some(vec![200u8; total_pixels * 3]);
    bundle.normal = Some(vec![128u8; total_pixels * 2]);
    bundle.roughness = Some(vec![80u8; total_pixels]);
    bundle.metallic = Some(vec![0u8; total_pixels]);
    bundle.ao = Some(vec![255u8; total_pixels]);

    let options = PackerOptions::default();
    let file = NtcBundlePacker::pack_pbr_bundle(
        &bundle,
        "materials/props/rusted_metal.gntc",
        &options,
        None,
        Some(5.0),
    )
    .unwrap();

    assert_eq!(file.original_path, "materials/props/rusted_metal.gntc");
    assert!((file.flags & TYPE_NEURAL_TEXTURE) != 0);
    assert_eq!(file.alignment, 65536);
    assert!(!file.chunks.is_empty());
}
