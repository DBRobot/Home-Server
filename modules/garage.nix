{ config, pkgs, lib, ... }:
let
  base = "distributed-datacenter.duckdns.org";
in
{
  services.garage = {
    enable = true;
    package = pkgs.garage; # no default; nixpkgs ships several majors
    environmentFile = config.sops.templates."garage.env".path;
    settings = {
      metadata_dir = "/var/lib/garage/meta";
      data_dir = "/vault/photos"; # recordsize=1M, auto-snapshot, already declared
      db_engine = "lmdb";
      replication_factor = 1; # single vdev, single node
      rpc_bind_addr = "[::1]:3901";
      rpc_public_addr = "[::1]:3901";
      s3_api = {
        s3_region = "us-east-1"; # ente requires this string regardless of reality
        api_bind_addr = "127.0.0.1:3900";
        root_domain = ".s3.${base}";
      };
    };
  };

  systemd.tmpfiles.rules = [
    "d /vault/photos 0750 garage garage -"
    "d /var/lib/garage/meta 0700 garage garage -"
  ];

  # Converging, like modules/zfs-datasets.nix: safe to re-run on every rebuild.
  # The access key is IMPORTED from sops rather than minted here, so ente can be
  # configured with the same credentials at deploy time.
  systemd.services.garage-setup = {
    description = "Converge garage layout, bucket and access key";
    after = [ "garage.service" ];
    requires = [ "garage.service" ];
    wantedBy = [ "multi-user.target" ];
    path = [
      config.services.garage.package
      pkgs.coreutils
      pkgs.gnugrep
    ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      EnvironmentFile = [
        config.sops.templates."garage.env".path
        config.sops.templates."garage-key.env".path
      ];
    };
    script = ''
      for i in $(seq 1 60); do garage status >/dev/null 2>&1 && break; sleep 2; done

      NODE=$(garage node id -q | cut -d@ -f1)
      if ! garage layout show 2>/dev/null | grep -q "$NODE.*home"; then
        garage layout assign -z home -c 3T "$NODE"
        VER=$(garage layout show 2>/dev/null | grep -oE 'version [0-9]+' | head -1 | grep -oE '[0-9]+')
        garage layout apply --version $((''${VER:-0} + 1))
      fi

      garage bucket create ente 2>/dev/null || true
      garage key import "$GARAGE_KEY_ID" "$GARAGE_KEY_SECRET" --yes -n ente 2>/dev/null || true
      garage bucket allow --read --write --owner ente --key "$GARAGE_KEY_ID" 2>/dev/null || true
    '';
  };
}
