# An FHS environment with the libraries the prebuilt Wine loads at run time.
# Used as a command prefix: `stdlinux-wine-env /path/to/wine ...`.
{ buildFHSEnv, writeShellScript }:

buildFHSEnv {
  name = "stdlinux-wine-env";

  targetPkgs = pkgs: with pkgs; [
    # graphics
    libGL
    vulkan-loader
    libdrm
    libva
    wayland
    libxkbcommon
    xkeyboard_config
    libx11
    libxext
    libxi
    libxcursor
    libxrandr
    libxrender
    libxfixes
    libxcomposite
    libxinerama
    libxxf86vm
    libxtst
    libxv
    libxcb
    # audio
    pipewire
    libpulseaudio
    alsa-lib
    alsa-plugins
    # fonts and text
    freetype
    fontconfig
    cairo
    pango
    # system
    glib
    gnutls
    dbus
    udev
    libgudev
    libusb1
    libunwind
    zlib
    bzip2
    ncurses
    krb5
    cups
    SDL2
    # media
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
    libpng
    libjpeg
    libopus
  ];

  runScript = writeShellScript "stdlinux-wine-env-run" ''exec "$@"'';
}
