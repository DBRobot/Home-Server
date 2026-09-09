{
  config,
  pkgs,
  lib,
  ...
}:
let
  base = "distributed-datacenter.duckdns.org";
  url = "https://idm.${base}";
  kanidm = "${config.services.kanidm.package}/bin/kanidm";
  dir = "/var/lib/kanidm-service-accounts";

  # kanidm-provision has no service-account support at all, so these are
  # converged by a oneshot instead - the same shape as garage-setup.nix. The
  # difference from garage is that a token cannot be pre-generated into sops:
  # kanidm mints it and never accepts an imported one, so the token is runtime
  # state and this unit owns its lifecycle.
  accounts = {
    signup = {
      displayName = "Signup service";
      # creating persons and sending credential-reset intents is exactly what
      # this builtin group delegates. It is inside idm_high_privilege, so the
      # service is deliberately localhost-only with nginx in front.
      group = "idm_people_on_boarding";
      owner = "signup";
    };
    mail-sender = {
      displayName = "Mail sender";
      group = "idm_mail_servers";
      owner = "kanidm-mail-sender";
    };
  };

  converge = pkgs.writeShellScript "kanidm-service-accounts" ''
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

    ${lib.concatStringsSep "\n" (
      lib.mapAttrsToList (name: a: ''
        if ! ${kanidm} service-account get ${name} >/dev/null 2>&1; then
          ${kanidm} service-account create ${name} "${a.displayName}" idm_admins >/dev/null
        fi
        ${kanidm} group add-members ${a.group} ${name} >/dev/null

        tok=${dir}/${name}.token
        # Re-mint when the cached token is missing OR no longer accepted. A
        # token that was revoked server-side would otherwise 401 forever, and
        # nothing would ever notice - /v1/self is the cheapest authenticated
        # call that proves it still works.
        if [ ! -s "$tok" ] || ! ${pkgs.curl}/bin/curl -sf -o /dev/null \
             -H "Authorization: Bearer $(cat "$tok")" ${url}/v1/self; then
          umask 077
          # -o json because text mode prints a "displayed ONCE" banner above the
          # token. The emptiness check below is load-bearing rather than
          # defensive: the cli logs a generate failure with error!() and still
          # EXITS ZERO, so pipefail cannot see it.
          ${kanidm} -o json service-account api-token generate --readwrite \
            ${name} "node1 ${name}" \
            | ${pkgs.jq}/bin/jq -r '.result // empty' > "$tok".new
          [ -s "$tok".new ] || { echo "no token minted for ${name}" >&2; exit 1; }
          mv "$tok".new "$tok"
          echo "minted a new api token for ${name}"
        fi
        chown ${a.owner} "$tok"
        chmod 0400 "$tok"
      '') accounts
    )}
  '';
in
{
  systemd.services.kanidm-service-accounts = {
    description = "Converge the service accounts and api tokens kanidm hands out";
    after = [ "kanidm.service" ];
    requires = [ "kanidm.service" ];
    wantedBy = [ "multi-user.target" ];
    # every consumer of a token must start after this, or it reads an empty file
    before = [
      "signup.service"
      "kanidm-mail-sender.service"
    ];
    path = [
      pkgs.coreutils
      pkgs.curl
      pkgs.jq
    ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      StateDirectory = "kanidm-service-accounts"; # also HOME for the cli token cache
      StateDirectoryMode = "0700";
      ExecStart = converge;
    };
  };
}
