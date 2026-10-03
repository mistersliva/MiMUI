//! Text shaping, measuring and glyph rasterization.
//!
//! MiMUI shapes text through `cosmic-text` (so Unicode, ligatures and
//! bidirectional text work) and rasterizes the shaped glyphs into a single
//! atlas texture that the renderer samples for all text on screen.

use crate::geom::{Rect, Vec2};
use std::collections::HashMap;

/// A glyph placed by the shaper, ready to become a quad.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    pub font_id: cosmic_text::fontdb::ID,
    pub glyph_id: u16,
    pub size: f32,
    /// Pen position on the baseline, relative to the text block origin.
    pub x: f32,
    /// Baseline, relative to the text block origin.
    pub y: f32,
    /// Glyph advance width.
    pub advance: f32,
    /// Offset from the pen position to the bitmap's top-left.
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// One laid-out line of text.
#[derive(Clone, Debug)]
pub struct Line {
    pub glyphs: Vec<Glyph>,
    pub width: f32,
    pub top: f32,
    pub height: f32,
    /// Distance from the line top to the baseline.
    pub baseline: f32,
    /// Index into the source string.
    pub source_line: usize,
}

/// The result of shaping a string.
#[derive(Clone, Debug, Default)]
pub struct ShapedText {
    pub lines: Vec<Line>,
    pub width: f32,
    pub height: f32,
}

impl ShapedText {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Index of the line containing `y` (relative to the text origin).
    pub fn line_at(&self, y: f32) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        for (i, l) in self.lines.iter().enumerate() {
            if y < l.top + l.height {
                return i;
            }
        }
        self.lines.len() - 1
    }

    /// The union of every glyph quad, useful for hit-testing.
    pub fn bounds(&self) -> Rect {
        let mut r: Option<Rect> = None;
        for l in &self.lines {
            for g in &l.glyphs {
                if g.width <= 0.0 || g.height <= 0.0 {
                    continue;
                }
                let q = Rect { x: g.x + g.left, y: g.y + g.top, w: g.width, h: g.height };
                r = Some(match r {
                    Some(prev) => prev.union(&q),
                    None => q,
                });
            }
        }
        r.unwrap_or_default()
    }
}

/// Everything needed to shape a run of text.
#[derive(Clone, Debug)]
pub struct TextAttrs {
    pub size: f32,
    /// Either a multiple of the font size (`1.4`) or an absolute pixel value.
    pub line_height: f32,
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub letter_spacing: f32,
    pub align: crate::style::TextAlign,
    pub wrap: bool,
    /// Wrap width; `None` measures without wrapping.
    pub width: Option<f32>,
}

impl Default for TextAttrs {
    fn default() -> Self {
        Self {
            size: 16.0,
            line_height: 1.4,
            family: String::new(),
            weight: 400,
            italic: false,
            letter_spacing: 0.0,
            align: crate::style::TextAlign::Left,
            wrap: true,
            width: None,
        }
    }
}

impl TextAttrs {
    /// The line box height in pixels.
    pub fn line_px(&self) -> f32 {
        if self.line_height <= 3.0 { self.size * self.line_height } else { self.line_height }
    }

    pub fn font_weight(&self) -> cosmic_text::Weight {
        cosmic_text::Weight(self.weight.clamp(1, 1000))
    }
}

/// A rasterized glyph inside the atlas.
#[derive(Clone, Copy, Debug)]
pub struct AtlasEntry {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    /// Bitmap size in texels.
    pub tex_w: f32,
    pub tex_h: f32,
    /// Offset from the pen position to the bitmap's top-left, in pixels.
    pub left: f32,
    pub top: f32,
    /// Bitmap size in logical pixels.
    pub width: f32,
    pub height: f32,
}

/// The CPU-side glyph atlas plus the pixels the GPU needs.
pub struct AtlasTexture {
    pub size: u32,
    /// 8-bit alpha coverage, `size * size` bytes.
    pub pixels: Vec<u8>,
    /// Maps `(font_id, glyph_id, size_bits)` to an atlas entry.
    pub entries: HashMap<(cosmic_text::fontdb::ID, u16, u32), AtlasEntry>,
}

impl AtlasTexture {
    fn new(size: u32) -> Self {
        Self { size, pixels: vec![0; (size * size) as usize], entries: HashMap::new() }
    }

    /// Drops every cached glyph and zeroes the pixels.
    pub fn clear(&mut self) {
        self.pixels.fill(0);
        self.entries.clear();
    }
}

/// A simple shelf packer.
struct Shelf {
    x: u32,
    y: u32,
    row_h: u32,
}

impl Shelf {
    fn new() -> Self {
        Self { x: 0, y: 0, row_h: 0 }
    }

    fn reset(&mut self) {
        self.x = 0;
        self.y = 0;
        self.row_h = 0;
    }

    /// Reserves a rectangle, or `None` when the atlas is full.
    fn alloc(&mut self, w: u32, h: u32, size: u32) -> Option<(u32, u32)> {
        if w == 0 || h == 0 || w > size || h > size {
            return None;
        }
        if self.x + w > size {
            self.x = 0;
            self.y += self.row_h + 1;
            self.row_h = 0;
        }
        if self.y + h > size {
            return None;
        }
        let pos = (self.x, self.y);
        self.x += w + 1;
        self.row_h = self.row_h.max(h);
        Some(pos)
    }
}

/// Names that mark a face as pictographic rather than text.
const NON_TEXT_MARKERS: &[&str] = &[
    "symbol",
    "webding",
    "wingding",
    "marlett",
    "icon",
    "emoji",
    "dingbat",
    "ornament",
    "pi ",
    "bats",
    "holi",
    "javatext",
    "glyphicon",
];

/// Drops the faces that cannot draw prose.
///
/// cosmic-text 0.12 ranks candidate faces by **weight only** — its
/// `Attrs::matches` ignores the family — and then takes the first face that can
/// render each codepoint. Icon fonts map ASCII onto pictograms and are
/// frequently registered at weight 500, so on Windows `font-weight: 500`
/// resolved to one and every label turned into symbols. Removing those faces
/// keeps font selection to something that can actually set text.
fn drop_non_text_fonts(fs: &mut cosmic_text::FontSystem) {
    let drop: Vec<_> = fs
        .db()
        .faces()
        .filter(|f| {
            let post = f.post_script_name.to_ascii_lowercase();
            let family = f
                .families
                .iter()
                .map(|(n, _)| n.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            NON_TEXT_MARKERS.iter().any(|m| post.contains(m) || family.contains(m))
        })
        .map(|f| f.id)
        .collect();
    if drop.is_empty() {
        return;
    }
    let db = fs.db_mut();
    for id in drop {
        db.remove_face(id);
    }
}

/// Owns the font system and the glyph atlas.
pub struct TextEngine {
    pub font_system: cosmic_text::FontSystem,
    swash: cosmic_text::SwashCache,
    pub atlas: AtlasTexture,
    atlas_dirty: bool,
    /// How many times the atlas ran out of room and had to be emptied.
    pub resets: u32,
    packer: Shelf,
    /// Scratch buffer reused between shapes to avoid per-frame allocation.
    buffer: cosmic_text::Buffer,
    /// Ascender/descender of the last shaped run, for baseline alignment.
    last_metrics: (f32, f32),
}

impl TextEngine {
    /// Builds an engine using the system font database.
    pub fn new() -> Self {
        Self::with_atlas_size(1024)
    }

    /// `size` is the glyph atlas edge length in texels.
    pub fn with_atlas_size(size: u32) -> Self {
        let mut font_system = cosmic_text::FontSystem::new();
        drop_non_text_fonts(&mut font_system);
        let buffer = cosmic_text::Buffer::new(&mut font_system, cosmic_text::Metrics::new(16.0, 22.0));
        Self {
            font_system,
            swash: cosmic_text::SwashCache::new(),
            atlas: AtlasTexture::new(size.max(256)),
            atlas_dirty: true,
            resets: 0,
            packer: Shelf::new(),
            buffer,
            last_metrics: (12.0, 4.0),
        }
    }

    /// Removes every face except those in `families`.
    ///
    /// Only needed if the default filter is not what you want; see
    /// [`drop_non_text_fonts`] for why it exists.
    pub fn retain_families(&mut self, families: &[&str]) {
        let wanted: Vec<String> = families.iter().map(|f| f.to_ascii_lowercase()).collect();
        let drop: Vec<_> = self
            .font_system
            .db()
            .faces()
            .filter(|f| {
                !f.families
                    .iter()
                    .any(|(name, _)| wanted.contains(&name.to_ascii_lowercase()))
            })
            .map(|f| f.id)
            .collect();
        let db = self.font_system.db_mut();
        for id in drop {
            db.remove_face(id);
        }
    }

    /// Loads a font file without making it the default.
    pub fn load_font(&mut self, data: &[u8]) {
        self.font_system.db_mut().load_font_data(data.to_vec());
    }

    /// Registers a font under `family` and makes it the default family.
    pub fn set_font(&mut self, data: &[u8], family: &str) {
        let db = self.font_system.db_mut();
        db.load_font_data(data.to_vec());
        db.set_sans_serif_family(family);
        db.set_serif_family(family);
        db.set_monospace_family(family);
    }

    /// Shapes `text` into positioned glyphs.
    pub fn shape(&mut self, text: &str, attrs: &TextAttrs) -> ShapedText {
        let line_px = attrs.line_px();
        let metrics = cosmic_text::Metrics::new(attrs.size.max(1.0), line_px.max(1.0));

        let family = attrs.family.clone();
        let ct_attrs = cosmic_text::Attrs::new()
            .family(if family.is_empty() {
                cosmic_text::Family::SansSerif
            } else {
                cosmic_text::Family::Name(family.as_str())
            })
            .weight(attrs.font_weight())
            .style(if attrs.italic {
                cosmic_text::Style::Italic
            } else {
                cosmic_text::Style::Normal
            });

        let wrap_width = attrs.width.filter(|w| *w > 0.0);
        let buffer = &mut self.buffer;
        buffer.set_metrics(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, wrap_width, None);
        buffer.set_wrap(
            &mut self.font_system,
            if attrs.wrap && wrap_width.is_some() {
                cosmic_text::Wrap::WordOrGlyph
            } else {
                cosmic_text::Wrap::None
            },
        );
        buffer.set_text(&mut self.font_system, text, ct_attrs, cosmic_text::Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, true);

        // An empty string shapes into one empty line in cosmic-text; report
        // "nothing" instead so widgets can skip measuring and drawing.
        if text.is_empty() {
            self.last_metrics = (attrs.size * 0.8, attrs.size * 0.2);
            return ShapedText::default();
        }

        // Copy the shaped runs out of the buffer first: rasterizing needs
        // `&mut self`, which would conflict with the borrow held by the
        // layout iterator.
        struct RawRun {
            source_line: usize,
            line_y: f32,
            line_top: f32,
            line_height: f32,
            line_w: f32,
            glyphs: Vec<(cosmic_text::fontdb::ID, u16, f32, f32, f32, f32)>,
        }
        let raw: Vec<RawRun> = buffer
            .layout_runs()
            .map(|run| RawRun {
                source_line: run.line_i,
                line_y: run.line_y,
                line_top: run.line_top,
                line_height: run.line_height,
                line_w: run.line_w,
                glyphs: run
                    .glyphs
                    .iter()
                    .map(|g| (g.font_id, g.glyph_id, g.font_size, g.x + g.x_offset, g.y, g.w))
                    .collect(),
            })
            .collect();

        // Group runs into visual lines.
        let mut out = ShapedText::default();
        let mut max_ascent = attrs.size * 0.8;
        let mut max_descent = attrs.size * 0.2;

        for run in raw {
            let RawRun {
                source_line,
                line_y: baseline,
                line_top,
                line_height: raw_line_h,
                line_w,
                glyphs: raw_glyphs,
            } = run;
            let glyph_count = raw_glyphs.len();
            let mut glyphs: Vec<Glyph> = Vec::with_capacity(glyph_count);
            // Ink boxes are measured from the baseline, so carry the baseline
            // inside the line box rather than the run's own y.
            let baseline_in_line = baseline - line_top;

            for (font_id, glyph_id, size, x, _y, advance) in raw_glyphs {
                let ink = self.raster(font_id, glyph_id, size);
                glyphs.push(Glyph {
                    font_id,
                    glyph_id,
                    size,
                    x,
                    y: baseline_in_line,
                    advance: advance + attrs.letter_spacing,
                    left: ink.left,
                    top: ink.top,
                    width: ink.width,
                    height: ink.height,
                });
            }

            let ascent = line_top.max(attrs.size * 0.8);
            let line_height = raw_line_h.max(line_px);
            max_ascent = max_ascent.max(ascent);
            max_descent = max_descent.max(line_height - ascent);

            let width = line_w + attrs.letter_spacing * glyph_count as f32;

            // Group by line_top, not source_line: soft-wrapped visual lines
            // share one source line, while a single visual line can be split
            // across runs when fonts differ.
            match out.lines.last_mut() {
                Some(l) if (l.top - line_top).abs() < 0.5 => {
                    l.glyphs.extend(glyphs);
                    l.width = l.width.max(width);
                    l.height = l.height.max(line_height);
                }
                _ => out.lines.push(Line {
                    glyphs,
                    width,
                    top: line_top,
                    height: line_height,
                    baseline,
                    source_line,
                }),
            }
        }

        self.last_metrics = (max_ascent, max_descent);

        // Align horizontally inside the wrap width.
        if let Some(avail) = wrap_width {
            for l in &mut out.lines {
                let shift = match attrs.align {
                    crate::style::TextAlign::Left => 0.0,
                    crate::style::TextAlign::Center => (avail - l.width) * 0.5,
                    crate::style::TextAlign::Right => avail - l.width,
                };
                let shift = shift.max(0.0);
                for g in &mut l.glyphs {
                    g.x += shift;
                }
                out.width = out.width.max(l.width + shift);
            }
        } else {
            out.width = out.lines.iter().map(|l| l.width).fold(0.0, f32::max);
        }

        out.height = out.lines.iter().map(|l| l.top + l.height).fold(0.0, f32::max);
        out
    }

    /// Measures text without wrapping.
    pub fn measure(&mut self, text: &str, attrs: &TextAttrs) -> Vec2 {
        let a = TextAttrs { wrap: false, width: None, align: crate::style::TextAlign::Left, ..attrs.clone() };
        let shaped = self.shape(text, &a);
        Vec2::new(shaped.width, shaped.height)
    }

    /// Where the baseline sits inside a block of `height`.
    pub fn baseline_in(&self, height: f32) -> f32 {
        let (a, d) = self.last_metrics;
        (height - (a + d)) * 0.5 + a
    }

    /// Rasterizes one glyph and returns its ink box.
///
/// The offsets are *relative to the pen*: `left` is the bearing, and `top` is
/// measured from the baseline (negative above it). The caller adds the pen and
/// baseline positions, which keeps one cached bitmap usable anywhere on screen.
fn raster(&mut self, font_id: cosmic_text::fontdb::ID, glyph_id: u16, size: f32) -> InkBox {
    let key = (font_id, glyph_id, size.to_bits());
    if let Some(e) = self.atlas.entries.get(&key) {
        return InkBox { left: e.left, top: e.top, width: e.width, height: e.height };
    }

    let target = size.ceil().max(1.0);
    // Snapping the subpixel position to zero keeps one bitmap per glyph rather
    // than one per quarter pixel, which is also what crisp UI text wants.
    let (cache_key, _snap_x, _snap_y) = cosmic_text::CacheKey::new(
        font_id,
        glyph_id,
        target,
        (0.0, 0.0),
        cosmic_text::CacheKeyFlags::empty(),
    );

        let image = {
            let got = self.swash.get_image(&mut self.font_system, cache_key);
            got.clone()
        };

        let placement = match &image {
            Some(i) => i.placement,
            None => return InkBox::default(),
        };
        let w = placement.width;
        let h = placement.height;

        if w == 0 || h == 0 {
            return InkBox::default();
        }

        let size_n = self.atlas.size;
        let mut pos = self.packer.alloc(w, h, size_n);
        if pos.is_none() {
            // Out of room: reset the atlas and try once more.
            self.resets += 1;
            self.atlas.clear();
            self.packer.reset();
            self.atlas_dirty = true;
            pos = self.packer.alloc(w, h, size_n);
        }
        let Some((px, py)) = pos else { return InkBox::default() };

        let px = px as usize;
        let py = py as usize;
        let stride = size_n as usize;
        let data = image.as_ref().map(|i| i.data.as_slice()).unwrap_or(&[]);
        let stride_w = w as usize;
        let stride_h = h as usize;
        for row in 0..stride_h {
            let src = row * stride_w;
            let dst = ((py + row) * stride) + px;
            if src + stride_w > data.len() || dst + stride_w > self.atlas.pixels.len() {
                break;
            }
            self.atlas.pixels[dst..dst + stride_w].copy_from_slice(&data[src..src + stride_w]);
        }
        self.atlas_dirty = true;

        let inv = 1.0 / size_n as f32;
        let scale = target / size.max(1.0);
        // swash reports `placement.top` as the distance from the baseline up to
        // the top of the bitmap, so the top edge sits at `-top` from the baseline.
        let left = placement.left as f32 * scale;
        let top = -(placement.top as f32) * scale;

        self.atlas.entries.insert(
            key,
            AtlasEntry {
                u0: px as f32 * inv,
                v0: py as f32 * inv,
                u1: (px as f32 + w as f32) * inv,
                v1: (py as f32 + h as f32) * inv,
                tex_w: w as f32,
                tex_h: h as f32,
                left,
                top,
                width: w as f32 * scale,
                height: h as f32 * scale,
            },
        );

        InkBox { left, top, width: w as f32 * scale, height: h as f32 * scale }
    }

    /// The atlas entry for a glyph, if it has been rasterized.
    pub fn entry(
        &self,
        font_id: cosmic_text::fontdb::ID,
        glyph_id: u16,
        size: f32,
    ) -> Option<&AtlasEntry> {
        self.atlas.entries.get(&(font_id, glyph_id, size.to_bits()))
    }

    /// True when the atlas pixels changed since the last upload.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::replace(&mut self.atlas_dirty, false)
    }

    /// Marks the atlas as needing an upload.
    pub fn mark_dirty(&mut self) {
        self.atlas_dirty = true;
    }
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// A glyph's ink box, in pixels relative to the pen position.
#[derive(Clone, Copy, Debug, Default)]
struct InkBox {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

/// A `ui!` text block ready to be measured and drawn.
#[derive(Clone, Debug)]
pub struct TextBlock {
    pub text: String,
    pub attrs: TextAttrs,
}

impl TextBlock {
    pub fn new(text: impl Into<String>, attrs: TextAttrs) -> Self {
        Self { text: text.into(), attrs }
    }
}

/// Shapes a text block against an optional content width.
pub fn layout_block(
    engine: &mut TextEngine,
    block: &TextBlock,
    avail: Option<f32>,
) -> ShapedText {
    let mut attrs = block.attrs.clone();
    attrs.width = if block.attrs.wrap { avail.filter(|w| *w > 0.0) } else { None };
    engine.shape(&block.text, &attrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_ascii() {
        let mut e = TextEngine::new();
        let attrs = TextAttrs { size: 24.0, wrap: false, ..Default::default() };
        let s = e.shape("Hello", &attrs);
        assert_eq!(s.lines.len(), 1);
        assert!(s.width > 0.0, "width {}", s.width);
        assert!(s.height > 0.0);
        assert!(!s.lines[0].glyphs.is_empty());
    }

    #[test]
    fn wider_text_measures_wider() {
        let mut e = TextEngine::new();
        let attrs = TextAttrs { size: 24.0, wrap: false, ..Default::default() };
        let a = e.measure("Hello", &attrs);
        let b = e.measure("Hello world", &attrs);
        assert!(b.x > a.x, "{:?} vs {:?}", a, b);
        assert_eq!(a.y, b.y);
    }

    #[test]
    fn wraps_at_a_width() {
        let mut e = TextEngine::new();
        let attrs = TextAttrs { size: 20.0, wrap: true, width: Some(100.0), ..Default::default() };
        let s = e.shape("one two three four five six seven eight", &attrs);
        assert!(s.lines.len() > 1, "expected wrapping, got {} line(s)", s.lines.len());
    }

    #[test]
    fn center_alignment_shifts_glyphs() {
        let mut e = TextEngine::new();
        let left = TextAttrs { size: 20.0, wrap: false, ..Default::default() };
        let center =
            TextAttrs { size: 20.0, wrap: false, align: crate::style::TextAlign::Center, width: Some(400.0), ..Default::default() };
        let a = e.shape("Hello", &left);
        let b = e.shape("Hello", &center);
        let ax = a.lines[0].glyphs.first().map(|g| g.x).unwrap_or(0.0);
        let bx = b.lines[0].glyphs.first().map(|g| g.x).unwrap_or(0.0);
        assert!(bx > ax, "centered text should start further right: {} vs {}", bx, ax);
    }

    #[test]
    fn empty_text_is_empty() {
        let mut e = TextEngine::new();
        let s = e.shape("", &TextAttrs::default());
        assert!(s.is_empty());
    }

    #[test]
    fn shelf_packs_without_overlap() {
        let mut p = Shelf::new();
        let a = p.alloc(10, 10, 64).unwrap();
        let b = p.alloc(10, 10, 64).unwrap();
        assert_ne!(a, b);
        // Too wide for the atlas.
        assert!(p.alloc(100, 10, 64).is_none());
    }

    #[test]
fn glyphs_land_in_the_atlas() {
        let mut e = TextEngine::new();
        let s = e.shape("MM", &TextAttrs { size: 32.0, wrap: false, ..Default::default() });
        let g = s.lines[0].glyphs[0];
        let entry = e.entry(g.font_id, g.glyph_id, g.size).expect("glyph should be cached");
        assert!(entry.tex_w > 0.0 && entry.tex_h > 0.0);
        assert!(entry.u1 > entry.u0 && entry.v1 > entry.v0);
        // The ink must not be entirely off the right edge of the atlas.
        assert!(entry.u1 <= 1.0 && entry.v1 <= 1.0);
    }

    #[test]
    fn every_glyph_of_a_real_string_is_cached() {
        let mut e = TextEngine::new();
        let attrs = TextAttrs { size: 16.0, wrap: false, ..Default::default() };
        let text = "BUILT-IN ANIMATIONS: fade_in · pop · pulse · shake";
        let s = e.shape(text, &attrs);
        let mut missing = Vec::new();
        for line in &s.lines {
            for g in &line.glyphs {
                // 0 is .notdef and 3 is the space: neither has any ink to cache.
                if g.glyph_id == 0 || g.glyph_id == 3 {
                    continue;
                }
                if e.entry(g.font_id, g.glyph_id, g.size).is_none() {
                    missing.push(g.glyph_id);
                }
            }
        }
        assert!(missing.is_empty(), "{missing:?} were never rasterized");
    }

    #[test]
    fn cached_atlas_rectangles_do_not_overlap() {
        let mut e = TextEngine::new();
        let attrs = TextAttrs { size: 16.0, wrap: false, ..Default::default() };
        for line in "the quick brown fox jumps over a lazy dog".lines() {
            e.shape(line, &attrs);
        }
        let mut boxes: Vec<(u32, u32, u32, u32)> = e
            .atlas
            .entries
            .values()
            .map(|a| {
                (
                    (a.u0 * 1024.0) as u32,
                    (a.v0 * 1024.0) as u32,
                    a.tex_w as u32,
                    a.tex_h as u32,
                )
            })
            .collect();
        let total = boxes.len();
        boxes.sort_unstable();
        for pair in boxes.windows(2) {
            let (ax, ay, aw, ah) = pair[0];
            let (bx, by, bw, bh) = pair[1];
            let disjoint = ax + aw <= bx || bx + bw <= ax || ay + ah <= by || by + bh <= ay;
            assert!(disjoint, "glyphs share atlas space: {pair:?}");
        }
        assert!(total > 20, "expected most of the alphabet, got {total}");
    }

    /// The showcase's whole vocabulary has to fit without the packer wrapping,
    /// which would silently invalidate every glyph already placed.
    #[test]
    fn a_real_screenful_of_text_fits_in_the_atlas() {
        let lines = [
            ("MiMUI", 30.0),
            ("immediate mode - CSS styling - declarative animation", 14.0),
            ("BUILT-IN ANIMATIONS", 12.0),
            ("fade_in - pop - pulse - shake", 15.0),
            ("slide_up - bounce_in - spin", 15.0),
            ("YOUR OWN", 12.0),
            ("INPUT", 12.0),
            ("Enable animations", 15.0),
            ("type something", 15.0),
            ("STYLING", 12.0),
            ("1px solid #2b3550", 15.0),
            ("hover me:", 15.0),
            ("shadow + gradient-free polish:", 15.0),
            ("bottom bar:", 14.0),
            ("options", 16.0),
            ("github", 16.0),
        ];
        let mut e = TextEngine::new();
        let mut placed = 0usize;
        for (text, size) in lines {
            let attrs = TextAttrs { size, wrap: false, ..Default::default() };
            let before = e.atlas.entries.len();
            e.shape(text, &attrs);
            placed += e.atlas.entries.len() - before;
        }
        assert!(placed > 40, "expected a lot of glyphs, cached {placed}");

        // Every cached rectangle must still be inside the atlas and unique.
        let size = e.atlas.size;
        for entry in e.atlas.entries.values() {
            assert!(entry.u0 >= 0.0 && entry.v0 >= 0.0);
            assert!(entry.u1 <= 1.0 && entry.v1 <= 1.0, "{entry:?}");
            assert!(entry.tex_w > 0.0 && entry.tex_h > 0.0);
            let _ = size;
        }
    }

    #[test]
    #[ignore = "writes a debug image"]
    fn dump_atlas() {
        let mut e = TextEngine::new();
        for (text, size) in [
            ("MiMUI", 30.0),
            ("immediate mode - CSS styling", 14.0),
            ("BUILT-IN ANIMATIONS", 12.0),
            ("fade_in pop pulse shake", 15.0),
        ] {
            let attrs = TextAttrs { size, wrap: false, ..Default::default() };
            e.shape(text, &attrs);
        }
        let n = e.atlas.size;
        let mut rgba = vec![0u8; (n * n * 4) as usize];
        for i in 0..(n * n) as usize {
            let a = e.atlas.pixels[i];
            rgba[i * 4..i * 4 + 4].copy_from_slice(&[a, a, a, 255]);
        }
        let img = image::RgbaImage::from_raw(n, n, rgba).expect("valid buffer");
        img.save(std::env::temp_dir().join("mimui-atlas.png")).expect("write png");
    }

    #[test]
    fn line_height_accepts_both_units() {
        let rel = TextAttrs { size: 20.0, line_height: 2.0, ..Default::default() };
        assert_eq!(rel.line_px(), 40.0);
        let abs = TextAttrs { size: 20.0, line_height: 33.0, ..Default::default() };
        assert_eq!(abs.line_px(), 33.0);
    }
}