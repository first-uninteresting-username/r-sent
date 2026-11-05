//! Software framebuffer and drawing of text and image slides (§7.3, §8.3).

use crate::config::{Config, Rgb};
use crate::font::{Fonts, GlyphPixels};
use crate::layout::{self, Metrics};
use crate::slide::{Slide, SlideKind};

/// A `width × height` framebuffer of `0x00RRGGBB` pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framebuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

fn blend_channel(fg: u32, bg: u32, a: u32) -> u32 {
    (fg * a + bg * (255 - a) + 127) / 255
}

fn blend_px(dst: u32, src: u32, a: u32) -> u32 {
    let ch = |shift: u32| blend_channel((src >> shift) & 0xff, (dst >> shift) & 0xff, a) << shift;
    ch(16) | ch(8) | ch(0)
}

impl Framebuffer {
    pub fn new(width: u32, height: u32) -> Framebuffer {
        Framebuffer {
            width,
            height,
            pixels: vec![0; width as usize * height as usize],
        }
    }

    /// Resizes the framebuffer (contents become undefined).
    pub fn resize(&mut self, width: u32, height: u32) {
        if (width, height) != (self.width, self.height) {
            self.width = width;
            self.height = height;
            self.pixels.clear();
            self.pixels.resize(width as usize * height as usize, 0);
        }
    }

    pub fn fill(&mut self, c: Rgb) {
        self.pixels.fill(c.xrgb());
    }

    #[cfg(test)]
    pub fn get(&self, x: u32, y: u32) -> u32 {
        self.pixels[y as usize * self.width as usize + x as usize]
    }

    /// Calls `f(fb_index, src_x, src_y)` for every pixel of a `w × h` rectangle at `(x, y)`
    /// that lies inside the framebuffer.
    fn for_each_clipped(
        &mut self,
        x: i64,
        y: i64,
        w: u32,
        h: u32,
        mut f: impl FnMut(&mut u32, usize, usize),
    ) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + i64::from(w)).min(i64::from(self.width));
        let y1 = (y + i64::from(h)).min(i64::from(self.height));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let stride = self.width as usize;
        for fy in y0..y1 {
            let row = fy as usize * stride;
            let sy = (fy - y) as usize;
            for fx in x0..x1 {
                f(&mut self.pixels[row + fx as usize], (fx - x) as usize, sy);
            }
        }
    }

    /// Blends an 8-bit coverage mask in colour `fg`.
    pub fn blend_mask(&mut self, x: i64, y: i64, w: u32, h: u32, mask: &[u8], fg: Rgb) {
        let fg = fg.xrgb();
        let stride = w as usize;
        self.for_each_clipped(x, y, w, h, |px, sx, sy| {
            let a = u32::from(mask[sy * stride + sx]);
            if a == 255 {
                *px = fg;
            } else if a > 0 {
                *px = blend_px(*px, fg, a);
            }
        });
    }

    /// Blends a straight-alpha RGBA image.
    pub fn blend_rgba(&mut self, x: i64, y: i64, w: u32, h: u32, rgba: &[u8]) {
        let stride = w as usize;
        self.for_each_clipped(x, y, w, h, |px, sx, sy| {
            let i = (sy * stride + sx) * 4;
            let p = &rgba[i..i + 4];
            let src = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
            *px = blend_px(*px, src, u32::from(p[3]));
        });
    }

    /// Copies an opaque RGB8 image.
    pub fn blit_rgb(&mut self, x: i64, y: i64, w: u32, h: u32, rgb: &[u8]) {
        let stride = w as usize;
        self.for_each_clipped(x, y, w, h, |px, sx, sy| {
            let i = (sy * stride + sx) * 3;
            *px = (u32::from(rgb[i]) << 16) | (u32::from(rgb[i + 1]) << 8) | u32::from(rgb[i + 2]);
        });
    }

    /// Writes the framebuffer as little-endian 32-bit pixels with alpha 0xFF (valid for both
    /// XRGB8888 and ARGB8888).
    pub fn write_to(&self, canvas: &mut [u8]) {
        for (dst, &px) in canvas.as_chunks_mut::<4>().0.iter_mut().zip(&self.pixels) {
            dst.copy_from_slice(&(px | 0xff00_0000).to_le_bytes());
        }
    }
}

/// Usable area: `floor(usable_* × size)`.
pub fn usable_area(cfg: &Config, w: u32, h: u32) -> (u32, u32) {
    (
        (cfg.layout.usable_width * f64::from(w)).floor() as u32,
        (cfg.layout.usable_height * f64::from(h)).floor() as u32,
    )
}

/// Draws `slide` into `fb`.
pub fn draw_slide(fb: &mut Framebuffer, slide: &mut Slide, fonts: &mut Fonts, cfg: &Config) {
    fb.fill(cfg.colors.background);
    let (uw, uh) = usable_area(cfg, fb.width, fb.height);
    match &mut slide.kind {
        SlideKind::Image {
            image: Some(img), ..
        } => draw_image(fb, img, uw, uh),
        // Images are loaded before drawing; an unloaded image slide stays blank.
        SlideKind::Image { image: None, .. } => {}
        SlideKind::Text => draw_text(fb, &slide.lines, fonts, cfg, uw, uh),
    }
}

/// Draws a (cached) scaled image centred in the framebuffer.
pub fn draw_image(fb: &mut Framebuffer, img: &mut crate::image::LoadedImage, uw: u32, uh: u32) {
    let s = img.scaled_for(uw, uh);
    let x = (i64::from(fb.width) - i64::from(s.width)) / 2;
    let y = (i64::from(fb.height) - i64::from(s.height)) / 2;
    fb.blit_rgb(x, y, s.width, s.height, &s.rgb);
}

fn draw_text(
    fb: &mut Framebuffer,
    lines: &[String],
    fonts: &mut Fonts,
    cfg: &Config,
    uw: u32,
    uh: u32,
) {
    let spacing = cfg.layout.line_spacing;
    let fit = layout::fit(fonts, lines, spacing, uw, uh);
    let placed = layout::place(fonts, lines, &fit, spacing, fb.width, fb.height);
    let fh = fonts.height(fit.scale);
    let fg = cfg.colors.foreground;

    for line in placed {
        let mut pen = line.x as f32;
        for c in line.text.chars() {
            let face = fonts.face_for(c);
            let fm = fonts.face_metrics(face, fit.scale);
            let baseline =
                line.y as f32 + ((i64::from(fh) - i64::from(fm.height)) / 2) as f32 + fm.ascent;
            let baseline = baseline.round() as i64;
            let gid = fonts.glyph_id(face, c);
            if let Some(g) = fonts.glyph(face, gid, fit.scale) {
                let gx = pen.round() as i64 + i64::from(g.left);
                let gy = baseline - i64::from(g.top);
                match &g.pixels {
                    GlyphPixels::Mask(m) => fb.blend_mask(gx, gy, g.width, g.height, m, fg),
                    GlyphPixels::Color(rgba) => fb.blend_rgba(gx, gy, g.width, g.height, rgba),
                }
            }
            pen += fonts.advance(c, fit.scale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
    const WHITE: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };

    #[test]
    fn fill_and_write() {
        let mut fb = Framebuffer::new(2, 1);
        fb.fill(Rgb { r: 1, g: 2, b: 3 });
        let mut out = [0u8; 8];
        fb.write_to(&mut out);
        assert_eq!(out, [3, 2, 1, 0xff, 3, 2, 1, 0xff]);
    }

    #[test]
    fn mask_blending_and_clipping() {
        let mut fb = Framebuffer::new(3, 3);
        fb.fill(WHITE);
        // 2x2 mask partly outside the top-left corner.
        fb.blend_mask(-1, -1, 2, 2, &[255, 255, 255, 128], BLACK);
        assert_eq!(fb.get(0, 0), 0x7f7f7f);
        assert_eq!(fb.get(1, 0), 0xffffff);
        // Fully outside: no panic, no change.
        fb.blend_mask(5, 5, 2, 2, &[255; 4], BLACK);
        fb.blend_mask(-5, 0, 2, 2, &[255; 4], BLACK);
        assert!(fb.pixels[1..].iter().all(|&p| p == 0xffffff));
    }

    #[test]
    fn rgb_blit_centred() {
        let mut fb = Framebuffer::new(4, 4);
        fb.fill(BLACK);
        let mut img = crate::image::blend(1, 1, &[65535, 0, 0, 65535], WHITE).unwrap();
        draw_image(&mut fb, &mut img, 2, 2);
        // 1x1 → 2x2, centred at (1, 1).
        assert_eq!(fb.get(0, 0), 0);
        assert_eq!(fb.get(1, 1), 0xff0000);
        assert_eq!(fb.get(2, 2), 0xff0000);
        assert_eq!(fb.get(3, 3), 0);
    }

    #[test]
    fn rgba_blending() {
        let mut fb = Framebuffer::new(1, 1);
        fb.fill(BLACK);
        fb.blend_rgba(0, 0, 1, 1, &[255, 0, 0, 255]);
        assert_eq!(fb.get(0, 0), 0xff0000);
        fb.blend_rgba(0, 0, 1, 1, &[0, 0, 255, 0]);
        assert_eq!(fb.get(0, 0), 0xff0000);
    }
}
