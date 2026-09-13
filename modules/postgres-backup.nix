{
  config,
  pkgs,
  lib,
  ...
}:
let
  dir = "/vault/backups/postgres";
  # services.postgresqlBackup writes <db>.sql.zstd and keeps one .prev. History
  # beyond that comes from zfs snapshots on the dataset, which is a mechanism
  # already trusted with the photos.
  dump = "${dir}/ente.sql.zstd";

  # The upstream module does the dump. These add the things it does not do, and
  # they are deliberately additive: if verification breaks, the backup still runs.
  verify = pkgs.writeShellScript "pg-backup-verify" ''
    set -euo pipefail
    psql=${config.services.postgresql.package}/bin/psql
    [ -s ${dump} ] || { echo "no dump at ${dump}"; exit 1; }

    # restoring into a scratch database is the only thing that proves a dump is
    # restorable rather than merely present
    $psql -q -c 'drop database if exists ente_restore_test' postgres
    $psql -q -c 'create database ente_restore_test' postgres
    # belt and braces: never feed a dump containing \connect into psql
    if ${pkgs.zstd}/bin/zstd -dc ${dump} | ${pkgs.gnugrep}/bin/grep -qE '^\\connect'; then
      echo "dump contains \\connect - refusing to restore, it would target the live db"
      exit 1
    fi
    ${pkgs.zstd}/bin/zstd -dc ${dump} | $psql -q -v ON_ERROR_STOP=1 -d ente_restore_test >/dev/null

    # the key hierarchy is the thing worth asserting on: without these rows the
    # blobs on vault cannot be decrypted by anyone
    keys=$($psql -tAd ente_restore_test -c 'select count(*) from key_attributes')
    files=$($psql -tAd ente_restore_test -c 'select count(*) from collection_files')
    $psql -q -c 'drop database ente_restore_test' postgres

    [ "$keys" -ge 1 ] || { echo "restored dump has $keys key_attributes rows"; exit 1; }
    echo "ok: key_attributes=$keys collection_files=$files"
  '';

  # Backups usually die silently rather than loudly.
  alert = pkgs.writeShellScript "pg-backup-alert" ''
        set -eu
        unit="$1"
        ${pkgs.msmtp}/bin/msmtp --from=distributed.datacenter@gmail.com alerts <<EOF
    From: node1 <distributed.datacenter@gmail.com>
    To: alerts
    Subject: node1: $unit FAILED

    $unit failed on node1.

      journalctl -u $unit -n 50

    These dumps are the only copy of the ente key hierarchy. Without them the
    photo blobs on vault cannot be decrypted by anyone, recovery key included.
    EOF
  '';
in
{
  # The dump itself is upstream's, not ours. It writes atomically via an
  # .in-progress file and runs as the postgres user.
  services.postgresqlBackup = {
    enable = true;
    databases = [
      "ente"
      "forgejo"
    ];
    location = dir;
    # NOT the default "-C". That emits CREATE DATABASE + \connect ente, so
    # restoring the dump into a scratch database would follow the \connect and
    # replay it into the LIVE one. Without -C the dump is portable and lands
    # wherever psql is pointed.
    pgdumpOptions = "--no-owner --no-privileges";
    compression = "zstd";
    compressionLevel = 6;
    startAt = "hourly";
  };

  systemd.services =
    lib.genAttrs [ "postgresqlBackup-ente" "postgresqlBackup-forgejo" ] (unit: {
      onFailure = [ "postgres-backup-alert@${unit}.service" ];
      unitConfig.RequiresMountsFor = "/vault";
      serviceConfig.ExecStartPre = [
        # "+" runs as root despite User=postgres, so it can create the directory
        # inside the root-owned dataset mountpoint. Doing it here rather than via
        # tmpfiles avoids racing zfs-datasets on first boot.
        "+${pkgs.coreutils}/bin/install -d -o postgres -g postgres -m 0700 ${dir}"
      ];
    })
    // {
      postgres-backup-verify = {
        description = "Restore the newest dump into a scratch database and check it";
        after = [ "postgresql.service" ];
        requires = [ "postgresql.service" ];
        onFailure = [ "postgres-backup-alert@postgres-backup-verify.service" ];
        startAt = "weekly";
        serviceConfig = {
          Type = "oneshot";
          User = "postgres";
          ExecStart = verify;
        };
      };

      "postgres-backup-alert@" = {
        description = "Mail out a backup failure for %i";
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${alert} %i";
        };
      };
    };
}
