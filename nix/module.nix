# NixOS module: the package plus the system settings a user cannot set themselves.
self:
{ config, lib, pkgs, ... }:

let
  cfg = config.programs.stdlinux;
in
{
  options.programs.stdlinux = {
    enable = lib.mkEnableOption "stdlinux, low-latency osu! stable";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.stdlinux;
      description = "The stdlinux package to install.";
    };

    realtimeGroup = lib.mkOption {
      type = lib.types.str;
      default = "audio";
      description = "Group whose members may use realtime priority and negative nice values.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    programs.gamemode.enable = lib.mkDefault true;

    # Lets Wine's audio thread and wineserver raise their own priority.
    security.pam.loginLimits = [
      { domain = "@${cfg.realtimeGroup}"; type = "-"; item = "rtprio"; value = "98"; }
      { domain = "@${cfg.realtimeGroup}"; type = "-"; item = "nice"; value = "-20"; }
      { domain = "@${cfg.realtimeGroup}"; type = "-"; item = "memlock"; value = "unlimited"; }
    ];

    # Lets the launcher keep CPU cores out of deep sleep while the game runs. Shipped as a
    # package so the rule sorts before 73-seat-late.rules, which applies the uaccess tag.
    services.udev.packages = [
      (pkgs.writeTextFile {
        name = "stdlinux-udev-rules";
        destination = "/lib/udev/rules.d/70-stdlinux.rules";
        text = ''
          KERNEL=="cpu_dma_latency", TAG+="uaccess"
        '';
      })
    ];

    # osu! takes split locks; without this the kernel throttles it each time.
    boot.kernel.sysctl."kernel.split_lock_mitigate" = lib.mkDefault 0;

    # ntsync: the kernel-side implementation of Windows synchronisation primitives.
    boot.kernelModules = [ "ntsync" ];
  };
}
