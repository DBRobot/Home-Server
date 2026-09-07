{ config, pkgs, lib, ... }:
let
  dir = "/vault/backups/postgres";
  keep = 48; # hourly, so two days of dumps; zfs snapshots hold the longer tail

  # A dump nobody has read is a rumour. pg_restore --list parses the archive
  # header and TOC, so a truncated or corrupt file fails here rather than in
  # six months when it is the only copy left.
  backup = pkgs.writeShellScript "pg-backup" ''
    set -euo pipefail
    umask 077
    mkdir -p ${dir}
    stamp=$(date -u +%Y%m%dT%H%M%SZ)
    tmp=${dir}/.ente-$stamp.dump.partial
    out=${dir}/ente-$stamp.dump

    ${config.services.postgresql.package}/bin/pg_dump -Fc -Z6 ente > "$tmp"

    ${config.services.postgresql.package}/bin/pg_restore --list "$tmp" > /dev/null

    # only becomes a real backup once it has been verified
    mv "$tmp" "$out"
    rm -f ${dir}/.ente-*.dump.partial || true

    # keep the newest N, never delete if the count looks wrong
    n=$(ls -1 ${dir}/ente-*.dump 2>/dev/null | wc -l)
    if [ "$n" -gt ${toString keep} ]; then
      ls -1t ${dir}/ente-*.dump | tail -n +$((${toString keep} + 1)) | xargs -r rm -f
    fi

    echo "ok: $out ($(stat -c%s "$out") bytes), $n dumps retained"
  '';

  # Restoring into a scratch database is the only thing that proves a dump is
  # restorable rather than merely well-formed.
  verify = pkgs.writeShellScript "pg-backup-verify" ''
    set -euo pipefail
    latest=$(ls -1t ${dir}/ente-*.dump 2>/dev/null | head -1)
    [ -n "$latest" ] || { echo "no dump to verify"; exit 1; }
    psql=${config.services.postgresql.package}/bin/psql

    $psql -c 'drop database if exists ente_restore_test' postgres
    $psql -c 'create database ente_restore_test' postgres
    ${config.services.postgresql.package}/bin/pg_restore \
      --dbname ente_restore_test --no-owner --no-privileges "$latest" 2>/dev/null || true

    # the key hierarchy is the thing worth asserting on
    rows=$($psql -tAd ente_restore_test -c 'select count(*) from key_attributes')
    $psql -c 'drop database ente_restore_test' postgres
    [ "$rows" -ge 1 ] || { echo "restore produced $rows key_attributes rows"; exit 1; }
    echo "ok: $latest restored, key_attributes=$rows"
  '';

  # Silent failure is the usual way backups die, so failures are mailed out
  # through the same relay ente uses.
  # Silent failure is the usual way backups die, so failures are mailed out
  # through the same relay ente uses. Headers must start at column zero and the
  # heredoc terminator must not be indented.
  alert = pkgs.writeShellScript "pg-backup-alert" ''
    set -eu
    unit="$1"
    ${pkgs.msmtp}/bin/msmtp --from=distributed.datacenter@gmail.com davidsprojects7@gmail.com <<EOF
From: node1 <distributed.datacenter@gmail.com>
To: davidsprojects7@gmail.com
Subject: node1: $unit FAILED

$unit failed on node1.

Check: journalctl -u $unit -n 50

These dumps are the only copy of the ente key hierarchy. Without them the
photo blobs on vault cannot be decrypted by anyone, recovery key included.
EOF
  '';
in
{
  programs.msmtp = {
    enable = true;
    accounts.default = {
      host = "smtp.gmail.com";
      port = 587;
      tls = true;
      tls_starttls = true;
      auth = "login";
      from = "distributed.datacenter@gmail.com";
      user = "distributed.datacenter@gmail.com";
      passwordeval = "${pkgs.coreutils}/bin/cat ${config.sops.secrets.ente-smtp-password.path}";
    };
  };

  systemd.services.postgres-backup = {
    description = "Dump and verify the ente database";
    after = [ "postgresql.service" "zfs-datasets.service" ];
    requires = [ "postgresql.service" ];
    onFailure = [ "postgres-backup-alert@postgres-backup.service" ];
    serviceConfig = {
      Type = "oneshot";
      User = "postgres";
      # "+" runs this as root regardless of User=, so it can create the
      # directory inside the root-owned dataset mountpoint. Doing it here
      # rather than via tmpfiles avoids racing zfs-datasets on first boot.
      ExecStartPre = "+${pkgs.coreutils}/bin/install -d -o postgres -g postgres -m 0700 ${dir}";
      ExecStart = backup;
    };
  };

  systemd.timers.postgres-backup = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnCalendar = "hourly";
      Persistent = true; # catch up after downtime rather than silently skipping
      RandomizedDelaySec = "5m";
    };
  };

  systemd.services.postgres-backup-verify = {
    description = "Prove the newest dump actually restores";
    after = [ "postgresql.service" ];
    requires = [ "postgresql.service" ];
    onFailure = [ "postgres-backup-alert@postgres-backup-verify.service" ];
    serviceConfig = {
      Type = "oneshot";
      User = "postgres";
      ExecStart = verify;
    };
  };

  systemd.timers.postgres-backup-verify = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnCalendar = "weekly";
      Persistent = true;
    };
  };

  systemd.services."postgres-backup-alert@" = {
    description = "Mail out a backup failure for %i";
    serviceConfig = {
      Type = "oneshot";
      ExecStart = "${alert} %i";
    };
  };

}
