{
  config,
  pkgs,
  lib,
  ...
}:
let
  root = "/srv/users";
  kanidm = "${config.services.kanidm.package}/bin/kanidm";

  # Every per-user account and directory, derived from kanidm at runtime rather
  # than declared here. That is deliberate on two counts:
  #
  #   - this repo is public, and a list of `users.users` is a list of everyone
  #     using the service. Names live in kanidm, which is private.
  #   - uids must agree across nodes, because shared storage carries the number
  #     and not the name. Kanidm derives the gidnumber from the account uuid,
  #     which replicates, so every node reaches the same answer without anyone
  #     maintaining a map.
  #
  # Samba used to own this and named each user in a share definition. It was
  # replaced by webdav (modules/webdav-media.nix), never carried a single file,
  # and was the last thing forcing names into the config.
  sync = pkgs.writeShellScript "user-accounts-sync" ''
    set -euo pipefail
    export HOME="$STATE_DIRECTORY"
    pw=${config.sops.secrets.kanidm-idm-admin-password.path}

    for i in $(seq 1 30); do
      if KANIDM_PASSWORD="$(cat "$pw")" ${kanidm} login -D idm_admin >/dev/null 2>&1; then
        break
      fi
      [ "$i" = 30 ] && { echo "kanidm did not become ready" >&2; exit 1; }
      sleep 2
    done

    install -d -m 0755 -o root -g root ${root}
    # where modules/webdav-media.nix sends a request whose token carried no
    # usable username. Root-owned and unwritable on purpose.
    install -d -m 0555 -o root -g root ${root}/__denied__

    # Two sources, deliberately. Provisioning at ENROLMENT rather than at
    # promotion is what makes a later `group add-members users` take effect with
    # no filesystem work behind it - by the time anything grants access, the uid
    # and the directory already exist.
    #
    # `passkeys` is the enrolment signal: it appears on the entry the moment
    # someone follows the emailed link and registers one. It is not
    # `credential status`, which answers EmptyResponse for a passkey-only
    # account and would have been the wrong test. Members of `users` are
    # included regardless so that a password-only account, or one that predates
    # any of this, is never left unprovisioned.
    enrolled=$(${kanidm} -o json person list \
      | jq -r '(. // []) | .[] | select((.passkeys // []) | length > 0) | .name[0]')
    granted=$(${kanidm} -o json group list-members users \
      | jq -r '(. // []) | .[] | split("@")[0]')

    for u in $(printf '%s\n%s\n' "$enrolled" "$granted" | sort -u); do
      [ -n "$u" ] || continue

      # idempotent, and deliberately WITHOUT --gidnumber: kanidm derives one
      # from the account uuid. An explicit number here would have to be
      # declared somewhere, and that somewhere is this public file.
      ${kanidm} person posix set "$u" >/dev/null

      gid=$(${kanidm} -o json person get "$u" | jq -r '.attrs.gidnumber[0] // empty')
      [ -n "$gid" ] || { echo "no gidnumber for $u after posix set" >&2; exit 1; }

      # Numeric, not the name. nss asks kanidm-unixd, whose cache may not have
      # caught up with the account we just created, and `install -o <name>`
      # would fail for a user that demonstrably exists.
      #
      # 0750 rather than 0700 is load-bearing: on posix acls the group bits are
      # the mask that caps named entries, so 0700 masks the jellyfin entry down
      # to nothing. The group holds only this user, so the bits grant no one
      # anything by themselves.
      install -d -m 0750 -o "$gid" -g "$gid" ${root}/"$u"

      setfacl -m u:jellyfin:r-x ${root}/"$u"
      setfacl -d -m u:jellyfin:r-x ${root}/"$u"
      setfacl -m u:nginx:rwx ${root}/"$u"
      setfacl -d -m u:nginx:rwx ${root}/"$u"
      setfacl -d -m "u:$gid:rwx" ${root}/"$u"

      echo "user $u -> uid $gid"
    done
  '';
in
{
  systemd.services.user-accounts = {
    description = "Per-user posix accounts and directories, from kanidm";
    after = [
      "kanidm.service"
      "zfs-mount.service"
      "zfs-datasets.service"
    ];
    requires = [
      "kanidm.service"
      "zfs-datasets.service"
    ];
    wantedBy = [ "multi-user.target" ];
    path = [
      pkgs.acl
      pkgs.coreutils
      pkgs.jq
    ];
    # Every five minutes, not just at activation. Without this the unit runs at
    # boot and never again, so an account enrolled afterwards would have no uid
    # and no directory until the next nixos-rebuild - and webdav would answer
    # 500 for them in the meantime. RemainAfterExit is deliberately NOT set:
    # a oneshot that stays "active (exited)" cannot be re-triggered by a timer.
    startAt = "*:0/5";

    serviceConfig = {
      Type = "oneshot";
      StateDirectory = "user-accounts"; # HOME for the kanidm cli token cache
      ExecStart = sync;
    };
  };
}
