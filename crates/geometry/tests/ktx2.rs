//! KTX2 (Basis Universal) textures: deterministic files with full mip chains that decode back to
//! close to their source pixels, color as ETC1S and data as UASTC.
use superspec_geometry::image::{self, Image};
use superspec_geometry::ktx2::{self, Kind};

/// A smooth gradient with a soft disc: typical of baked color and normal content.
fn pattern(width: usize, height: usize) -> Image {
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let (u, v) = (x as f64 / width as f64, y as f64 / height as f64);
            let disc = (((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 4.).min(1.);
            rgba.extend([
                (40. + u * 180.) as u8,
                (60. + v * 150.) as u8,
                (200. - disc * 120.) as u8,
                255,
            ]);
        }
    }
    Image {
        width,
        height,
        rgba,
        mime: "image/png",
    }
}

fn mean_error(a: &[u8], b: &[u8]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (*a as f64 - *b as f64).abs())
        .sum::<f64>()
        / a.len() as f64
}

fn levels(file: &[u8]) -> u32 {
    u32::from_le_bytes(file[40..44].try_into().unwrap())
}

#[test]
fn color_and_data_textures_round_trip_with_mips() {
    let source = pattern(256, 128);
    for (kind, tolerance) in [(Kind::Color, 6.), (Kind::Data, 3.)] {
        let file = ktx2::encode(&source, kind).unwrap();
        assert!(file.starts_with(&ktx2::MAGIC));
        assert_eq!(ktx2::dimensions(&file).unwrap(), (256, 128));
        // 256 x 128 has nine levels down to 1 x 1.
        assert_eq!(levels(&file), 9, "{kind:?}");
        // Far smaller than the 128 KiB of raw pixels.
        assert!(file.len() < 40 * 1024, "{kind:?}: {} bytes", file.len());
        let decoded = ktx2::decode(&file).unwrap();
        assert_eq!((decoded.width, decoded.height), (256, 128));
        let error = mean_error(&decoded.rgba, &source.rgba);
        assert!(error < tolerance, "{kind:?}: mean error {error}");
        // The shared image decoder recognizes KTX2 too.
        assert_eq!(image::decode(&file).unwrap().rgba, decoded.rgba);
    }
}

#[test]
fn equal_pixels_give_equal_files() {
    let source = pattern(64, 64);
    for kind in [Kind::Color, Kind::Data] {
        assert_eq!(
            ktx2::encode(&source, kind).unwrap(),
            ktx2::encode(&source, kind).unwrap()
        );
    }
}

#[test]
fn invalid_textures_are_rejected() {
    let mut short = pattern(8, 8);
    short.rgba.pop();
    assert!(ktx2::encode(&short, Kind::Color).is_err());
    let empty = Image {
        width: 0,
        height: 4,
        rgba: vec![],
        mime: "image/png",
    };
    assert!(ktx2::encode(&empty, Kind::Data).is_err());
    assert!(ktx2::decode(b"not a texture").is_err());
    let mut truncated = ktx2::encode(&pattern(16, 16), Kind::Color).unwrap();
    truncated.truncate(64);
    assert!(ktx2::decode(&truncated).is_err());
}
