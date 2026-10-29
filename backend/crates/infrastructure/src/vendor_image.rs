//! Bounded, metadata-free reference image normalization.

use std::io::Cursor;

use application::ports::NormalizedVendorImage;
use image::{DynamicImage, ImageDecoder, ImageFormat, Limits, Rgb, RgbImage};

pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;
// PNG's internal decoder and every decoded output buffer use this budget.
// zune-jpeg exposes no scratch-allocation limit: its scratch is bounded only
// by dimensions/scans, not by this value. This is a decoder/output budget,
// not a hard limit on JPEG scratch allocations or process RSS.
const MAX_ALLOC: u64 = 64 * 1024 * 1024;

/// Deliberately opaque: decoder messages must never reach logs or responses.
#[derive(Debug)]
pub struct InvalidImage;

pub fn normalize(data: &[u8]) -> Result<NormalizedVendorImage, InvalidImage> {
    if data.is_empty() || data.len() > MAX_IMAGE_BYTES {
        return Err(InvalidImage);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(MAX_ALLOC);
    let format = image::guess_format(data).map_err(|_| InvalidImage)?;
    let (mut decoded, orientation) = match format {
        ImageFormat::Jpeg => {
            validate_jpeg_end(data)?;
            // image's JPEG adapter disables strict mode and header dimension
            // limits. Use its existing underlying codec with limits configured
            // BEFORE decode_headers, not after its header allocations.
            let options = zune_core::options::DecoderOptions::default()
                .set_strict_mode(true)
                .set_max_width(8192)
                .set_max_height(8192)
                .jpeg_set_max_scans(32)
                .jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::RGB);
            let mut decoder = zune_jpeg::JpegDecoder::new_with_options(
                zune_core::bytestream::ZCursor::new(data),
                options,
            );
            decoder.decode_headers().map_err(|_| InvalidImage)?;
            let (width, height) = decoder.dimensions().ok_or(InvalidImage)?;
            let size = decoder.output_buffer_size().ok_or(InvalidImage)?;
            if width == 0
                || height == 0
                || width as u64 * height as u64 > MAX_PIXELS
                || size as u64 > MAX_ALLOC
            {
                return Err(InvalidImage);
            }
            let orientation = decoder
                .exif()
                .and_then(|exif| image::metadata::Orientation::from_exif_chunk(exif))
                .unwrap_or(image::metadata::Orientation::NoTransforms);
            let mut pixels = vec![0; size];
            decoder.decode_into(&mut pixels).map_err(|_| InvalidImage)?;
            (
                DynamicImage::ImageRgb8(
                    RgbImage::from_raw(width as u32, height as u32, pixels).ok_or(InvalidImage)?,
                ),
                orientation,
            )
        }
        ImageFormat::Png => {
            validate_png(data)?;
            let mut decoder =
                image::codecs::png::PngDecoder::with_limits(Cursor::new(data), limits.clone())
                    .map_err(|_| InvalidImage)?;
            if decoder.is_apng().map_err(|_| InvalidImage)? {
                return Err(InvalidImage);
            }
            let (width, height) = decoder.dimensions();
            if width == 0
                || height == 0
                || u64::from(width) * u64::from(height) > MAX_PIXELS
                || decoder.total_bytes() > MAX_ALLOC
            {
                return Err(InvalidImage);
            }
            decoder.set_limits(limits).map_err(|_| InvalidImage)?;
            let orientation = decoder.orientation().map_err(|_| InvalidImage)?;
            (
                DynamicImage::from_decoder(decoder).map_err(|_| InvalidImage)?,
                orientation,
            )
        }
        // This endpoint supports only static JPEG/PNG; WebP is not supported.
        _ => return Err(InvalidImage),
    };
    decoded.apply_orientation(orientation);
    // Resize before conversion/flattening so temporary RGB/RGBA buffers are small.
    let resized = if decoded.width().max(decoded.height()) > 1568 {
        decoded.resize(1568, 1568, image::imageops::FilterType::Triangle)
    } else {
        decoded
    }
    .to_rgba8();
    let mut rgb = RgbImage::new(resized.width(), resized.height());
    for (x, y, p) in resized.enumerate_pixels() {
        let a = u32::from(p[3]);
        rgb.put_pixel(
            x,
            y,
            Rgb([0, 1, 2].map(|c| ((u32::from(p[c]) * a + 255 * (255 - a) + 127) / 255) as u8)),
        );
    }
    let mut output = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 85)
        .encode_image(&rgb)
        .map_err(|_| InvalidImage)?;
    if output.is_empty() || output.len() > MAX_IMAGE_BYTES {
        return Err(InvalidImage);
    }
    Ok(NormalizedVendorImage {
        data: output,
        width: rgb.width(),
        height: rgb.height(),
    })
}

// zune's strict mode still accepts a complete scan with a missing EOI.
// Walk only marker framing (not image data): APP/EXIF payload may itself
// contain FF D9, so searching for those two bytes is not a valid end check.
fn validate_jpeg_end(data: &[u8]) -> Result<(), InvalidImage> {
    let mut pos = 2;
    let mut saw_scan = false;
    loop {
        if data.get(pos) != Some(&0xff) {
            return Err(InvalidImage);
        }
        while data.get(pos) == Some(&0xff) {
            pos += 1;
        }
        let marker = *data.get(pos).ok_or(InvalidImage)?;
        pos += 1;
        match marker {
            0xd9 => return if saw_scan { Ok(()) } else { Err(InvalidImage) },
            0x00 | 0xd8 | 0xd0..=0xd7 => return Err(InvalidImage),
            0x01 => continue, // TEM has no length field.
            _ => {}
        }
        let length = data.get(pos..pos + 2).ok_or(InvalidImage)?;
        let length = u16::from_be_bytes([length[0], length[1]]) as usize;
        if length < 2 {
            return Err(InvalidImage);
        }
        pos = pos
            .checked_add(length)
            .filter(|&p| p <= data.len())
            .ok_or(InvalidImage)?;
        if marker != 0xda {
            continue;
        }
        saw_scan = true;
        // Entropy-coded bytes are opaque; only stuffing and restart framing
        // distinguish an embedded FF from the next real marker.
        loop {
            let byte = *data.get(pos).ok_or(InvalidImage)?;
            if byte != 0xff {
                pos += 1;
                continue;
            }
            let start = pos;
            while data.get(pos) == Some(&0xff) {
                pos += 1;
            }
            match *data.get(pos).ok_or(InvalidImage)? {
                0x00 | 0xd0..=0xd7 => pos += 1,
                _ => {
                    pos = start;
                    break;
                }
            }
        }
    }
}

// Validate the complete container (including IEND and animation chunks after
// IDAT); the image adapter's next_frame alone stops before the final chunks.
fn validate_png(data: &[u8]) -> Result<(), InvalidImage> {
    if data.len() < 33 || &data[12..16] != b"IHDR" {
        return Err(InvalidImage);
    }
    let width = u32::from_be_bytes(data[16..20].try_into().map_err(|_| InvalidImage)?);
    let height = u32::from_be_bytes(data[20..24].try_into().map_err(|_| InvalidImage)?);
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(InvalidImage);
    }
    let mut pos = 8usize;
    while let Some(header) = data.get(pos..pos + 8) {
        let len = u32::from_be_bytes(header[..4].try_into().map_err(|_| InvalidImage)?) as usize;
        let end = pos
            .checked_add(12)
            .and_then(|n| n.checked_add(len))
            .ok_or(InvalidImage)?;
        let chunk = data.get(pos + 4..end).ok_or(InvalidImage)?;
        let crc = u32::from_be_bytes(
            chunk[chunk.len() - 4..]
                .try_into()
                .map_err(|_| InvalidImage)?,
        );
        if crc32fast::hash(&chunk[..chunk.len() - 4]) != crc
            || matches!(&header[4..], b"acTL" | b"fcTL" | b"fdAT")
        {
            return Err(InvalidImage);
        }
        if &header[4..] == b"IEND" {
            return if len == 0 { Ok(()) } else { Err(InvalidImage) };
        }
        pos = end;
    }
    Err(InvalidImage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageEncoder, Rgba, RgbaImage};

    fn png(width: u32, height: u32) -> Vec<u8> {
        let pixels = RgbaImage::from_pixel(width, height, Rgba([255, 0, 0, 0]));
        let mut data = Vec::new();
        image::codecs::png::PngEncoder::new(&mut data)
            .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
            .unwrap();
        data
    }

    #[test]
    fn normalizes_png_white_alpha_resizes_and_strips_payload() {
        let mut data = png(2000, 1000);
        data.extend_from_slice(b"private trailing payload");
        let result = normalize(&data).unwrap();
        assert_eq!((result.width, result.height), (1568, 784));
        assert!(!result.data.windows(7).any(|s| s == b"private"));
        let decoded = image::load_from_memory(&result.data).unwrap().to_rgb8();
        assert!(decoded.get_pixel(0, 0).0.iter().all(|&c| c > 250));
        assert!(result.data.len() <= MAX_IMAGE_BYTES);
    }

    #[test]
    fn jpeg_is_decoded_completely_and_orientation_applied() {
        let rgb = RgbImage::from_pixel(3, 2, Rgb([200, 80, 40]));
        let mut data = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut data)
            .encode_image(&rgb)
            .unwrap();
        assert_eq!(
            (
                normalize(&data).unwrap().width,
                normalize(&data).unwrap().height
            ),
            (3, 2)
        );
        for n in [0, 3, data.len() / 2, data.len() - 2] {
            assert!(normalize(&data[..n]).is_err(), "length {n}");
        }
        let mut fake_end = vec![0xff, 0xd8, 0xff, 0xed, 0, 4, 0xff, 0xd9];
        fake_end.extend_from_slice(&data[2..data.len() - 2]);
        assert!(normalize(&fake_end).is_err());
        let mut trailing = data.clone();
        trailing.extend_from_slice(b"private trailing payload");
        assert_eq!(normalize(&trailing).unwrap().width, 3);
        // Little-endian TIFF, one Orientation=6 entry, in a JPEG APP1 segment.
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut oriented = data[..2].to_vec();
        oriented.extend_from_slice(&[0xff, 0xe1]);
        oriented.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        oriented.extend_from_slice(exif);
        oriented.extend_from_slice(&data[2..]);
        let result = normalize(&oriented).unwrap();
        assert_eq!((result.width, result.height), (2, 3));
        assert!(!result.data.windows(4).any(|b| b == b"Exif"));
    }

    #[test]
    fn png_exif_orientation_is_applied_and_webp_is_unsupported() {
        let exif = b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let rgb = RgbImage::from_pixel(3, 2, Rgb([200, 80, 40]));
        let mut data = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new(&mut data);
        encoder.set_exif_metadata(exif.to_vec()).unwrap();
        encoder
            .write_image(&rgb, 3, 2, image::ExtendedColorType::Rgb8)
            .unwrap();
        let normalized = normalize(&data).unwrap();
        assert_eq!((normalized.width, normalized.height), (2, 3));
        // Real lossless 1x1 WebP fixture; no WebP codec dependency is needed.
        let webp = b"RIFF\x1e\x00\x00\x00WEBPVP8L\x11\x00\x00\x00/\x00\x00\x00\x00\x07P\xa8\x22\x17\xa5\xff\x81\x88\xe8\x7f\x00\x00";
        assert_eq!(image::guess_format(webp).unwrap(), ImageFormat::WebP);
        assert!(normalize(webp).is_err());
    }

    #[test]
    fn jpeg_header_dimensions_and_pixels_are_checked_before_pixel_allocation() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&RgbImage::new(2, 2))
            .unwrap();
        let sof = jpeg.windows(2).position(|b| b == [0xff, 0xc0]).unwrap();
        for (width, height) in [(8193u16, 1u16), (1, 8193), (5000, 4000)] {
            let mut oversized = jpeg.clone();
            oversized[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
            oversized[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
            assert!(normalize(&oversized).is_err());
        }
    }

    #[test]
    fn rejects_corruption_animation_dimensions_and_unsupported_formats() {
        for bytes in [
            &b"<html>not an image"[..],
            b"GIF89a",
            b"\xff\xd8\xff",
            b"RIFF\0\0\0\0WEBP",
            b"<svg/>",
        ] {
            assert!(normalize(bytes).is_err());
        }
        let valid = png(2, 2);
        assert!(normalize(&valid[..valid.len() - 12]).is_err());
        let mut corrupt = valid.clone();
        corrupt[25] ^= 1;
        assert!(normalize(&corrupt).is_err());
        let mut animation = valid[..33].to_vec();
        let chunk = b"acTL\0\0\0\x02\0\0\0\0";
        animation.extend_from_slice(&8u32.to_be_bytes());
        animation.extend_from_slice(chunk);
        animation.extend_from_slice(&crc32fast::hash(chunk).to_be_bytes());
        animation.extend_from_slice(&valid[33..]);
        assert!(normalize(&animation).is_err());
        for (w, h) in [(8193u32, 1u32), (8192, 8192), (0, 2)] {
            let mut huge = valid.clone();
            huge[16..20].copy_from_slice(&w.to_be_bytes());
            huge[20..24].copy_from_slice(&h.to_be_bytes());
            let crc = crc32fast::hash(&huge[12..29]);
            huge[29..33].copy_from_slice(&crc.to_be_bytes());
            assert!(normalize(&huge).is_err());
        }
        assert!(normalize(&vec![0; MAX_IMAGE_BYTES + 1]).is_err());
    }
}
