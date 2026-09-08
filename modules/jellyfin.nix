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

  # A symlink into the store does not work: jellyfin rewrites meta.json when
  # it loads a plugin, so the directory has to be a real writable copy. With
  # the symlink it loaded the assemblies and then threw
  # UnauthorizedAccessException from SaveManifest, leaving the endpoint 503.
  systemd.services.jellyfin-plugins = {
    description = "Install jellyfin plugins from the store into its data dir";
    before = [ "jellyfin.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      dir=${config.services.jellyfin.dataDir}/plugins/SSO-Auth_4.0.0.3
      rm -rf "$dir"
      mkdir -p "$dir"
      cp ${ssoPlugin}/* "$dir"/
      chown -R jellyfin:${config.services.jellyfin.group} "$dir"
      chmod -R u+w "$dir"
    '';
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
