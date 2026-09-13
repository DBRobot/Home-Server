{
  config,
  pkgs,
  ...
}:
let
  kanidm = "${config.services.kanidm.package}/bin/kanidm";
  clients = [
    "llm"
    "dd"
    "grafana"
    "jellyfin"
  ];
  # A refresh token lives 16 hours, an access token 15 minutes. A key that
  # has not signed anything for eight days has signed nothing still valid, so
  # revoking it logs nobody out - it only closes the door for a copy of that
  # key that left the box.
  graceDays = 8;

  rotate = pkgs.writeShellScript "kanidm-key-rotation" ''
    set -euo pipefail
    export HOME="$STATE_DIRECTORY"
    export KANIDM_NAME=idm_admin
    KANIDM_PASSWORD="$(cat ${config.sops.secrets.kanidm-idm-admin-password.path})" \
      ${kanidm} login -D idm_admin >/dev/null
    now=$(date +%s)
    cutoff=$((now - ${toString graceDays} * 86400))
    # kanidm reports a key's valid_from as 0 both for keys that predate
    # rotation and for the replacement it spawns on every revoke, so the
    # listing cannot say how old a key is. This file can: "client kid epoch",
    # the first time this job saw the key. A key is revoked only once it has
    # been known for the whole grace period, and never if it is the newest
    # signer.
    seen="$STATE_DIRECTORY/seen"
    touch "$seen"
    for c in ${toString clients}; do
      ${kanidm} system oauth2 rotate-cryptographic-keys "$c" now >/dev/null
      keys=$(${kanidm} system oauth2 get "$c" | ${pkgs.gawk}/bin/awk '/^key_internal_data:/ && $3 == "valid" { sub(":", "", $2); print $2, $4, $NF }')
      # remember every valid key we have not met before
      echo "$keys" | while read -r kid _ _; do
        [ -n "$kid" ] || continue
        grep -q "^$c $kid " "$seen" || echo "$c $kid $now" >> "$seen"
      done
      for tpe in jws_es256 jwe_a128gcm; do
        newest=$(echo "$keys" | ${pkgs.gawk}/bin/awk -v t="$tpe" '$2 == t { if ($3+0 >= m) { m = $3+0; k = $1 } } END { print k }')
        echo "$keys" | ${pkgs.gawk}/bin/awk -v t="$tpe" '$2 == t { print $1 }' | while read -r kid; do
          [ "$kid" = "$newest" ] && continue
          first=$(${pkgs.gawk}/bin/awk -v c="$c" -v k="$kid" '$1 == c && $2 == k { print $3 }' "$seen")
          if [ -n "$first" ] && [ "$first" -lt "$cutoff" ]; then
            ${kanidm} system oauth2 revoke-cryptographic-key "$c" "$kid" >/dev/null
            echo "$c: revoked $kid (known since $first)"
          fi
        done
      done
      echo "$c: rotated"
    done
  '';
in
{
  # The signing keys live in kanidm's database and cannot leave it. What can
  # be bounded is how long a COPY of that database stays useful: every week a
  # fresh key signs, and any key older than the grace period is revoked. A
  # dump, a snapshot or a stolen disk from more than ~two weeks ago can no
  # longer mint a token anyone accepts. Rotation is graceful - old keys keep
  # verifying until revoked - so the weekly run logs nobody out.
  systemd.services.kanidm-key-rotation = {
    description = "Rotate kanidm's oauth2 signing keys, retire old ones";
    after = [ "kanidm.service" ];
    requires = [ "kanidm.service" ];
    startAt = "Mon *-*-* 04:00:00";
    path = [
      pkgs.coreutils
      pkgs.gawk
    ];
    serviceConfig = {
      Type = "oneshot";
      StateDirectory = "kanidm-key-rotation"; # HOME for the cli token cache
      StateDirectoryMode = "0700";
      ExecStart = rotate;
    };
  };
}
