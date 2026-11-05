//! Error type and fatal-error formatting.

use std::fmt;
use std::io;

/// Every error r-sent can report. `Display` yields the message without the `r-sent: ` prefix.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read '{0}': {1}")]
    Read(String, #[source] io::Error),
    #[error("presentation contains no slides")]
    NoSlides,
    #[error("cannot connect to Wayland display: {0}")]
    Connect(String),
    #[error("compositor lacks required protocol '{0}'")]
    MissingGlobal(&'static str),
    #[error("Wayland connection error: {0}")]
    Wayland(String),
    #[error("none of the configured fonts are installed")]
    NoFont,
    #[error("cannot open image '{path}': {err}")]
    ImageOpen { path: String, err: io::Error },
    #[error("filter '{command}' failed for '{path}': {reason}")]
    Filter {
        command: String,
        path: String,
        reason: String,
    },
    #[error("cannot decode image '{path}': {reason}")]
    ImageDecode { path: String, reason: String },
    #[error("image '{path}' has invalid dimensions")]
    ImageDimensions { path: String },
    #[error("{path}: {msg}")]
    Config { path: String, msg: String },
    #[error("reload is not available for stdin input")]
    ReloadStdin,
}

/// Formats an error as the single line printed to stderr.
pub struct Fatal<'a>(pub &'a Error);

impl fmt::Display for Fatal<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r-sent: {}", self.0)
    }
}

/// Prints `r-sent: <error>` to stderr.
pub fn report(e: &Error) {
    eprintln!("{}", Fatal(e));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        assert_eq!(
            Fatal(&Error::NoSlides).to_string(),
            "r-sent: presentation contains no slides"
        );
        assert_eq!(
            Error::MissingGlobal("wl_shm").to_string(),
            "compositor lacks required protocol 'wl_shm'"
        );
        let e = Error::Read(
            "a.txt".into(),
            io::Error::new(io::ErrorKind::NotFound, "gone"),
        );
        assert_eq!(e.to_string(), "cannot read 'a.txt': gone");
    }
}
