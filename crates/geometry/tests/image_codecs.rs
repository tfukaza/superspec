use superspec_geometry::image::{decode, encode};

#[test]
fn streamed_png_retains_only_encoded_bytes_and_bounds_entropy_and_cancellation() {
    use superspec_geometry::image::{Image, encode_cancellable, encoding_working_bytes};
    fn noisy(width: usize, height: usize) -> Image {
        let mut state = 37u32;
        Image {
            width,
            height,
            mime: "image/png",
            rgba: (0..width * height * 4)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state as u8
                })
                .collect(),
        }
    }
    for (width, height) in [(1, 8192), (8192, 1), (103, 503)] {
        let image = noisy(width, height);
        let encoded = encode(&image).unwrap();
        assert_eq!(encoded.capacity(), encoded.len());
        assert_eq!(decode(&encoded).unwrap().rgba, image.rgba);
    }
    let image = noisy(2048, 2048);
    assert!(
        encode(&image).unwrap_err().contains("byte budget"),
        "High entropy output exceeds the encoded budget without publication"
    );
    assert!(
        encoding_working_bytes(16_777_216, 1).is_err(),
        "Exceptionally wide rows are rejected before encoder allocation"
    );
    let small = noisy(32, 32);
    let rows = std::cell::Cell::new(0);
    assert!(
        encode_cancellable(&small, &|| {
            rows.set(rows.get() + 1);
            rows.get() > 4
        })
        .unwrap_err()
        .contains("cancelled")
    );
    assert_eq!(decode(&encode(&small).unwrap()).unwrap().rgba, small.rgba);
}

#[test]
fn jpeg_and_png_decode_distinct_source_samples_and_reject_truncation() {
    let png = include_bytes!("../../../tests/fixtures/aml/asymmetric-heightmap.png");
    let image = decode(png).unwrap();
    assert_eq!((image.width, image.height, image.mime), (2, 2, "image/png"));
    assert_eq!(
        image.rgba,
        [
            0, 0, 0, 255, 85, 85, 85, 255, 170, 170, 170, 255, 255, 255, 255, 255
        ]
    );
    let jpeg = include_bytes!("../../../tests/fixtures/rust/color-blocks.jpg");
    let image = decode(jpeg).unwrap();
    assert_eq!(
        (image.width, image.height, image.mime),
        (16, 16, "image/jpeg")
    );
    assert_eq!(image.rgba.len(), 16 * 16 * 4);
    let encoded = encode(&image).unwrap();
    assert_eq!(decode(&encoded).unwrap().rgba, image.rgba);
    // Uniform 8x8 blocks avoid chroma-edge ambiguity; JPEG IDCT permits a two-byte rounding difference.
    for (index, expected) in [
        (0, [240, 32, 48]),
        (8, [24, 212, 72]),
        (128, [40, 72, 224]),
        (136, [208, 176, 48]),
    ] {
        for (channel, value) in expected.into_iter().enumerate() {
            assert!((image.rgba[index * 4 + channel] as i32 - value).abs() <= 2);
        }
        assert_eq!(image.rgba[index * 4 + 3], 255);
    }
    for bytes in [
        &png[..8],
        &png[..png.len() - 12],
        &jpeg[..jpeg.len() / 2],
        b"not an image",
    ] {
        assert!(decode(bytes).is_err());
    }
    assert!(
        decode(jpeg).is_ok(),
        "failed inputs do not poison a later decode"
    );
}

#[test]
fn png_normalization_preserves_explicit_alpha_but_clears_keyed_colors_and_rescales_sixteen_bits() {
    let encode =
        |color, depth, pixels: &[u8], transparency: Option<Vec<u8>>, palette: Option<Vec<u8>>| {
            let mut bytes = vec![];
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(color);
            encoder.set_depth(depth);
            if let Some(value) = transparency {
                encoder.set_trns(value);
            }
            if let Some(value) = palette {
                encoder.set_palette(value);
            }
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(pixels).unwrap();
            writer.finish().unwrap();
            bytes
        };
    use png::{BitDepth as B, ColorType as C};
    for (bytes, expected) in [
        (
            encode(
                C::Rgb,
                B::Eight,
                &[17, 34, 51],
                Some(vec![0, 17, 0, 34, 0, 51]),
                None,
            ),
            [0, 0, 0, 0],
        ),
        (
            encode(C::Grayscale, B::Eight, &[85], Some(vec![0, 85]), None),
            [0, 0, 0, 0],
        ),
        (
            encode(C::Rgba, B::Eight, &[17, 34, 51, 0], None, None),
            [17, 34, 51, 0],
        ),
        (
            encode(
                C::Indexed,
                B::Eight,
                &[0],
                Some(vec![0]),
                Some(vec![17, 34, 51]),
            ),
            [17, 34, 51, 0],
        ),
        (
            encode(C::Rgb, B::Sixteen, &[0, 255, 128, 0, 255, 255], None, None),
            [1, 128, 255, 255],
        ),
    ] {
        assert_eq!(decode(&bytes).unwrap().rgba, expected);
    }
}
