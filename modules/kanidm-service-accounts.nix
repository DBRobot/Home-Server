{
  config,
  pkgs,
  lib,
  ...
}:
let
  kanidm = "${config.services.kanidm.package}/bin/kanidm";

  # Which service accounts exist and what they are allowed to do. Their TOKENS
  # are not here - they live in sops like every other secret, because kanidm
  # only ever *generates* a credential (there is `service-account credential
  # generate` and no `set`, and an api token is a jwt the server signs), so a
  # token cannot be pre-generated the way garage's access key is. It is minted
  # once by hand and encrypted, which makes it a one-time external act like
  # `zpool create` - see mint-kanidm-token below.
  accounts = {
    signup = {
      displayName = "Signup service";
      # creating persons and sending credential-reset intents is exactly what
      # this builtin group delegates. It sits inside idm_high_privilege, so the
      # service that holds its token is localhost-only with nginx in front.
      group = "idm_people_on_boarding";
    };
    # Underscore, not a hyphen: kanidm rejects "-" in an account name, and
    # reports it as ReferentialIntegrity("Uuid referenced not found in
    # database") rather than as a name validation error. The systemd unit and
    # its unix user keep the hyphenated name; only the kanidm account differs.
    mail_sender = {
      displayName = "Mail sender";
      # needs read-WRITE: draining the queue means marking messages as sent
      group = "idm_mail_servers";
    };
  };

  # Bootstrap and rotation in one command, so the manual step is one line and
  # the token never lands in a shell history or a scrollback.
  #
  #   mint-kanidm-token signup | ssh ... (straight into sops on the legion)
  mintToken = pkgs.writeShellScriptBin "mint-kanidm-token" ''
    set -euo pipefail
    if [ $# -ne 1 ]; then
      echo "usage: mint-kanidm-token <${lib.concatStringsSep "|" (lib.attrNames accounts)}>" >&2
      exit 2
    fi
    export HOME=$(mktemp -d)
    trap 'rm -rf "$HOME"' EXIT
    KANIDM_PASSWORD="$(cat ${config.sops.secrets.kanidm-idm-admin-password.path})" \
      ${kanidm} login -D idm_admin >/dev/null
    # -o json because text mode prints a "displayed ONCE" banner above the
    # token. The emptiness check is load-bearing rather than defensive: the cli
    # logs a generate failure with error!() and still EXITS ZERO.
    tok=$(${kanidm} -o json service-account api-token generate --readwrite \
      "$1" "node1 $1" | ${pkgs.jq}/bin/jq -r '.result // empty')
    [ -n "$tok" ] || { echo "no token minted for $1" >&2; exit 1; }
    printf '%s' "$tok"
  '';

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
          # entry-managed-by resolves an spn or a uuid, NOT a bare name -
          # "idm_admins" fails with ReferentialIntegrity("Uuid referenced not
          # found in database"). The builtin uuid rather than the spn, so the
          # duckdns domain is not baked into one more place.
          ${kanidm} service-account create ${name} "${a.displayName}" \
            00000000-0000-0000-0000-000000000001 >/dev/null
          echo "created service account ${name}"
        fi
        ${kanidm} group add-members ${a.group} ${name} >/dev/null
      '') accounts
    )}

    # idm_people_on_boarding delegates CREATING people. It does not delegate
    # writing to a group, and kanidm answers 404 rather than 403 for a group the
    # caller cannot see - so without this, signup creates the account and then
    # fails to file it. Entry-manager rather than idm_group_admins: it confers
    # rights over this ONE group and nothing else, which is the whole point of
    # `pending` being separate. kanidm-provision cannot express this; its group
    # options are present/members/overwriteMembers only.
    ${kanidm} group set-entry-manager pending signup >/dev/null
  '';
in
{
  environment.systemPackages = [ mintToken ];

  # kanidm-provision has no service-account support at all, so the accounts
  # themselves are converged by a oneshot - the same shape as garage-setup.nix.
  # Only their EXISTENCE and group membership are handled here; nothing secret
  # passes through this unit.
  systemd.services.kanidm-service-accounts = {
    description = "Converge the service accounts kanidm delegates to";
    after = [ "kanidm.service" ];
    requires = [ "kanidm.service" ];
    wantedBy = [ "multi-user.target" ];
    before = [
      "signup.service"
      "kanidm-mail-sender.service"
    ];
    path = [ pkgs.coreutils ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      StateDirectory = "kanidm-service-accounts"; # HOME for the cli token cache
      StateDirectoryMode = "0700";
      ExecStart = converge;
    };
  };
}
