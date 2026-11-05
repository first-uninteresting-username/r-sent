{
  lib,
  rustPlatform,
  pkg-config,
  libxkbcommon,
  bzip2,
  makeWrapper,
  installShellFiles,
  formats,

  # Overrides for the embedded default configuration, merged recursively onto
  # config.def.toml. Arrays (keys, mouse, filters, fonts.fallbacks) replace the default
  # as a whole, like runtime overrides do. Example:
  #   r-sent.override { settings.colors = { foreground = "#FFFFFF"; background = "#000000"; }; }
  settings ? { },
  # A complete config.toml to embed instead; takes precedence over `settings`.
  configFile ? null,
  # Packages put on the PATH of r-sent, for the commands used by image filters.
  filterPackages ? [ bzip2 ],
}:
let
  manifest = (lib.importTOML ./Cargo.toml).package;

  embeddedConfig =
    if configFile != null then
      configFile
    else
      (formats.toml { }).generate "r-sent-config.toml" (
        lib.recursiveUpdate (lib.importTOML ./config.def.toml) settings
      );
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

  # build.rs validates config.toml, so an invalid configuration fails the build.
  postPatch = ''
    cp ${embeddedConfig} config.toml
  '';

  nativeBuildInputs = [
    pkg-config
    makeWrapper
    installShellFiles
  ];
  buildInputs = [ libxkbcommon ];

  postInstall = ''
    installManPage r-sent.1
  ''
  + lib.optionalString (filterPackages != [ ]) ''
    wrapProgram $out/bin/r-sent --suffix PATH : ${lib.makeBinPath filterPackages}
  '';

  meta = {
    description = manifest.description;
    license = lib.licenses.gpl3Only;
    mainProgram = "r-sent";
    platforms = lib.platforms.linux;
  };
}
