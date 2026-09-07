{ config, pkgs, ... }:
let
  hot = "/vault/media";
  cold = "/mnt/cold";
  union = "/srv/media";
  cache = "/tank/rclone-cache";

  # Nothing demotes automatically: an age or size policy is a real decision and
  # this is a rehearsal. Run as root or the media user - the env file is 0400.
  mediaDemote = pkgs.writeShellScriptBin "media-demote" ''
    set -euo pipefail
    if [ $# -ne 1 ]; then
      echo "usage: media-demote <path under ${union}>" >&2
      exit 2
    fi
    rel="''${1#${union}/}"
    set -a
    . ${config.sops.templates."rclone.env".path}
    set +a
    exec ${pkgs.rclone}/bin/rclone move "${hot}/$rel" "cold:$rel" \
      --progress --delete-empty-src-dirs
  '';

  # mergerfs does not notify, so ordering for jellyfin needs an explicit wait.
  # It lives in a script because systemd would eat the $i in a loop written
  # inline.
  waitForMount = pkgs.writeShellScript "wait-for-media-union" ''
    for _ in $(seq 1 100); do
      ${pkgs.util-linux}/bin/mountpoint -q ${union} && exit 0
      sleep 0.1
    done
    exit 1
  '';
in
{
  environment.systemPackages = [ mediaDemote ];

  # mergerfs and the rclone mount both run as `media`, but jellyfin reads them
  # as `jellyfin`. Without this, allow_other is refused for non-root fuse mounts.
  programs.fuse.userAllowOther = true;

  users.users.media = {
    isSystemUser = true;
    group = "media";
  };
  users.groups.media = { };

  systemd.tmpfiles.rules = [
    # NOT ${hot}: that is a zfs dataset, and tmpfiles runs before zfs mounts
    # it, so anything set here ends up hidden under the mount. media-dirs
    # below owns it instead.
    "d ${cold} 0755 media media -"
    "d ${union} 0755 media media -"
    "d ${cache} 0700 media media -" # nvme, not the media pool
  ];

  systemd.services.media-dirs = {
    description = "Own the hot media tier after zfs has mounted it";
    after = [
      "zfs-mount.service"
      "zfs-datasets.service"
    ];
    requires = [ "zfs-datasets.service" ];
    before = [ "media-union.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      mkdir -p ${hot}
      chown media:media ${hot}
      chmod 2775 ${hot}
    '';
  };

  # The cold tier. Today it loops back to garage on this same disk, which buys
  # no capacity - the point is that the ciphertext is real and the mount works,
  # so node2 is a change of endpoint rather than a change of design.
  systemd.services.rclone-cold = {
    description = "Encrypted cold media tier (rclone crypt over garage S3)";
    after = [
      "garage.service"
      "garage-setup.service"
    ];
    requires = [
      "garage.service"
      "garage-setup.service" # creates the bucket the crypt remote writes into
    ];
    wantedBy = [ "multi-user.target" ];
    # fusermount3 must be the setuid wrapper; the one in the store is not, and
    # an unprivileged mount fails with EPERM without it.
    path = [ "/run/wrappers" ];
    serviceConfig = {
      Type = "notify";
      User = "media";
      Group = "media";
      EnvironmentFile = config.sops.templates."rclone.env".path;
      ExecStart = ''
        ${pkgs.rclone}/bin/rclone mount cold: ${cold} \
          --allow-other \
          --umask 002 \
          --cache-dir ${cache} \
          --vfs-cache-mode full \
          --vfs-cache-max-size 20G \
          --vfs-cache-max-age 168h \
          --vfs-read-chunk-size 32M \
          --vfs-read-chunk-size-limit 1G \
          --buffer-size 64M \
          --dir-cache-time 72h \
          --poll-interval 0
      '';
      ExecStop = "/run/wrappers/bin/fusermount -u ${cold}";
      Restart = "on-failure";
      RestartSec = 10;
    };
  };

  # Hot tier first and NC on the cold one, so nothing is ever created straight
  # into the encrypted remote - files land on local disk and are moved down
  # deliberately.
  systemd.services.media-union = {
    description = "mergerfs union of the hot and cold media tiers";
    after = [
      "rclone-cold.service"
      "media-dirs.service"
    ];
    requires = [
      "rclone-cold.service"
      "media-dirs.service"
    ];
    wantedBy = [ "multi-user.target" ];
    path = [ "/run/wrappers" ]; # setuid fusermount3, as above
    serviceConfig = {
      Type = "simple";
      User = "media";
      Group = "media";
      ExecStart = ''
        ${pkgs.mergerfs}/bin/mergerfs -f \
          -o allow_other,use_ino,cache.files=partial,dropcacheonclose=true \
          -o category.create=ff,moveonenospc=true,umask=002,fsname=media \
          ${hot}=RW:${cold}=NC ${union}
      '';
      ExecStartPost = waitForMount;
      ExecStop = "/run/wrappers/bin/fusermount -u ${union}";
      Restart = "on-failure";
      RestartSec = 5;
    };
  };
}
