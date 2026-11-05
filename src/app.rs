//! Application state (slides, index, source) and actions (§9).

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::{Action, Config};
use crate::error::Error;
use crate::parse::{self, ParseError};
use crate::slide::{Slide, SlideKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    Stdin,
}

impl Source {
    /// Display name used in error messages.
    pub fn name(&self) -> String {
        match self {
            Source::File(p) => p.to_string_lossy().into_owned(),
            Source::Stdin => "<stdin>".into(),
        }
    }

    /// Directory relative image paths are resolved against.
    pub fn base_dir(&self) -> PathBuf {
        match self {
            Source::File(p) => match p.parent() {
                Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
                _ => PathBuf::from("."),
            },
            Source::Stdin => PathBuf::from("."),
        }
    }

    /// Reads and parses the presentation, resolving image paths. Images are not loaded.
    pub fn read_slides(&self) -> Result<Vec<Slide>, Error> {
        let read_err = |e| Error::Read(self.name(), e);
        let data = match self {
            Source::File(p) => std::fs::read(p).map_err(read_err)?,
            Source::Stdin => {
                let mut buf = Vec::new();
                std::io::stdin().read_to_end(&mut buf).map_err(read_err)?;
                buf
            }
        };
        let mut slides = parse::parse(&data[..]).map_err(|e| match e {
            ParseError::NoSlides => Error::NoSlides,
            ParseError::Io(e) => read_err(e),
        })?;
        resolve_paths(&mut slides, &self.base_dir());
        Ok(slides)
    }
}

/// Makes relative image paths relative to `base`.
pub fn resolve_paths(slides: &mut [Slide], base: &Path) {
    for slide in slides {
        if let SlideKind::Image { path, .. } = &mut slide.kind
            && path.is_relative()
            && base != Path::new(".")
        {
            *path = base.join(&*path);
        }
    }
}

/// Loads the images of all image slides (§8.1).
pub fn load_images(slides: &mut [Slide], config: &Config) -> Result<(), Error> {
    for slide in slides {
        if let SlideKind::Image { path, image } = &mut slide.kind {
            *image = Some(crate::image::load(
                path,
                &config.filters,
                config.colors.background,
            )?);
        }
    }
    Ok(())
}

/// What the window layer has to do after an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    Redraw,
    ToggleFullscreen,
}

pub struct App {
    pub config: Config,
    pub source: Source,
    /// Never empty.
    pub slides: Vec<Slide>,
    pub idx: usize,
    pub running: bool,
    /// Fullscreen state as reported by the last configure.
    pub fullscreen: bool,
}

impl App {
    pub fn new(config: Config, source: Source, slides: Vec<Slide>) -> App {
        debug_assert!(!slides.is_empty());
        let fullscreen = config.fullscreen;
        App {
            config,
            source,
            slides,
            idx: 0,
            running: true,
            fullscreen,
        }
    }

    pub fn current(&mut self) -> &mut Slide {
        &mut self.slides[self.idx]
    }

    /// Runs one action.
    pub fn perform(&mut self, action: Action) -> Effect {
        match action {
            Action::Quit => {
                self.running = false;
                Effect::None
            }
            Action::Advance(n) => {
                if self.advance(n) {
                    Effect::Redraw
                } else {
                    Effect::None
                }
            }
            Action::Reload => match self.reload() {
                Ok(()) => Effect::Redraw,
                Err(e) => {
                    crate::error::report(&e);
                    Effect::None
                }
            },
            Action::ToggleFullscreen => Effect::ToggleFullscreen,
        }
    }

    /// Moves by `n` slides, clamped. Returns whether the slide changed.
    pub fn advance(&mut self, n: i32) -> bool {
        let last = self.slides.len() as i64 - 1;
        let new = (self.idx as i64 + i64::from(n)).clamp(0, last) as usize;
        if new == self.idx {
            return false;
        }
        self.slides[self.idx].drop_scaled();
        self.idx = new;
        true
    }

    /// Re-reads the presentation and all images. On error the old presentation is kept.
    pub fn reload(&mut self) -> Result<(), Error> {
        if self.source == Source::Stdin {
            return Err(Error::ReloadStdin);
        }
        let mut slides = self.source.read_slides()?;
        load_images(&mut slides, &self.config)?;
        self.replace_slides(slides);
        Ok(())
    }

    fn replace_slides(&mut self, slides: Vec<Slide>) {
        debug_assert!(!slides.is_empty());
        self.slides = slides;
        self.idx = self.idx.min(self.slides.len() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn config() -> Config {
        crate::config::parse_str(include_str!("../config.def.toml")).unwrap()
    }

    fn slides(n: usize) -> Vec<Slide> {
        (0..n).map(|i| Slide::text(vec![i.to_string()])).collect()
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("r-sent-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn write(p: &Path, s: &str) {
        std::fs::File::create(p)
            .unwrap()
            .write_all(s.as_bytes())
            .unwrap();
    }

    #[test]
    fn advance_clamps() {
        let mut app = App::new(config(), Source::Stdin, slides(3));
        assert!(!app.advance(-1));
        assert_eq!(app.idx, 0);
        assert!(app.advance(1));
        assert_eq!(app.idx, 1);
        assert!(app.advance(10));
        assert_eq!(app.idx, 2);
        assert!(!app.advance(1));
        assert!(!app.advance(0));
        assert!(app.advance(i32::MIN));
        assert_eq!(app.idx, 0);
        assert_eq!(app.perform(Action::Advance(-1)), Effect::None);
        assert_eq!(app.perform(Action::Advance(2)), Effect::Redraw);
        assert_eq!(app.perform(Action::Advance(i32::MAX)), Effect::None);
    }

    #[test]
    fn advance_drops_scaled_cache() {
        let mut s = slides(1);
        let mut img = crate::image::blend(
            1,
            1,
            &[0, 0, 0, 65535],
            crate::config::Rgb { r: 0, g: 0, b: 0 },
        )
        .unwrap();
        img.scaled_for(2, 2);
        s.insert(
            0,
            Slide {
                lines: vec!["@x".into()],
                kind: SlideKind::Image {
                    path: "x".into(),
                    image: Some(img),
                },
            },
        );
        let mut app = App::new(config(), Source::Stdin, s);
        assert!(app.advance(1));
        match &app.slides[0].kind {
            SlideKind::Image { image: Some(i), .. } => assert!(i.scaled.is_none()),
            _ => unreachable!(),
        }
    }

    #[test]
    fn quit_and_fullscreen() {
        let mut app = App::new(config(), Source::Stdin, slides(1));
        assert_eq!(
            app.perform(Action::ToggleFullscreen),
            Effect::ToggleFullscreen
        );
        assert!(app.running);
        app.perform(Action::Quit);
        assert!(!app.running);
    }

    #[test]
    fn reload_from_stdin_rejected() {
        let mut app = App::new(config(), Source::Stdin, slides(2));
        assert!(matches!(app.reload(), Err(Error::ReloadStdin)));
        assert_eq!(
            Error::ReloadStdin.to_string(),
            "reload is not available for stdin input"
        );
        assert_eq!(app.perform(Action::Reload), Effect::None);
        assert_eq!(app.slides.len(), 2);
    }

    #[test]
    fn reload_failure_keeps_slides() {
        let p = temp_path("keep.txt");
        write(&p, "a\n\nb\n\nc\n");
        let mut app = App::new(
            config(),
            Source::File(p.clone()),
            Source::File(p.clone()).read_slides().unwrap(),
        );
        app.advance(1);

        // No slides.
        write(&p, "# nothing\n");
        assert!(matches!(app.reload(), Err(Error::NoSlides)));
        assert_eq!(app.slides.len(), 3);
        assert_eq!(app.idx, 1);

        // Missing image.
        write(&p, "@does-not-exist.png\n");
        assert!(matches!(app.reload(), Err(Error::ImageOpen { .. })));
        assert_eq!(app.slides.len(), 3);

        // Unreadable file.
        std::fs::remove_file(&p).unwrap();
        let e = app.reload().unwrap_err();
        assert!(matches!(e, Error::Read(..)));
        assert!(
            e.to_string()
                .starts_with(&format!("cannot read '{}': ", p.display()))
        );
        assert_eq!(app.slides.len(), 3);
        assert_eq!(app.perform(Action::Reload), Effect::None);
    }

    #[test]
    fn reload_clamps_index() {
        let p = temp_path("clamp.txt");
        write(&p, "a\n\nb\n\nc\n");
        let mut app = App::new(
            config(),
            Source::File(p.clone()),
            Source::File(p.clone()).read_slides().unwrap(),
        );
        app.advance(2);
        write(&p, "x\n\ny\n");
        assert_eq!(app.perform(Action::Reload), Effect::Redraw);
        assert_eq!(app.slides.len(), 2);
        assert_eq!(app.idx, 1);
        assert_eq!(app.slides[1].lines, ["y"]);
        write(&p, "x\n\ny\n\nz\n\nw\n");
        app.reload().unwrap();
        assert_eq!(app.idx, 1);
    }

    #[test]
    fn image_paths_resolved_against_file_dir() {
        let mut s = parse::parse(&b"@a.png\n\n@/abs.png\n"[..]).unwrap();
        resolve_paths(&mut s, Path::new("/pres"));
        assert_eq!(s[0].image_path().unwrap(), Path::new("/pres/a.png"));
        assert_eq!(s[1].image_path().unwrap(), Path::new("/abs.png"));
        assert_eq!(Source::File("x.txt".into()).base_dir(), Path::new("."));
        assert_eq!(Source::File("d/x.txt".into()).base_dir(), Path::new("d"));
        assert_eq!(Source::Stdin.base_dir(), Path::new("."));
    }
}
