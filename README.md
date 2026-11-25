# stdlinux

Low-latency osu! stable on Linux. One command installs a patched Wine, the prefix,
the game and the importers for beatmaps, skins and replays.

Status: early. Tested on NixOS (niri, NVIDIA, PipeWire). Other distributions are
supported by design but not tested yet.

## Install

Any distribution, one command (downloads a static binary, verifies its checksum, runs the installer):

```
curl -fsSL https://raw.githubusercontent.com/23iq/stdlinux/main/install.sh | sh
```

NixOS / Nix with flakes:

```
nix run github:23iq/stdlinux -- install
```

or as a NixOS module, which also grants realtime priority, lets the launcher block deep
CPU sleep while playing, and enables GameMode:

```nix
{
  inputs.stdlinux.url = "github:23iq/stdlinux";
  # in your configuration:
  imports = [ inputs.stdlinux.nixosModules.default ];
  programs.stdlinux.enable = true;
}
```

From source: `cargo install --git https://github.com/23iq/stdlinux`.

`stdlinux install` finds an existing osu! setup (osu-winello, `~/.wine-osu`) and offers to
reuse it: the prefix is copied, the game folder is used in place, nothing is modified.
A fresh prefix needs `cabextract` and `unzip` on the host (the flake brings them).

On distributions other than NixOS, run `stdlinux tune` once afterwards: it writes four
small files under `/etc` (realtime limits, CPU sleep control, split-lock sysctl, ntsync)
with one sudo prompt, and `stdlinux tune --undo` removes them.

## Use

| Command | What it does |
|---|---|
| `stdlinux` | install if needed, otherwise start the game |
| `stdlinux run` | start osu! |
| `stdlinux import <files or osu:// links>` | import `.osz`, `.osk`, `.osr`; also what file managers and browsers call |
| `stdlinux preset [name]` | list presets or switch |
| `stdlinux doctor` | check the system for latency and stutter sources |
| `stdlinux doctor --mouse` | measure the real mouse report rate |
| `stdlinux tune` | apply the system settings that need root |
| `stdlinux show` | print the resolved settings and environment |
| `stdlinux kill` | stop the game and its Wine server |
| `stdlinux uninstall` | remove Wine, prefix and integration; the game folder is kept |

## Presets

| Preset | Audio | Notes |
|---|---|---|
| `latency` (default) | PipeWire driver, 64 frames | P-cores only, NVIDIA low-latency, XWayland |
| `ultra` | 32 frames | plus realtime wineserver; needs rtprio limits |
| `wayland` | 64 frames | native Wayland driver |
| `balanced` | 128 frames | no core pinning |
| `compat` | PulseAudio defaults | no tuning, for troubleshooting |

Single settings can be overridden in `~/.config/stdlinux/config.toml`:

```toml
preset = "latency"

[overrides]
quantum = 48
display = "wayland"

[env]
WINEDEBUG = "-all"
```

## What it is built on

Wine is the [wine-osu](https://github.com/NelloKudo/WineBuilder) build (Wine Staging with
[wine-osu-patches](https://github.com/whrvt/wine-osu-patches)), downloaded with a pinned
checksum. stdlinux is not affiliated with osu! or ppy Pty Ltd; the game itself is
downloaded from the official servers.

### Building Wine yourself

The flake can also build Wine from source with the same patch set plus everything in
[`patches/`](patches/README.md), and run it natively instead of inside an FHS environment:

```
nix build .#stdlinux-native     # compiles Wine; takes a while
```

Use it from the module with
`programs.stdlinux.package = inputs.stdlinux.packages.x86_64-linux.stdlinux-native;`.

## License

GPL-3.0-or-later.
