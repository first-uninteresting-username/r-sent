{
  description = "r-sent: a simple plaintext presentation tool for Wayland";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        r-sent = pkgs.callPackage ./package.nix { };
        default = r-sent;
      });

      overlays.default = final: _prev: {
        r-sent = final.callPackage ./package.nix { };
      };

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
