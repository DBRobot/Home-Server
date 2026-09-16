{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.dd.garage;
  meta = "/var/lib/garage/meta";
in
{
  # One garage cluster across every storage box. Each box holds a replica
  # of every object (replication_factor below), so losing a box loses no
  # data and reads keep working with a box down. Writes need every replica
  # up until there are three boxes: a two-box cluster that accepted writes
  # with one box away would hand out reads that miss what was just written.
  # No box is trusted, and garage does not change that: a box in the layout
  # can read and delete every object in the cluster. What it holds is
  # ciphertext (ente, the media tier and restic all encrypt before garage
  # sees anything), so the exposure is durability, and the two-copy rule is
  # the answer to that.
  options.dd.garage = {
    zone = lib.mkOption {
      type = lib.types.str;
      description = "Layout zone of this box: its region id from fleet/boxes.json.";
    };
    capacity = lib.mkOption {
      type = lib.types.str;
      description = "Capacity this box offers the layout, e.g. \"3T\".";
    };
    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "/srv/garage";
      description = "Where the blocks live; a dataset the box's hardware file declares.";
    };
    publicAddr = lib.mkOption {
      type = lib.types.str;
      description = "host:port the other boxes reach this box's rpc on; its tailnet address.";
    };
    peers = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "The other storage boxes as id@host:port. Empty until a box has started once and published its id.";
    };
    replicationFactor = lib.mkOption {
      type = lib.types.int;
      default = 2;
    };
    envFile = lib.mkOption {
      type = lib.types.path;
      description = "File with GARAGE_RPC_SECRET, the same on every storage box.";
    };
    setupEnvFiles = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = [ ];
      description = "Files with key ids and secrets the setup snippets below read.";
    };
    setup = lib.mkOption {
      type = lib.types.lines;
      default = "";
      description = "Shell run after the layout converges: the buckets and keys a role wants. Must be safe to re-run.";
    };
  };

  config = {
    services.garage = {
      enable = true;
      package = pkgs.garage; # no default; nixpkgs ships several majors
      environmentFile = cfg.envFile;
      settings = {
        metadata_dir = meta;
        data_dir = cfg.dataDir;
        db_engine = "lmdb";
        replication_factor = cfg.replicationFactor;
        consistency_mode = "consistent";
        # a daily copy of the metadata db, in ${meta}/snapshots; the backup
        # module ships it, so a cluster whose metadata is gone is rebuilt
        # from the last snapshot plus the blocks
        metadata_auto_snapshot_interval = "1d";
        # rpc on every interface; the firewall admits the tailnet and nothing
        # else, and the rpc is authenticated by the shared secret regardless
        rpc_bind_addr = "0.0.0.0:3901";
        rpc_public_addr = cfg.publicAddr;
        bootstrap_peers = cfg.peers;
        s3_api = {
          s3_region = "us-east-1"; # ente requires this string regardless of reality
          # every interface, so a box without garage can back up to this one
          # over the tailnet; the gateway proxies the public name to it
          api_bind_addr = "0.0.0.0:3900";
          root_domain = ".s3.${config.dd.domain}";
        };
      };
    };

    # The module defaults to DynamicUser, which allocates a UID at runtime - so
    # there is no stable owner for data on the pool, and garage cannot write
    # there. A static user is the normal answer for persistent data.
    users.users.garage = {
      isSystemUser = true;
      group = "garage";
      home = "/var/lib/garage";
    };
    users.groups.garage = { };

    systemd.services.garage = {
      serviceConfig = {
        DynamicUser = false;
        User = "garage";
        Group = "garage";
      };
      # the data dir is a dataset; do not start on an empty mountpoint
      unitConfig.RequiresMountsFor = [ cfg.dataDir ];
      after = [ "zfs-datasets.service" ];
      wants = [ "zfs-datasets.service" ];
    };

    systemd.tmpfiles.rules = [
      "d ${cfg.dataDir} 0750 garage garage -"
      "d /var/lib/garage 0750 garage garage -"
      "d ${meta} 0700 garage garage -"
    ];

    # Converging, like zfs-datasets: safe to re-run on every rebuild and on a
    # timer, because the layout needs every box before it applies and the
    # other box may join later.
    systemd.services.garage-setup = {
      description = "Converge garage layout, buckets and keys";
      after = [ "garage.service" ];
      requires = [ "garage.service" ];
      wantedBy = [ "multi-user.target" ];
      path = [
        config.services.garage.package
        pkgs.awscli2
        pkgs.coreutils
        pkgs.gnugrep
      ];
      serviceConfig = {
        Type = "oneshot";
        EnvironmentFile = [ cfg.envFile ] ++ cfg.setupEnvFiles;
      };
      script = ''
        set -u
        for i in $(seq 1 60); do garage status >/dev/null 2>&1 && break; sleep 2; done

        # this box's own role in the layout; the other boxes stage theirs.
        # apply fails while the layout has fewer boxes than copies, and that
        # is fine: the timer tries again once the other box has staged
        NODE=$(garage node id -q | cut -d@ -f1)
        # layout show prints ids cut to 16 hex
        if ! garage layout show 2>/dev/null | grep -q "^''${NODE:0:16}"; then
          garage layout assign -z ${cfg.zone} -c ${cfg.capacity} "$NODE"
        fi
        # garage prints the version to apply; computing it ourselves gets
        # "Invalid new layout version"
        VER=$(garage layout show 2>/dev/null | grep -oE -- "--version [0-9]+" | grep -oE "[0-9]+" | head -1)
        if [ -n "$VER" ]; then
          garage layout apply --version "$VER" || echo "layout not applied yet: waiting for the other box"
        fi

        ${cfg.setup}
      '';
    };
    systemd.timers.garage-setup = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "10m";
        OnUnitActiveSec = "10m";
        RandomizedDelaySec = "1m";
      };
    };
  };
}
