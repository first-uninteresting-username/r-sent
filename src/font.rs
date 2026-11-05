//! Font discovery, fallback chain, metrics and glyph rasterization (§7.1).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fontdb::{Database, Family, ID, Query, Stretch, Style, Weight};
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;
use swash::{CacheKey, FontRef, GlyphId};

use crate::config;
use crate::error::Error;
use crate::layout::Metrics;

/// A rasterized glyph.
#[derive(Debug)]
pub struct Glyph {
    /// Offset of the bitmap's left edge from the pen position.
    pub left: i32,
    /// Offset of the bitmap's top edge above the baseline.
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub pixels: GlyphPixels,
}

#[derive(Debug)]
pub enum GlyphPixels {
    /// 8-bit coverage, `width * height` bytes.
    Mask(Vec<u8>),
    /// Straight-alpha RGBA, `width * height * 4` bytes.
    Color(Vec<u8>),
}

struct Face {
    data: Arc<Vec<u8>>,
    offset: u32,
    key: CacheKey,
}

impl Face {
    fn load(db: &Database, id: ID) -> Option<Face> {
        db.with_face_data(id, |data, index| {
            let font = FontRef::from_index(data, index as usize)?;
            let offset = font.offset;
            Some(Face {
                data: Arc::new(data.to_vec()),
                offset,
                key: CacheKey::new(),
            })
        })
        .flatten()
    }

    fn font(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        }
    }
}

/// Vertical metrics of a face at a scale, in pixels.
#[derive(Debug, Clone, Copy)]
pub struct FaceMetrics {
    pub ascent: f32,
    /// `ceil(ascent + descent)`.
    pub height: u32,
}

/// The fontset with caches for one surface scale factor.
pub struct Fonts {
    db: Database,
    faces: Vec<Face>,
    in_set: HashSet<ID>,
    /// Point sizes of all scales.
    points: Vec<f64>,
    /// Surface scale factor `s`.
    surface_scale: f64,
    face_for: HashMap<char, usize>,
    metrics: HashMap<(usize, usize), FaceMetrics>,
    advances: HashMap<(char, usize), f32>,
    glyphs: HashMap<(usize, GlyphId, usize), Option<Glyph>>,
    ctx: ScaleContext,
}

impl Fonts {
    /// Loads system fonts and resolves the configured fallback families.
    pub fn new(cfg: &config::Fonts) -> Result<Fonts, Error> {
        let mut db = Database::new();
        db.load_system_fonts();

        let mut faces = Vec::new();
        let mut in_set = HashSet::new();
        for wanted in &cfg.fallbacks {
            let Some(family) = find_family(&db, wanted) else {
                continue;
            };
            let query = Query {
                families: &[Family::Name(&family)],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
            };
            if let Some(id) = db.query(&query) {
                if in_set.contains(&id) {
                    continue;
                }
                if let Some(face) = Face::load(&db, id) {
                    in_set.insert(id);
                    faces.push(face);
                }
            }
        }
        if faces.is_empty() {
            return Err(Error::NoFont);
        }

        Ok(Fonts {
            db,
            faces,
            in_set,
            points: (0..cfg.scales).map(|i| cfg.points(i)).collect(),
            surface_scale: 1.0,
            face_for: HashMap::new(),
            metrics: HashMap::new(),
            advances: HashMap::new(),
            glyphs: HashMap::new(),
            ctx: ScaleContext::new(),
        })
    }

    /// Sets the surface scale factor, clearing size-dependent caches if it changed.
    pub fn set_surface_scale(&mut self, s: f64) {
        if s != self.surface_scale {
            self.surface_scale = s;
            self.metrics.clear();
            self.advances.clear();
            self.glyphs.clear();
        }
    }

    /// Pixel size of scale `i`: points × 96 / 72 × s.
    pub fn pixel_size(&self, scale: usize) -> f32 {
        let pt = self.points.get(scale).copied().unwrap_or(0.0);
        (pt * 96.0 / 72.0 * self.surface_scale) as f32
    }

    /// Index of the face used for `c` (§7.1 glyph selection). Falls back to face 0, whose
    /// `.notdef` glyph is then drawn.
    pub fn face_for(&mut self, c: char) -> usize {
        if let Some(&i) = self.face_for.get(&c) {
            return i;
        }
        let i = self
            .faces
            .iter()
            .position(|f| f.font().charmap().map(c) != 0)
            .or_else(|| self.search_system(c))
            .unwrap_or(0);
        self.face_for.insert(c, i);
        i
    }

    /// Searches all system faces for one covering `c` and appends it to the fontset.
    fn search_system(&mut self, c: char) -> Option<usize> {
        let mut candidates: Vec<&fontdb::FaceInfo> = self
            .db
            .faces()
            .filter(|f| !self.in_set.contains(&f.id))
            .collect();
        // Prefer regular, upright faces.
        candidates.sort_by_key(|f| (f.style != Style::Normal, f.weight.0.abs_diff(400)));
        let id = candidates.into_iter().map(|f| f.id).find(|&id| {
            self.db
                .with_face_data(id, |data, index| {
                    FontRef::from_index(data, index as usize)
                        .is_some_and(|f| f.charmap().map(c) != 0)
                })
                .unwrap_or(false)
        })?;
        let face = Face::load(&self.db, id)?;
        self.in_set.insert(id);
        self.faces.push(face);
        Some(self.faces.len() - 1)
    }

    pub fn glyph_id(&self, face: usize, c: char) -> GlyphId {
        self.faces[face].font().charmap().map(c)
    }

    pub fn face_metrics(&mut self, face: usize, scale: usize) -> FaceMetrics {
        if let Some(m) = self.metrics.get(&(face, scale)) {
            return *m;
        }
        let px = self.pixel_size(scale);
        let m = self.faces[face].font().metrics(&[]).scale(px);
        let ascent = m.ascent;
        let descent = m.descent.abs();
        let fm = FaceMetrics {
            ascent,
            height: (ascent + descent).ceil().max(1.0) as u32,
        };
        self.metrics.insert((face, scale), fm);
        fm
    }

    /// Rasterizes (and caches) glyph `gid` of `face` at `scale`.
    pub fn glyph(&mut self, face: usize, gid: GlyphId, scale: usize) -> Option<&Glyph> {
        let key = (face, gid, scale);
        if !self.glyphs.contains_key(&key) {
            let px = self.pixel_size(scale);
            let font = self.faces[face].font();
            let mut scaler = self.ctx.builder(font).size(px).hint(false).build();
            let image = Render::new(&[
                Source::ColorOutline(0),
                Source::ColorBitmap(StrikeWith::BestFit),
                Source::Outline,
            ])
            .format(Format::Alpha)
            .render(&mut scaler, gid);
            let glyph = image.and_then(|img| {
                let p = img.placement;
                let n = p.width as usize * p.height as usize;
                let pixels = match img.content {
                    Content::Mask if img.data.len() >= n => GlyphPixels::Mask(img.data),
                    Content::Color if img.data.len() >= n * 4 => GlyphPixels::Color(img.data),
                    Content::SubpixelMask if img.data.len() >= n * 4 => GlyphPixels::Mask(
                        img.data.as_chunks::<4>().0.iter().map(|p| p[1]).collect(),
                    ),
                    _ => return None,
                };
                Some(Glyph {
                    left: p.left,
                    top: p.top,
                    width: p.width,
                    height: p.height,
                    pixels,
                })
            });
            self.glyphs.insert(key, glyph);
        }
        self.glyphs.get(&key).and_then(Option::as_ref)
    }
}

impl Metrics for Fonts {
    fn scales(&self) -> usize {
        self.points.len()
    }

    /// `fh(i)`: height of the first face.
    fn height(&mut self, scale: usize) -> u32 {
        self.face_metrics(0, scale).height
    }

    fn advance(&mut self, c: char, scale: usize) -> f32 {
        if let Some(a) = self.advances.get(&(c, scale)) {
            return *a;
        }
        let face = self.face_for(c);
        let gid = self.glyph_id(face, c);
        let px = self.pixel_size(scale);
        let a = self.faces[face]
            .font()
            .glyph_metrics(&[])
            .scale(px)
            .advance_width(gid);
        self.advances.insert((c, scale), a);
        a
    }
}

/// Finds the installed family whose name matches `wanted` case-insensitively.
fn find_family(db: &Database, wanted: &str) -> Option<String> {
    db.faces()
        .flat_map(|f| f.families.iter())
        .find(|(name, _)| {
            name.eq_ignore_ascii_case(wanted) || name.to_lowercase() == wanted.to_lowercase()
        })
        .map(|(name, _)| name.clone())
}
