{ config, pkgs, ... }:
let
  base = "distributed-datacenter.duckdns.org";
  host = "jellyfin.${base}";

  # Not in nixpkgs, and jellyfin has no plugin option, so it is fetched and
  # dropped into the plugin dir. The upstream repo is ARCHIVED - it targets
  # abi 10.11.0.0 which matches jellyfin 10.11.11 today, but a future
  # jellyfin bump may strand it and there is no maintained replacement.
  ssoPlugin = pkgs.stdenvNoCC.mkDerivation {
    pname = "jellyfin-plugin-sso";
    version = "4.0.0.3";
    src = pkgs.fetchurl {
      url = "https://github.com/9p4/jellyfin-plugin-sso/releases/download/v4.0.0.3/sso-authentication_4.0.0.3.zip";
      hash = "sha256-3glRJVvsTtZGA3ZB5+CqEhCzoAoUFAZUgIe+2ZTLm90=";
    };
    nativeBuildInputs = [ pkgs.unzip ];
    unpackPhase = "unzip $src -d .";
    installPhase = "mkdir -p $out && cp *.dll meta.json $out/";
  };
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

  systemd.tmpfiles.rules = [
    "L+ ${config.services.jellyfin.dataDir}/plugins/SSO-Auth_4.0.0.3 - - - - ${ssoPlugin}"
  ];

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
