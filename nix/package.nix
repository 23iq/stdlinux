{ lib
, rustPlatform
, makeWrapper
, cabextract
, unzip
, gnutar
, xz
, xdg-utils
, shared-mime-info
, desktop-file-utils
, libnotify
  # Directory with bin/wine that the launcher should use.
, wine
  # Command the Wine binaries are run through, or null when `wine` runs natively.
, wineWrap ? null
}:

rustPlatform.buildRustPackage {
  pname = "stdlinux";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [ ../Cargo.toml ../Cargo.lock ../src ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];

  # GameMode is deliberately not in PATH here: it needs its daemon, which the NixOS
  # module (or the system) provides.
  postInstall = ''
    wrapProgram $out/bin/stdlinux \
      --set-default STDLINUX_WINE ${wine} \
      --set-default STDLINUX_WINE_WRAP "${lib.optionalString (wineWrap != null) wineWrap}" \
      --prefix PATH : ${lib.makeBinPath [ cabextract unzip gnutar xz xdg-utils shared-mime-info desktop-file-utils libnotify ]}
  '';

  meta = {
    description = "Low-latency osu! stable on Linux";
    license = lib.licenses.gpl3Plus;
    platforms = [ "x86_64-linux" ];
    mainProgram = "stdlinux";
  };
}
