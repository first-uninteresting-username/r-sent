//! Font-size fitting and text placement (§7.2, §7.3). Pure: works on any [`Metrics`].

/// Font metrics as needed by layout. Scales are indices into the configured size list.
pub trait Metrics {
    /// Number of available scales (≥ 1).
    fn scales(&self) -> usize;
    /// Font height `fh(i)` in pixels.
    fn height(&mut self, scale: usize) -> u32;
    /// Horizontal advance of `c` at `scale`, in pixels.
    fn advance(&mut self, c: char, scale: usize) -> f32;

    /// Width of `text`: the sum of its glyph advances, rounded up.
    fn width(&mut self, text: &str, scale: usize) -> u32 {
        let w: f32 = text.chars().map(|c| self.advance(c, scale)).sum();
        w.ceil().max(0.0) as u32
    }
}

/// Result of fitting a slide's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    pub scale: usize,
    pub line_height: u32,
    pub block_width: u32,
    pub block_height: u32,
}

/// Chooses the font scale for `lines` (§7.2).
///
/// `block_width` is the width of the widest line, but never more than `uw`; a line wider than
/// the block (only possible at scale 0) is truncated by [`place`].
pub fn fit<M: Metrics>(m: &mut M, lines: &[String], line_spacing: f64, uw: u32, uh: u32) -> Fit {
    let n = lines.len().max(1);
    let lfac = line_spacing * (n - 1) as f64 + 1.0;

    let mut j = (0..m.scales())
        .rev()
        .find(|&i| f64::from(m.height(i)) * lfac <= f64::from(uh))
        .unwrap_or(0);

    for line in lines {
        while j > 0 && m.width(line, j) > uw {
            j -= 1;
        }
    }

    let widest = lines.iter().map(|l| m.width(l, j)).max().unwrap_or(0);
    let line_height = m.height(j);
    Fit {
        scale: j,
        line_height,
        block_width: widest.min(uw),
        block_height: (f64::from(line_height) * lfac).floor() as u32,
    }
}

/// A line of text positioned on the framebuffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedLine {
    /// Text to draw (possibly truncated with `...`).
    pub text: String,
    /// Top-left corner of the line's box (`block_width × line_height`).
    pub x: i64,
    pub y: i64,
}

/// Places `lines` centred as a block in a `w × h` framebuffer (§7.3).
pub fn place<M: Metrics>(
    m: &mut M,
    lines: &[String],
    fit: &Fit,
    line_spacing: f64,
    w: u32,
    h: u32,
) -> Vec<PlacedLine> {
    let x = (i64::from(w) - i64::from(fit.block_width)) / 2;
    let top = (i64::from(h) - i64::from(fit.block_height)) / 2;
    lines
        .iter()
        .enumerate()
        .map(|(k, line)| PlacedLine {
            text: truncate(m, line, fit.scale, fit.block_width),
            x,
            y: top + (k as f64 * line_spacing * f64::from(fit.line_height)).floor() as i64,
        })
        .collect()
}

/// Returns `line` if it fits in `max_width`; otherwise drops trailing characters until the
/// rest plus `...` fits, and appends `...`.
pub fn truncate<M: Metrics>(m: &mut M, line: &str, scale: usize, max_width: u32) -> String {
    if m.width(line, scale) <= max_width {
        return line.to_owned();
    }
    let ellipsis: f32 = m.advance('.', scale) * 3.0;
    let mut pen = 0.0f32;
    let mut end = 0;
    for (i, c) in line.char_indices() {
        let next = pen + m.advance(c, scale);
        if (next + ellipsis).ceil() > max_width as f32 {
            break;
        }
        pen = next;
        end = i + c.len_utf8();
    }
    format!("{}...", &line[..end])
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Fake metrics: `fh(i) = i + 10`, every character is `i + 1` pixels wide.
    pub struct Fake(pub usize);

    impl Metrics for Fake {
        fn scales(&self) -> usize {
            self.0
        }
        fn height(&mut self, i: usize) -> u32 {
            i as u32 + 10
        }
        fn advance(&mut self, _c: char, i: usize) -> f32 {
            (i + 1) as f32
        }
    }

    fn lines(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn height_limited() {
        // One short line, uh = 30: largest i with i + 10 <= 30 is 20.
        let f = fit(&mut Fake(42), &lines(&["ab"]), 1.4, 1000, 30);
        assert_eq!(f.scale, 20);
        assert_eq!(f.line_height, 30);
        assert_eq!(f.block_width, 2 * 21);
        assert_eq!(f.block_height, 30);

        // Two lines: lfac = 2.4, largest i with (i + 10) * 2.4 <= 60 is 15.
        let f = fit(&mut Fake(42), &lines(&["a", "b"]), 1.4, 1000, 60);
        assert_eq!(f.scale, 15);
        assert_eq!(f.block_height, (25.0f64 * 2.4).floor() as u32);

        // Capped by the number of scales.
        let f = fit(&mut Fake(5), &lines(&["a"]), 1.4, 1000, 1000);
        assert_eq!(f.scale, 4);

        // Nothing fits: scale 0.
        let f = fit(&mut Fake(42), &lines(&["a"]), 1.4, 1000, 5);
        assert_eq!(f.scale, 0);
    }

    #[test]
    fn width_limited() {
        // 10 chars, uw = 100: width 10 * (i + 1) <= 100 → i = 9.
        let f = fit(
            &mut Fake(42),
            &lines(&["short", "0123456789"]),
            1.0,
            100,
            1000,
        );
        assert_eq!(f.scale, 9);
        assert_eq!(f.block_width, 100);
        assert_eq!(f.line_height, 19);
    }

    #[test]
    fn huge_line_truncated() {
        let long = "x".repeat(200);
        let ls = lines(&[&long]);
        let mut m = Fake(42);
        let f = fit(&mut m, &ls, 1.4, 50, 1000);
        assert_eq!(f.scale, 0);
        assert_eq!(f.block_width, 50);
        let placed = place(&mut m, &ls, &f, 1.4, 100, 100);
        assert_eq!(placed[0].text, format!("{}...", "x".repeat(47)));
        assert_eq!(m.width(&placed[0].text, 0), 50);
        assert_eq!(placed[0].x, 25);
    }

    #[test]
    fn truncate_edge_cases() {
        let mut m = Fake(1);
        assert_eq!(truncate(&mut m, "abc", 0, 3), "abc");
        assert_eq!(truncate(&mut m, "abcd", 0, 3), "...");
        assert_eq!(truncate(&mut m, "abcd", 0, 1), "...");
        assert_eq!(truncate(&mut m, "äöüßx", 0, 4), "ä...");
    }

    #[test]
    fn multi_line_block_positions() {
        let ls = lines(&["a", "abcd", "ab"]);
        let mut m = Fake(42);
        // uw = 40 → 4 * (i + 1) <= 40 → i = 9; uh = 1000.
        let f = fit(&mut m, &ls, 1.5, 40, 1000);
        assert_eq!(f.scale, 9);
        assert_eq!(f.line_height, 19);
        assert_eq!(f.block_width, 40);
        // lfac = 1.5 * 2 + 1 = 4 → block height 76.
        assert_eq!(f.block_height, 76);

        let placed = place(&mut m, &ls, &f, 1.5, 200, 100);
        // All lines share x: left-aligned in a centred block.
        assert!(placed.iter().all(|p| p.x == (200 - 40) / 2));
        let top = (100 - 76) / 2;
        assert_eq!(placed[0].y, top);
        assert_eq!(placed[1].y, top + (1.5f64 * 19.0).floor() as i64);
        assert_eq!(placed[2].y, top + (3.0f64 * 19.0).floor() as i64);
        assert_eq!(placed[1].text, "abcd");
    }

    #[test]
    fn negative_offsets_when_too_big() {
        let ls = lines(&["abc"]);
        let mut m = Fake(1);
        let f = fit(&mut m, &ls, 1.0, 100, 1);
        let placed = place(&mut m, &ls, &f, 1.0, 1, 1);
        assert_eq!(placed[0].x, (1 - 3) / 2);
        assert_eq!(placed[0].y, (1 - 10) / 2);
    }
}
