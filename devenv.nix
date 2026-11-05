{
  pkgs,
  lib,
  config,
  inputs,
  ...
}:
{
  languages.rust.enable = true;

  packages = [
    pkgs.pkg-config
    pkgs.libxkbcommon
    pkgs.wayland
    pkgs.bzip2
  ];

  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
  };
}
