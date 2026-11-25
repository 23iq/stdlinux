# The release binary: statically linked, no wrapper, runs on any x86_64 Linux.
{ lib, rustPlatform }:

rustPlatform.buildRustPackage {
  pname = "stdlinux-static";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [ ../Cargo.toml ../Cargo.lock ../src ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  meta.mainProgram = "stdlinux";
}
