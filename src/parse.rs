//! Presentation parser. Pure: the only I/O is reading from a `BufRead`.

use std::io::{self, BufRead};
use std::path::PathBuf;

use crate::slide::{Slide, SlideKind};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("presentation contains no slides")]
    NoSlides,
}

/// Parses a presentation (§3 of the specification).
pub fn parse<R: BufRead>(mut reader: R) -> Result<Vec<Slide>, ParseError> {
    let mut slides = Vec::new();
    let mut current: Option<Slide> = None;
    let mut buf = Vec::new();

    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        let line = String::from_utf8_lossy(&buf);

        if line.is_empty() {
            slides.extend(current.take());
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let slide = current.get_or_insert_with(|| Slide::text(Vec::new()));
        if slide.lines.is_empty()
            && line.len() > 1
            && let Some(path) = line.strip_prefix('@')
        {
            slide.kind = SlideKind::Image {
                path: PathBuf::from(path),
                image: None,
            };
        }
        let text = line.strip_prefix('\\').unwrap_or(&line);
        slide.lines.push(text.to_owned());
    }
    slides.extend(current);

    if slides.is_empty() {
        return Err(ParseError::NoSlides);
    }
    Ok(slides)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simplified view of a slide for comparisons: `Err(path)` for image slides.
    fn view(input: &[u8]) -> Vec<Result<Vec<String>, String>> {
        parse(input)
            .unwrap()
            .into_iter()
            .map(|s| match &s.kind {
                SlideKind::Text => Ok(s.lines),
                SlideKind::Image { path, .. } => Err(path.to_string_lossy().into_owned()),
            })
            .collect()
    }

    fn text(lines: &[&str]) -> Result<Vec<String>, String> {
        Ok(lines.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn separators() {
        assert_eq!(view(b"a\nb\n\nc\n"), [text(&["a", "b"]), text(&["c"])]);
        assert_eq!(
            view(b"\n\n\na\n\n\n\n\nb\n\n\n"),
            [text(&["a"]), text(&["b"])]
        );
    }

    #[test]
    fn comments() {
        // Inside a paragraph a comment does not end the slide.
        assert_eq!(view(b"a\n# c\nb\n"), [text(&["a", "b"])]);
        // Between paragraphs.
        assert_eq!(view(b"a\n\n# c\n\nb\n"), [text(&["a"]), text(&["b"])]);
        // A comment-only paragraph yields no slide.
        assert_eq!(view(b"# only\n\na\n"), [text(&["a"])]);
        // A comment before an image line keeps it the first line.
        assert_eq!(view(b"# c\n@x.png\n"), [Err("x.png".into())]);
    }

    #[test]
    fn escapes() {
        assert_eq!(view(b"\\@foo.png\n"), [text(&["@foo.png"])]);
        assert_eq!(view(b"\\#x\n"), [text(&["#x"])]);
        assert_eq!(view(b"\\\n"), [text(&[""])]);
        assert_eq!(view(b"\\\\x\n"), [text(&["\\x"])]);
        assert_eq!(view(b"a\\b\n"), [text(&["a\\b"])]);
    }

    #[test]
    fn image_detection() {
        assert_eq!(view(b"@a b.png\nnot drawn\n"), [Err("a b.png".into())]);
        let slides = parse(&b"@ x.png \nmore\n"[..]).unwrap();
        assert_eq!(slides[0].lines, ["@ x.png ", "more"]);
        assert_eq!(slides[0].image_path().unwrap().to_str(), Some(" x.png "));
        // Only on the first line.
        assert_eq!(view(b"text\n@x.png\n"), [text(&["text", "@x.png"])]);
        // A lone '@' is text.
        assert_eq!(view(b"@\n"), [text(&["@"])]);
    }

    #[test]
    fn line_endings_and_encoding() {
        assert_eq!(view(b"a\r\nb\r\n\r\nc"), [text(&["a", "b"]), text(&["c"])]);
        // Only one trailing \r is removed.
        assert_eq!(view(b"a\r\r\n"), [text(&["a\r"])]);
        assert_eq!(view(b"a\xffb\n"), [text(&["a\u{fffd}b"])]);
        assert_eq!(view(b"no newline"), [text(&["no newline"])]);
    }

    #[test]
    fn whitespace_line_is_content() {
        assert_eq!(view(b"a\n \nb\n"), [text(&["a", " ", "b"])]);
        assert_eq!(view(b"\t\n"), [text(&["\t"])]);
    }

    #[test]
    fn no_slides() {
        for input in [&b""[..], b"\n\n", b"# a\n# b\n", b"\n# a\n\n# b\n"] {
            assert!(matches!(parse(input), Err(ParseError::NoSlides)));
        }
    }

    #[test]
    fn spec_example() {
        let input = b"r-sent\n\n@diagram.png\n\nwhy plain text\n- diffable\n- no tooling\n\
            # comment, not shown\n\\# shown, starts with a hash\n\\@shown.png, not an image\n\n\\\n";
        assert_eq!(
            view(input),
            [
                text(&["r-sent"]),
                Err("diagram.png".into()),
                text(&[
                    "why plain text",
                    "- diffable",
                    "- no tooling",
                    "# shown, starts with a hash",
                    "@shown.png, not an image"
                ]),
                text(&[""]),
            ]
        );
    }

    #[test]
    fn example_presentation() {
        let input = include_bytes!("../example/presentation.txt");
        let long = "this is a very long line that does not fit even at the smallest font size \
            so it gets truncated with an ellipsis when the window is narrow enough, \
            which is exactly what we want to demonstrate here";
        assert_eq!(
            view(input),
            [
                text(&["r-sent"]),
                text(&["plain text slides", "scaled to fit"]),
                text(&[
                    "why plain text",
                    "- diffable",
                    "- no tooling",
                    "- no markup"
                ]),
                Err("diagram.png".into()),
                Err("transparent.png".into()),
                Err("gradient.ff.bz2".into()),
                text(&[
                    "# escaped: this line starts with a hash",
                    "@escaped.png is text, not an image",
                    "\\ a line starting with one backslash",
                ]),
                text(&[""]),
                text(&["@", "a lone '@' is text"]),
                text(&["wide Unicode → ½ ∑ λ — 日本語 — emoji 🦀🎉"]),
                text(&[long]),
                text(&[
                    "whitespace-only lines are content:",
                    "   ",
                    "(the line above has spaces)",
                ]),
                text(&["thanks!"]),
            ]
        );
        // The image slide keeps its extra (never drawn) line.
        let slides = parse(&input[..]).unwrap();
        assert_eq!(slides[3].lines.len(), 2);
    }
}
