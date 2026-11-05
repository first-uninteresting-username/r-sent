{
  lib,
  rustPlatform,
  pkg-config,
  libxkbcommon,
  bzip2,
  makeWrapper,
  installShellFiles,
}:
let
  manifest = (lib.importTOML ./Cargo.toml).package;
in
rustPlatform.buildRustPackage {
  pname = manifest.name;
  inherit (manifest) version;

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./build.rs
      ./config.def.toml
      ./src
      ./example
      ./r-sent.1
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    makeWrapper
    installShellFiles
  ];
  buildInputs = [ libxkbcommon ];

  postInstall = ''
    installManPage r-sent.1
    # The default config's image filter uses bunzip2.
    wrapProgram $out/bin/r-sent --suffix PATH : ${lib.makeBinPath [ bzip2 ]}
  '';

  meta = {
    description = manifest.description;
    license = lib.licenses.gpl3Only;
    mainProgram = "r-sent";
    platforms = lib.platforms.linux;
  };
}
