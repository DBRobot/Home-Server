{
  config,
  pkgs,
  ...
}:
let
  root = "/srv/users";
  images = "/srv/images";
  directory = "/var/lib/dd-verify/keys";

  # Every per-user directory, derived from the directory at runtime rather
  # than declared here. Who exists is whoever published a signed entry; there
  # is no list to keep and no server to ask.
  #
  # The uid is a hash of the name. Every box computes the same one from the
  # same entry, so shared storage carries a number that means the same
  # everywhere and nobody maintains a map. No nss entry exists for it: the
  # files are owned by a number, and every reader here (nginx, jellyfin) gets
  # in through an acl, never through the name.
  sync = pkgs.writeShellScript "user-accounts-sync" ''
    set -euo pipefail

    install -d -m 0755 -o root -g root ${root}
    install -d -m 0755 -o root -g root ${images}
    # where modules/webdav-media.nix sends a request whose token carried no
    # usable username. Root-owned and unwritable on purpose.
    install -d -m 0555 -o root -g root ${root}/__denied__
    install -d -m 0555 -o root -g root ${images}/__denied__

    for f in ${directory}/*.json; do
      [ -e "$f" ] || continue
      u=$(basename "$f" .json)
      case "$u" in *.passkeys) continue ;; esac
      # the same whitelist the verifier applies to a name
      printf '%s' "$u" | grep -qE '^[a-z0-9][a-z0-9._-]{0,63}$' || continue

      # 1000000 + 30 bits of sha256: numbers no distro hands out, no two
      # names collide in practice, and any box reaches the same one
      uid=$(( 1000000 + 0x$(printf 'dd-uid:%s' "$u" | sha256sum | cut -c1-8) % 1073741824 ))

      # 0750 rather than 0700 is load-bearing: on posix acls the group bits
      # are the mask that caps named entries, so 0700 masks the jellyfin
      # entry down to nothing. The group holds only this user, so the bits
      # grant no one anything by themselves.
      install -d -m 0750 -o "$uid" -g "$uid" ${root}/"$u"
      install -d -m 0750 -o "$uid" -g "$uid" ${images}/"$u"
      # a directory made under an earlier scheme (kanidm handed out the
      # numbers once) is taken over rather than orphaned
      for d in ${root}/"$u" ${images}/"$u"; do
        [ "$(stat -c %u "$d")" = "$uid" ] || chown -R "$uid:$uid" "$d"
      done

      setfacl -m u:jellyfin:r-x ${root}/"$u"
      setfacl -d -m u:jellyfin:r-x ${root}/"$u"
      setfacl -m u:nginx:rwx ${root}/"$u"
      setfacl -d -m u:nginx:rwx ${root}/"$u"
      setfacl -d -m "u:$uid:rwx" ${root}/"$u"

      # The archive dir: same owner and the same nginx entry, but deliberately
      # NO jellyfin entry. Nothing in here is media and nothing in here is
      # readable by anyone but the uploader anyway - it arrives encrypted.
      setfacl -m u:nginx:rwx ${images}/"$u"
      setfacl -d -m u:nginx:rwx ${images}/"$u"
      setfacl -d -m "u:$uid:rwx" ${images}/"$u"

      echo "user $u -> uid $uid"
    done
  '';
in
{
  systemd.services.user-accounts = {
    description = "Per-user directories, from the directory";
    after = [
      "zfs-mount.service"
      "zfs-datasets.service"
      "dd-verify.service"
    ];
    requires = [ "zfs-datasets.service" ];
    wantedBy = [ "multi-user.target" ];
    path = [
      pkgs.acl
      pkgs.coreutils
      pkgs.gnugrep
    ];
    # Every five minutes, not just at activation: an identity published in
    # between would have no directory until the next rebuild, and webdav
    # would answer 500 for them. RemainAfterExit is deliberately NOT set: a
    # oneshot that stays "active (exited)" cannot be re-triggered by a timer.
    startAt = "*:0/5";
    serviceConfig = {
      Type = "oneshot";
      ExecStart = sync;
    };
  };
}
