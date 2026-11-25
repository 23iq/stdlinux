# The pinned wine-osu build, unmodified. It is a generic-Linux binary, so it is only
# ever executed inside ./wine-env.nix. Keep url/hash in sync with src/wine.rs.
{ stdenvNoCC, fetchurl }:

stdenvNoCC.mkDerivation {
  pname = "wine-osu-bin";
  version = "11.22-1";

  src = fetchurl {
    url = "https://github.com/NelloKudo/WineBuilder/releases/download/wine-osu-staging-11.22-1/wine-osu-winello-fonts-wow64-11.22-1-x86_64.tar.xz";
    sha256 = "045c4de0863b5abb7b5a7bfef8e735dcd6dd8af0e3a2ed221f640206d18f288f";
  };

  dontConfigure = true;
  dontBuild = true;
  # Windows DLLs and prebuilt ELF files: leave every byte alone.
  dontFixup = true;

  installPhase = ''
    mkdir -p $out
    cp -a . $out/
  '';
}
