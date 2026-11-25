# Wine patches

Every `*.patch` file in this directory is applied, in name order, on top of Wine Staging
plus [wine-osu-patches](https://github.com/whrvt/wine-osu-patches) when the
`wine-osu` flake package is built (`nix build .#wine-osu`).

Rules for adding one:

- It fixes something that was measured (`stdlinux doctor`, a frame-time log), and the
  commit message says what the measurement was before and after.
- One change per file, named `NNNN-short-description.patch`, made with `git format-patch`
  against the patched tree.

There are none yet: the patch set above already covers audio latency, raw input, timer
resolution and Wayland tearing, and nothing measured so far points at Wine itself.
