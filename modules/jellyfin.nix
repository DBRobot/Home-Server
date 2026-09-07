{ config, pkgs, ... }:
let
  base = "distributed-datacenter.duckdns.org";
  host = "jellyfin.${base}";
in
{
  services.jellyfin = {
    enable = true;
    group = "media"; # reads the union in modules/media-tier.nix
  };

  # Tiger Lake iris xe does several 4k transcodes at once, but only with the
  # VA-API driver present. Without it jellyfin silently falls back to software
  # and pegs all 8 cores on a single stream.
  hardware.graphics = {
    enable = true;
    extraPackages = with pkgs; [
      intel-media-driver
      vpl-gpu-rt
    ];
  };

  users.users.jellyfin.extraGroups = [
    "render" # /dev/dri/renderD128
    "video"
  ];

  # The union is a fuse mount; jellyfin marks a library "missing" and drops its
  # metadata if it scans while the mount is absent.
  systemd.services.jellyfin = {
    after = [ "media-union.service" ];
    requires = [ "media-union.service" ];
  };

  services.nginx.virtualHosts.${host} = {
    useACMEHost = base;
    forceSSL = true;
    locations."/" = {
      proxyPass = "http://127.0.0.1:8096";
      proxyWebsockets = true;
      extraConfig = ''
        client_max_body_size 0;
        proxy_buffering off; # streams, not pages
      '';
    };
  };
}
