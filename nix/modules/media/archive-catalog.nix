{ pkgs, ... }:
let
  dir = "/srv/catalog/public";

  importer = pkgs.writers.writePython3Bin "archive-catalog-import" {
    flakeIgnore = [
      "E501"
      "W503"
      "W504"
      "E128"
      "E126"
      "E131"
    ];
  } (builtins.readFile ./archive-catalog/import.py);
in
{
  # Deliberately NOT under /srv/media: the union in modules/media-tier.nix is
  # household media, this is a public catalogue, and keeping them apart is the
  # whole point. Its only writer is this service - there is no path by which a
  # user puts anything into the public library, so nothing needs policing at
  # runtime.
  users.users.archive-catalog = {
    isSystemUser = true;
    group = "archive-catalog";
  };
  users.groups.archive-catalog = { };

  systemd.tmpfiles.rules = [
    "d /srv/catalog 0755 archive-catalog media -"
    "d ${dir} 0755 archive-catalog media -" # media group so jellyfin can read
  ];

  systemd.services.archive-catalog = {
    description = "Refresh the public-domain film catalogue from archive.org";
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
    serviceConfig = {
      Type = "oneshot";
      User = "archive-catalog";
      Group = "archive-catalog";
      ExecStart = "${importer}/bin/archive-catalog-import";
      Environment = [ "CATALOG_DIR=${dir}" ];
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      NoNewPrivileges = true;
      ReadWritePaths = [ dir ];
    };
  };

  systemd.timers.archive-catalog = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnCalendar = "weekly";
      Persistent = true;
      RandomizedDelaySec = "1h"; # donation-funded archive, do not stampede it
    };
  };
}
