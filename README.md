# r-sent

`r-sent` (pronounced "recent") is a cleanroom soft rewrite of [`sent`](https://tools.suckless.org/sent/) in Rust.

## Differences with original

Advantages:

- Wayland support
- Runtime configuration
- Friendlier configuration language

Disadvantages:

- Bloat
- No support for X11

Other things:

- Different license
- Being written by AI
- Rust instead of C

## Installation

Requirements:

- x86_64 or aarch64 Linux with glibc 2.35 or newer (Debian 12+, Ubuntu 22.04+) for the prebuilt binaries
- Wayland compositor
- `libxkbcommon` (the package is `libxkbcommon0` on Debian/Ubuntu and `libxkbcommon` on Fedora/Arch)
- Optional: `bzip2`, used by the default image filter to decode `.ff.bz2` (compressed farbfeld) images

### Nix

Run without installing:

```bash
nix run github:first-uninteresting-username/r-sent -- presentation.txt
```

To install, add the input to your `flake.nix`:

```nix
inputs.r-sent = {
  url = "github:first-uninteresting-username/r-sent";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Then use the package (or `inputs.r-sent.overlays.default`, which provides `pkgs.r-sent`):

```nix
inputs.r-sent.packages.${pkgs.stdenv.hostPlatform.system}.default

# Override the embedded configuration
(inputs.r-sent.packages.${pkgs.stdenv.hostPlatform.system}.default.override {
  # Merged onto config.def.toml; arrays replace the default as a whole
  settings.colors = {
    foreground = "#FFFFFF";
    background = "#000000";
  };
  # Or embed a complete config.toml instead (takes precedence over settings)
  # configFile = ./config.toml;
  # Packages put on the PATH for image filter commands (default: [ pkgs.bzip2 ])
  # filterPackages = [ pkgs.bzip2 ];
})
```

The Nix package installs the man page and wraps `r-sent` so that `bzip2` is on its `PATH`.

### Standalone

#### Prebuilt binaries

```bash
VERSION=0.1.0 # Replace with the newest version
ARCH=$(uname -m)
NAME=r-sent-$VERSION-$ARCH-linux
REPO=https://github.com/first-uninteresting-username/r-sent/releases/download/v$VERSION

curl -LO "$REPO/$NAME.tar.gz"
tar -xzf "$NAME.tar.gz"

install -Dm755 "$NAME/r-sent"   ~/.local/bin/r-sent
# Optional, install the man page
install -Dm644 "$NAME/r-sent.1" ~/.local/share/man/man1/r-sent.1
```

Each release also publishes a `SHA256SUMS` file for verifying the archives.

#### Manual compilation

Build requirements:

- Rust 1.88+
- C toolchain, `pkg-config` and the `libxkbcommon` headers
  (`sudo apt install build-essential pkg-config libxkbcommon-dev` on Debian/Ubuntu)

##### Cargo

Cargo does not install the man page.

```bash
cargo install --git https://github.com/first-uninteresting-username/r-sent
```

##### From source

```bash
git clone https://github.com/first-uninteresting-username/r-sent
cd r-sent

# Optional, customise the embedded configuration
cp config.def.toml config.toml
$EDITOR config.toml

cargo build --release --locked
cargo test --locked # Optional

install -Dm755 target/release/r-sent ~/.local/bin/r-sent
# Optional, install the man page
install -Dm644 r-sent.1 ~/.local/share/man/man1/r-sent.1
```

## Configuration

The default configuration is [config.def.toml](config.def.toml).
At build time it is copied to `config.toml` (unless that file already exists), validated and embedded
in the binary, so an invalid configuration fails the build.
A build-time `config.toml` must be complete: it has to contain every key from `config.def.toml`.

At run time, the embedded configuration is overridden by, in order:

1. `$XDG_CONFIG_HOME/r-sent/config.toml` (or `~/.config/r-sent/config.toml`)
2. The `config` command-line argument

Override files only need the keys you want to change.
Tables are merged key by key, while the arrays `fonts.fallbacks`, `keys`, `mouse` and `filters` replace
the previous array entirely. Unknown keys are errors.

## Usage

```bash
r-sent [-v] [--] [file] [config]
```

- `-v` prints the version and exits
- `--` ends option parsing, so the next argument is the file even if it starts with `-`
- `file` is the presentation; if omitted or `-`, it is read from standard input
- `config` is an extra configuration file that overrides the others

### Presentation format

Every paragraph is one slide; text is drawn at the largest size that fits.

```text
# Lines starting with '#' are comments
r-sent

plain text slides
scaled to fit

@diagram.png

\@not-an-image.png is a text line
```

- A slide whose first line starts with `@` is an image slide; image paths are relative to the presentation
- One leading `\` is removed from every line, to escape `#` and `@`
- Images are decoded natively (PNG, JPEG, GIF, BMP, WebP, farbfeld) unless they match an image filter

See [example/presentation.txt](example/presentation.txt) and `man r-sent` for the full format and key bindings.

Default keys: `Right`/`Space`/`Return`/`l`/`j`/`n` next slide, `Left`/`BackSpace`/`h`/`k`/`p` previous slide,
`r` reload, `f` toggle fullscreen, `q`/`Escape` quit.

## License

[GPL-3.0](LICENSE)

## Credits

- Suckless for the original [sent](https://tools.suckless.org/sent/)
