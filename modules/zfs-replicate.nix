{ pkgs, ... }:
let
  # vault lives on the external usb drive (sdb), tank on the internal nvme.
  # Replicating between them means either disk can die without taking the only
  # copy with it. It is NOT offsite: fire or theft still takes both, and that
  # needs node2 or something off the machine entirely.
  pairs = {
    # the ente library. 74.8G today; tank has ~790G free, so this holds until
    # the photo library passes roughly 700G.
    photos = {
      source = "vault/photos";
      target = "tank/replica/photos";
    };
    # the postgres dumps. Small, and the most important bytes on the machine:
    # without the key hierarchy the photo blobs decrypt for nobody. Until now
    # they sat on the same disk as the thing they exist to recover.
    backups = {
      source = "vault/backups";
      target = "tank/replica/backups";
    };
    # per-user uploads
    users = {
      source = "vault/users";
      target = "tank/replica/users";
    };
  };

  alert = pkgs.writeShellScript "replicate-alert" ''
    set -eu
    unit="$1"
    ${pkgs.msmtp}/bin/msmtp --from=distributed.datacenter@gmail.com davidsprojects7@gmail.com <<EOF
From: node1 <distributed.datacenter@gmail.com>
To: davidsprojects7@gmail.com
Subject: node1: $unit FAILED

$unit failed on node1, so vault is back to being the only copy.

  journalctl -u $unit -n 50
  zfs list -t snapshot -o name,creation tank/replica

EOF
  '';
in
{
  services.syncoid = {
    enable = true;
    interval = "hourly";
    # -x mountpoint: vault/users sets its mountpoint LOCALLY (/srv/users), and a
    # received copy carrying that property would fight the live dataset for the
    # same directory. Excluding it makes every replica inherit from
    # tank/replica, which is mountpoint=none - they exist to be sent back, not
    # browsed. readonly stops anything writing into a replica and breaking the
    # next incremental.
    commonArgs = [ "--no-privilege-elevation" ];
    commands = builtins.mapAttrs (_: p: {
      inherit (p) source target;
      recvOptions = "-x mountpoint -o readonly=on";
    }) pairs;
  };

  systemd.services = builtins.mapAttrs (name: _: {
    onFailure = [ "replicate-alert@syncoid-${name}.service" ];
  }) (builtins.listToAttrs (
    map (n: {
      name = "syncoid-${n}";
      value = null;
    }) (builtins.attrNames pairs)
  ))
  // {
    "replicate-alert@" = {
      description = "Mail out a replication failure for %i";
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${alert} %i";
      };
    };
  };
}
