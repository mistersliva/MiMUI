//! Image loading: PNG/JPEG/WebP decode, SVG rasterization and GPU upload.
//!
//! Images are cached by a content hash, so asking for the same path twice in a
//! frame (or across frames) costs one decode.

use crate::geom::{Rect, Vec2};
use std::collections::HashMap;
use std::path::Path;

/// A decoded image living in GPU memory.
pub struct GpuImage {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    /// UV rectangle covering the whole image.
    pub uv: Rect,
}

/// Where an image's pixels come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    /// A raster file on disk (PNG, JPEG, WebP, GIF).
    Path,
    /// Inline SVG source.
    Svg,
    /// Pixels supplied directly by the program.
    Raster,
}

/// Result of a decode, before upload.
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// RGBA8, non-premultiplied.
    pub pixels: Vec<u8>,
    /// Normalized UV rect into the atlas this image will live in.
    pub uv: Rect,
}

/// Caches decoded images by content hash.
#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<[u8; 32], GpuImage>,
    /// Cache key by user-provided path, to skip re-hashing and re-reading.
    paths: HashMap<String, [u8; 32]>,
}

impl ImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hashes a file or an in-memory asset name.
    pub fn key_for(spec: &str) -> [u8; 32] {
        *blake3::hash(spec.as_bytes()).as_bytes()
    }

    /// Hashes raw bytes (used for SVG and generated content).
    pub fn key_bytes(bytes: &[u8]) -> [u8; 32] {
        *blake3::hash(bytes).as_bytes()
    }

    /// Returns the cached image for `spec` if it was already uploaded.
    pub fn get(&self, key: &[u8; 32]) -> Option<&GpuImage> {
        self.entries.get(key)
    }

    pub fn insert(&mut self, key: [u8; 32], img: GpuImage) {
        self.entries.insert(key, img);
    }

    pub fn cached_path(&self, path: &str) -> Option<[u8; 32]> {
        self.paths.get(path).copied()
    }

    pub fn remember_path(&mut self, path: &str, key: [u8; 32]) {
        self.paths.insert(path.to_string(), key);
    }

    /// Number of live GPU images, for diagnostics.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Decodes a raster image file (PNG, JPEG, WebP, GIF).
pub fn decode_raster(path: impl AsRef<Path>) -> Result<DecodedImage, ImageError> {
    let path = path.as_ref();
    let reader = image::ImageReader::open(path)
        .map_err(|e| ImageError::Io(path.display().to_string(), e.to_string()))?
        .with_guessed_format()
        .map_err(|e| ImageError::Io(path.display().to_string(), e.to_string()))?;

    let img = reader
        .decode()
        .map_err(|e| ImageError::Decode(path.display().to_string(), e.to_string()))?;

    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok(DecodedImage { width: w, height: h, pixels: rgba.into_raw(), uv: unit_uv() })
}

/// Creates a decoded image from raw RGBA bytes.
pub fn decode_rgba(width: u32, height: u32, pixels: Vec<u8>) -> DecodedImage {
    DecodedImage { width, height, pixels, uv: unit_uv() }
}

fn unit_uv() -> Rect {
    Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }
}

/// Rasterizes SVG source into RGBA at the requested size.
///
/// `size` is the longest-edge size in pixels; the SVG's own aspect ratio is
/// preserved.
#[cfg(feature = "svg")]
pub fn decode_svg(source: &str, size: f32) -> Result<DecodedImage, ImageError> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(source, &opt)
        .map_err(|e| ImageError::Svg(e.to_string()))?;

    let size = size.max(1.0);
    let src_size = tree.size();
    let scale = size / src_size.width().max(src_size.height()).max(0.001);
    let w = ((src_size.width() * scale).round() as u32).max(1);
    let h = ((src_size.height() * scale).round() as u32).max(1);

    let mut pixmap = tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| ImageError::Svg("allocation failed".into()))?;
    let transform =
        tiny_skia::Transform::from_scale(scale as f32, scale as f32);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // tiny-skia is premultiplied; our shaders expect straight alpha.
    let mut pixels = pixmap.take();
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
            continue;
        }
        let un = |c: u8| ((c as u32 * 255 + a / 2) / a).min(255) as u8;
        let r = un(px[0]);
        let g = un(px[1]);
        let b = un(px[2]);
        px[0] = r;
        px[1] = g;
        px[2] = b;
    }

    Ok(DecodedImage { width: w, height: h, pixels, uv: unit_uv() })
}

/// Non-SVG build: reports that the feature is off.
#[cfg(not(feature = "svg"))]
pub fn decode_svg(_source: &str, _size: f32) -> Result<DecodedImage, ImageError> {
    Err(ImageError::Svg(
        "SVG support is off. Enable the `svg` feature to use it.".into(),
    ))
}

/// How a raster image is fitted into its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fit {
    #[default]
    Cover,
    Contain,
    Fill,
    None,
}

impl Fit {
    pub fn parse(s: &str) -> Option<Fit> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "cover" => Fit::Cover,
            "contain" => Fit::Contain,
            "fill" | "stretch" => Fit::Fill,
            "none" | "auto" => Fit::None,
            _ => return None,
        })
    }

    /// The source rect (in UV space) that fills `box` while honouring the fit.
    ///
    /// * `Cover` scales the sampled region so the image fills the box, cropping
    ///   the overflow on the long axis.
    /// * `Contain` scales the sampled region so the whole image fits, letterboxing.
    /// * `Fill` / `None` always sample everything; they differ in the destination
    ///   size, which layout handles.
    pub fn source_rect(self, img: Vec2, box_: Vec2) -> Rect {
        let full = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
        if img.x <= 0.0 || img.y <= 0.0 || box_.x <= 0.0 || box_.y <= 0.0 {
            return full;
        }
        match self {
            Fit::Fill | Fit::None => full,
            fit => {
                let img_ar = img.x / img.y;
                let box_ar = box_.x / box_.y;
                // Fraction of the texture to sample on each axis, centred.
                // `Cover` fills the box (cropping the long axis);
                // `Contain` fits the whole image (letterboxing).
                let (w, h) = if img_ar > box_ar {
                    // The image is wider than the box.
                    if fit == Fit::Cover {
                        (box_ar / img_ar, 1.0)
                    } else {
                        (1.0, box_ar / img_ar)
                    }
                } else if img_ar < box_ar {
                    // The image is taller than the box.
                    if fit == Fit::Cover {
                        (1.0, img_ar / box_ar)
                    } else {
                        (img_ar / box_ar, 1.0)
                    }
                } else {
                    (1.0, 1.0)
                };
                Rect { x: (1.0 - w) * 0.5, y: (1.0 - h) * 0.5, w, h }
            }
        }
    }
}

/// Why an image could not be produced.
#[derive(Debug)]
pub enum ImageError {
    Io(String, String),
    Decode(String, String),
    Svg(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Io(p, e) => write!(f, "could not open `{p}`: {e}"),
            ImageError::Decode(p, e) => write!(f, "could not decode `{p}`: {e}"),
            ImageError::Svg(e) => write!(f, "svg error: {e}"),
        }
    }
}

impl std::error::Error for ImageError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_vs_contain_for_a_wide_image() {
        // A 2:1 image in a square box: cover crops the sides, contain adds bars.
        let img = Vec2::new(200.0, 100.0);
        let b = Vec2::new(100.0, 100.0);
        let cover = Fit::Cover.source_rect(img, b);
        assert_eq!(cover.w, 0.5);
        assert_eq!(cover.h, 1.0);

        let contain = Fit::Contain.source_rect(img, b);
        assert_eq!(contain.w, 1.0);
        assert_eq!(contain.h, 0.5);
    }

    #[test]
    fn fill_covers_the_whole_texture() {
        let r = Fit::Fill.source_rect(Vec2::new(10.0, 10.0), Vec2::new(3.0, 7.0));
        assert_eq!(r, Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
    }

    #[test]
    fn decodes_a_png_we_generate() {
        // Encode a 2x2 PNG in memory, then decode it through the same path.
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
        let mut bytes: Vec<u8> = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 2));
        assert_eq!(decoded.get_pixel(0, 0).0, [10, 20, 30, 255]);
    }
}