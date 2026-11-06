# r-sent

A cleanroom soft rewrite of `sent` in Rust.

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

- x86_64 or aarch64 Linux with glibc 2.35 or newer (Debian 12+, Ubuntu 22.04+)
- Wayland compositor
- `libxkbcommon` (the package is `libxkbcommon0` on Debian/Ubuntu and `libxkbcommon` on Fedora/Arch)
- Optional: `bzip2` for images and some font

### Nix

Just running:

```bash
nix run github:first-uninteresting-username/r-sent -- presentation.txt
```

Installation:

Add the input to your `flake.nix`:

```nix
input.r-sent = {
  url =  "github:first-uninteresting-username/r-sent";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Use the package under the following attribute:

```nix
inputs.r-sent.packages.${pkgs.system}.default

# Overrides for configuration
(inputs.r-sent.packages.${pkgs.system}.default.override {
  # you can add settings.* block here, which maps to the toml configuration
  # configFile sets the configuration file
})
```

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
# Optional, install man pages
install -Dm644 "$NAME/r-sent.1" ~/.local/share/man/man1/r-sent.1
```

#### Manual compilation

Build requirements:

- Rust 1.88
- C toolchain (`gcc` or `clang` and `pkg-config` and `libxkbcommon` headers (`sudo apt install build-essential pkg-config libxkbcommon-dev` on apt based systems))

##### Cargo

Note that cargo won't install the man pages.

```bash
cargo install --git https://github.com/first-uninteresting-username/r-sent
```

##### Make

```bash
git clone https://github.com/first-uninteresting-username/r-sent
cd r-sent

# Optional, edit the default config
$EDITOR config.toml
cargo build --release --locked
cargo test --locked # optional

# installation steps are the same as for prebuilt binary
install -Dm755 target/release/r-sent ~/.local/bin/r-sent
# Optional, install man pages
install -Dm644 r-sent.1 ~/.local/share/man/man1/r-sent.1
```

## Credits

- Suckless for the original [sent](https://tools.suckless.org/sent/)
