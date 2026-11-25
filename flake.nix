{
  description = "stdlinux: low-latency osu! stable on Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Source build of Wine with the osu! patch set; ours go on top (see ./patches).
    nix-gaming.url = "github:fufexan/nix-gaming";
  };

  outputs = { self, nixpkgs, nix-gaming }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      lib = pkgs.lib;

      ownPatches = map (name: ./patches + "/${name}")
        (builtins.filter (lib.hasSuffix ".patch") (builtins.attrNames (builtins.readDir ./patches)));
    in
    {
      packages.${system} = rec {
        # Prebuilt Wine, run inside an FHS environment. Nothing to compile.
        wine-osu-bin = pkgs.callPackage ./nix/wine-osu-bin.nix { };
        wine-env = pkgs.callPackage ./nix/wine-env.nix { };
        stdlinux = pkgs.callPackage ./nix/package.nix {
          wine = wine-osu-bin;
          wineWrap = "${wine-env}/bin/stdlinux-wine-env";
        };

        # Wine built from source with ./patches applied. Runs natively (no sandbox), so
        # GameMode can see the game. Compiling it takes a while.
        wine-osu =
          let base = nix-gaming.packages.${system}.wine-osu; in
          if ownPatches == [ ] then base
          else base.overrideDerivation (old: { patches = (old.patches or [ ]) ++ ownPatches; });
        stdlinux-native = pkgs.callPackage ./nix/package.nix { wine = wine-osu; };

        # What the release workflow uploads.
        stdlinux-static = pkgs.pkgsStatic.callPackage ./nix/static.nix { };

        default = stdlinux;
      };

      apps.${system}.default = {
        type = "app";
        program = "${self.packages.${system}.stdlinux}/bin/stdlinux";
      };

      nixosModules.default = import ./nix/module.nix self;

      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [ cargo rustc clippy rustfmt ];
      };
    };
}
