//! Slide types.

use std::path::PathBuf;

use crate::image::LoadedImage;

#[derive(Debug)]
pub struct Slide {
    /// All content lines (escapes already removed). For image slides the first line is the
    /// `@path` line; further lines are kept but never drawn.
    pub lines: Vec<String>,
    pub kind: SlideKind,
}

#[derive(Debug)]
pub enum SlideKind {
    Text,
    Image {
        path: PathBuf,
        image: Option<LoadedImage>,
    },
}

impl Slide {
    pub fn text(lines: Vec<String>) -> Slide {
        Slide {
            lines,
            kind: SlideKind::Text,
        }
    }

    /// Image path of an image slide.
    #[cfg(test)]
    pub fn image_path(&self) -> Option<&PathBuf> {
        match &self.kind {
            SlideKind::Image { path, .. } => Some(path),
            SlideKind::Text => None,
        }
    }

    /// Drops the scaled-image cache, if any.
    pub fn drop_scaled(&mut self) {
        if let SlideKind::Image {
            image: Some(img), ..
        } = &mut self.kind
        {
            img.scaled = None;
        }
    }
}
