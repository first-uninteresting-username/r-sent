//! Image loading, filters, alpha pre-blending and scaling (§8).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};

use image::{ImageFormat, ImageReader};

use crate::config::{Filter, Rgb};
use crate::error::Error;

/// A decoded image, already blended with the background colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedImage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 3` bytes, row-major RGB8.
    pub rgb: Vec<u8>,
    /// Scaled copy for the current window size (§8.4).
    pub scaled: Option<ScaledImage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaledImage {
    /// The usable area (`uw × uh`) this copy was made for.
    pub for_area: (u32, u32),
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Returns the first filter whose regex matches `path`.
pub fn select_filter<'a>(filters: &'a [Filter], path: &str) -> Option<&'a Filter> {
    filters.iter().find(|f| f.regex.is_match(path))
}

/// Loads and decodes the image at `path` (§8.1).
pub fn load(path: &Path, filters: &[Filter], background: Rgb) -> Result<LoadedImage, Error> {
    let name = path.to_string_lossy().into_owned();
    let file = File::open(path).map_err(|err| Error::ImageOpen {
        path: name.clone(),
        err,
    })?;
    let decode_err = |reason: String| Error::ImageDecode {
        path: name.clone(),
        reason,
    };

    let img = match select_filter(filters, &name) {
        Some(filter) => {
            let data = run_filter(&filter.command, file).map_err(|reason| Error::Filter {
                command: filter.command.clone(),
                path: name.clone(),
                reason,
            })?;
            image::load_from_memory(&data).map_err(|e| decode_err(e.to_string()))?
        }
        None => {
            let mut reader = ImageReader::new(BufReader::new(file));
            if let Ok(format) = ImageFormat::from_path(path) {
                reader.set_format(format);
            }
            let reader = reader
                .with_guessed_format()
                .map_err(|e| decode_err(e.to_string()))?;
            reader.decode().map_err(|e| decode_err(e.to_string()))?
        }
    };

    let rgba = img.into_rgba16();
    let (w, h) = rgba.dimensions();
    blend(w, h, rgba.as_raw(), background).ok_or(Error::ImageDimensions { path: name })
}

/// Runs `sh -c command` with `input` as stdin and returns its stdout.
pub fn run_filter(command: &str, input: File) -> Result<Vec<u8>, String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::from(input))
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot spawn: {e}"))?;
    let mut out = Vec::new();
    let read = match child.stdout.take() {
        Some(mut stdout) => stdout.read_to_end(&mut out).map(drop),
        None => Ok(()),
    };
    let status = child.wait().map_err(|e| e.to_string())?;
    read.map_err(|e| format!("cannot read output: {e}"))?;
    if !status.success() {
        return Err(status.to_string());
    }
    Ok(out)
}

/// Blends an RGBA16 image with `bg` (§8.2). Returns `None` for invalid dimensions.
pub fn blend(width: u32, height: u32, rgba16: &[u16], bg: Rgb) -> Option<LoadedImage> {
    if width == 0 || height == 0 {
        return None;
    }
    let pixels = (width as usize).checked_mul(height as usize)?;
    let len = pixels.checked_mul(3)?;
    if rgba16.len() < pixels.checked_mul(4)? {
        return None;
    }
    let mut rgb = Vec::with_capacity(len);
    for px in rgba16.as_chunks::<4>().0.iter().take(pixels) {
        let a = u32::from(px[3] / 257);
        for (c, b) in [(px[0], bg.r), (px[1], bg.g), (px[2], bg.b)] {
            let c = u32::from(c / 257);
            rgb.push(((c * a + u32::from(b) * (255 - a)) / 255) as u8);
        }
    }
    Some(LoadedImage {
        width,
        height,
        rgb,
        scaled: None,
    })
}

/// Target size of a `bw × bh` image fitted into `uw × uh` (§8.3).
pub fn target_size(bw: u32, bh: u32, uw: u32, uh: u32) -> (u32, u32) {
    let (bw, bh, uw, uh) = (u64::from(bw), u64::from(bh), u64::from(uw), u64::from(uh));
    let (tw, th) = if uw * bh > uh * bw {
        (bw * uh / bh, uh)
    } else {
        (uw, bh * uw / bw)
    };
    (tw as u32, th as u32)
}

/// Nearest-neighbour scaling of an RGB8 image.
pub fn scale_nearest(src: &[u8], bw: u32, bh: u32, tw: u32, th: u32) -> Vec<u8> {
    let (bw64, bh64) = (u64::from(bw), u64::from(bh));
    let mut out = Vec::with_capacity(tw as usize * th as usize * 3);
    let cols: Vec<usize> = (0..u64::from(tw))
        .map(|x| (x * bw64 / u64::from(tw)) as usize * 3)
        .collect();
    for y in 0..u64::from(th) {
        let sy = (y * bh64 / u64::from(th)) as usize;
        let row = &src[sy * bw as usize * 3..(sy + 1) * bw as usize * 3];
        for &sx in &cols {
            out.extend_from_slice(&row[sx..sx + 3]);
        }
    }
    out
}

impl LoadedImage {
    /// Returns the image scaled for the usable area `uw × uh`, using the cache if valid.
    pub fn scaled_for(&mut self, uw: u32, uh: u32) -> &ScaledImage {
        let valid = self.scaled.as_ref().is_some_and(|s| s.for_area == (uw, uh));
        if !valid {
            let (tw, th) = target_size(self.width, self.height, uw.max(1), uh.max(1));
            // Very thin images could round to zero; keep at least one pixel.
            let (tw, th) = (tw.max(1), th.max(1));
            self.scaled = Some(ScaledImage {
                for_area: (uw, uh),
                width: tw,
                height: th,
                rgb: scale_nearest(&self.rgb, self.width, self.height, tw, th),
            });
        }
        match &self.scaled {
            Some(s) => s,
            None => unreachable!("scaled image was just set"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Filter;
    use regex::RegexBuilder;
    use std::io::Write;

    const WHITE: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };

    fn filter(re: &str, cmd: &str) -> Filter {
        Filter {
            regex: RegexBuilder::new(re)
                .case_insensitive(true)
                .build()
                .unwrap(),
            command: cmd.into(),
        }
    }

    fn temp_file(name: &str, data: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("r-sent-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::File::create(&p).unwrap().write_all(data).unwrap();
        p
    }

    #[test]
    fn alpha_blending() {
        let bg = Rgb {
            r: 10,
            g: 20,
            b: 30,
        };
        let data = [
            // opaque red
            65535, 0, 0, 65535, //
            // fully transparent green
            0, 65535, 0, 0, //
            // half-transparent white: a8 = 32896 / 257 = 128
            65535, 65535, 65535, 32896,
        ];
        let img = blend(3, 1, &data, bg).unwrap();
        let half = |b: u32| ((255 * 128 + b * 127) / 255) as u8;
        assert_eq!(
            img.rgb,
            [255, 0, 0, 10, 20, 30, half(10), half(20), half(30)]
        );
        // c16 / 257 truncates.
        let img = blend(1, 1, &[256, 257, 514, 65535], WHITE).unwrap();
        assert_eq!(img.rgb, [0, 1, 2]);
    }

    #[test]
    fn zero_size_rejected() {
        assert!(blend(0, 5, &[], WHITE).is_none());
        assert!(blend(5, 0, &[], WHITE).is_none());
        assert!(blend(u32::MAX, u32::MAX, &[], WHITE).is_none());
    }

    #[test]
    fn target_sizes() {
        // Wide image: limited by width.
        assert_eq!(target_size(400, 100, 300, 300), (300, 75));
        // Tall image: limited by height.
        assert_eq!(target_size(100, 400, 300, 300), (75, 300));
        // Same aspect ratio as the area.
        assert_eq!(target_size(16, 9, 1600, 900), (1600, 900));
        // Upscaling.
        assert_eq!(target_size(2, 1, 1000, 1000), (1000, 500));
        // Large values don't overflow.
        assert_eq!(target_size(u32::MAX, 1, u32::MAX, 1), (u32::MAX, 1));
    }

    #[test]
    fn nearest_neighbour() {
        // 2x2 image with distinct pixels, scaled to 4x4 and 1x1.
        let src = [1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4];
        let up = scale_nearest(&src, 2, 2, 4, 4);
        let ids: Vec<u8> = up.chunks(3).map(|p| p[0]).collect();
        assert_eq!(ids, [1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 4, 4]);
        assert_eq!(scale_nearest(&src, 2, 2, 1, 1), [1, 1, 1]);
        // 3x1 → 2x1: src_x = 0, 1.
        let src = [1, 1, 1, 2, 2, 2, 3, 3, 3];
        assert_eq!(scale_nearest(&src, 3, 1, 2, 1), [1, 1, 1, 2, 2, 2]);
    }

    #[test]
    fn scaled_cache() {
        let mut img = blend(2, 1, &[0, 0, 0, 65535, 65535, 65535, 65535, 65535], WHITE).unwrap();
        assert_eq!(img.scaled_for(4, 4).width, 4);
        assert_eq!(img.scaled_for(4, 4).height, 2);
        assert_eq!(img.scaled_for(8, 8).width, 8);
        assert_eq!(img.scaled.as_ref().unwrap().for_area, (8, 8));
    }

    #[test]
    fn filter_selection() {
        let filters = [
            filter(r"\.ff\.bz2$", "bunzip2"),
            filter(r"\.bz2$", "other"),
            filter(r"\.png$", "cat"),
        ];
        assert_eq!(
            select_filter(&filters, "a.ff.bz2").unwrap().command,
            "bunzip2"
        );
        assert_eq!(
            select_filter(&filters, "A.FF.BZ2").unwrap().command,
            "bunzip2"
        );
        assert_eq!(select_filter(&filters, "a.bz2").unwrap().command, "other");
        assert_eq!(select_filter(&filters, "dir/X.Png").unwrap().command, "cat");
        assert!(select_filter(&filters, "a.jpg").is_none());
    }

    fn png_bytes() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn native_and_filtered_loading() {
        let p = temp_file("red.png", &png_bytes());
        let img = load(&p, &[], WHITE).unwrap();
        assert_eq!((img.width, img.height), (3, 2));
        assert_eq!(&img.rgb[..3], [255, 0, 0]);

        // Through a filter (the format is guessed from content).
        let p2 = temp_file("red.through-cat", &png_bytes());
        let img = load(&p2, &[filter(r"\.through-cat$", "cat")], WHITE).unwrap();
        assert_eq!((img.width, img.height), (3, 2));

        // Wrong extension: content wins.
        let p3 = temp_file("red.jpg", &png_bytes());
        assert!(load(&p3, &[], WHITE).is_ok());
    }

    #[test]
    fn filter_failure_reported() {
        let p = temp_file("fail.img", b"data");
        let e = load(&p, &[filter(r"\.img$", "exit 3")], WHITE).unwrap_err();
        let msg = e.to_string();
        assert!(matches!(e, Error::Filter { .. }), "{msg}");
        assert!(msg.starts_with("filter 'exit 3' failed for '"), "{msg}");
        assert!(msg.contains("exit status: 3"), "{msg}");
    }

    #[test]
    fn open_and_decode_errors() {
        let e = load(Path::new("/nonexistent/x.png"), &[], WHITE).unwrap_err();
        assert!(matches!(e, Error::ImageOpen { .. }));
        assert!(
            e.to_string()
                .starts_with("cannot open image '/nonexistent/x.png': ")
        );

        let p = temp_file("garbage.png", b"not an image");
        let e = load(&p, &[], WHITE).unwrap_err();
        assert!(matches!(e, Error::ImageDecode { .. }), "{e}");
    }
}
