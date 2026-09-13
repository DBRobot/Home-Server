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
      # NOT idm_people_on_boarding. That builtin grants credential resets on
      # EVERY person - verified: the signup token could queue a reset link for
      # any existing account. signup_service is our own group, and the access
      # controls below let it create people and touch only members of
      # `pending`. Compromise of the signup service then yields new empty
      # accounts, not yours.
      group = "signup_service";
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

    # the receiver group has to exist before the loop puts anyone in it.
    # `group get` exits 0 for a missing group ("No matching entries"), so
    # existence is read off the output, not the exit code.
    if ! ${kanidm} group get signup_service 2>/dev/null | grep -qx 'name: signup_service'; then
      ${kanidm} group create signup_service 00000000-0000-0000-0000-000000000001 >/dev/null
      echo "created group signup_service"
    fi

    ${lib.concatStringsSep "\n" (
      lib.mapAttrsToList (name: a: ''
        if ! ${kanidm} service-account get ${name} 2>/dev/null | grep -qx 'name: ${name}'; then
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

    # The scoped access controls for the signup account. Three profiles:
    #   create   any person (creation is not the risk; a new entry is empty)
    #   modify   credential + expiry attrs, ONLY on members of pending
    #   search   the pending group's member list, so the service can tell a
    #            retry of a never-enrolled account from a taken name
    # Target scopes carry uuids, looked up here because they are runtime.
    # Writing access controls needs idm_access_control_admins, which only the
    # system `admin` account can grant - done once, converged every time.
    KANIDM_PASSWORD="$(cat ${config.sops.secrets.kanidm-admin-password.path})" \
      ${kanidm} login -D admin >/dev/null
    ${kanidm} group add-members idm_access_control_admins idm_admin >/dev/null 2>&1 || true
    KANIDM_PASSWORD="$(cat "$pw")" ${kanidm} login -D idm_admin >/dev/null

    ${kanidm} group remove-members idm_people_on_boarding signup >/dev/null 2>&1 || true

    pending=$(${kanidm} group get pending | ${pkgs.gnused}/bin/sed -n 's/^uuid: //p')
    receiver=$(${kanidm} group get signup_service | ${pkgs.gnused}/bin/sed -n 's/^uuid: //p')
    for acp in ${acpCreate} ${acpModify} ${acpSearch}; do
      name=$(${pkgs.jq}/bin/jq -r '.name[0]' "$acp")
      n=$(${kanidm} -o json raw search "name eq \"$name\"" | ${pkgs.jq}/bin/jq '.totalResults')
      if [ "$n" = "0" ]; then
        ${pkgs.gnused}/bin/sed "s/PENDING_UUID/$pending/g; s/RECEIVER_UUID/$receiver/g" "$acp" > "$STATE_DIRECTORY/$name.json"
        ${kanidm} raw create "$STATE_DIRECTORY/$name.json" >/dev/null
        echo "created access control $name"
      fi
    done
  '';

  # The builtin idm_acp_people_create / idm_acp_people_credential_reset, with
  # our receiver and, for modify and search, a scope narrowed to pending.
  scope =
    extra:
    builtins.toJSON {
      "and" = [
        {
          eq = [
            "class"
            "person"
          ];
        }
        {
          eq = [
            "class"
            "account"
          ];
        }
      ]
      ++ extra
      ++ [
        {
          andnot = {
            "or" = [
              {
                eq = [
                  "class"
                  "recycled"
                ];
              }
              {
                eq = [
                  "class"
                  "tombstone"
                ];
              }
            ];
          };
        }
      ];
    };
  credAttrs = [
    "account_expire"
    "account_softlock_expire"
    "account_valid_from"
    "attested_passkeys"
    "passkeys"
    "primary_credential"
  ];
  acpCreate = pkgs.writeText "dd_acp_signup_create.json" (
    builtins.toJSON {
      class = [
        "object"
        "access_control_profile"
        "access_control_create"
        "access_control_receiver_group"
        "access_control_target_scope"
      ];
      name = [ "dd_acp_signup_create" ];
      description = [ "signup service: create persons" ];
      acp_receiver_group = [ "RECEIVER_UUID" ];
      acp_targetscope = [ (scope [ ]) ];
      acp_create_class = [
        "account"
        "object"
        "person"
      ];
      acp_create_attr = [
        "account_expire"
        "class"
        "displayname"
        "mail"
        "name"
        "uuid"
      ];
    }
  );
  acpModify = pkgs.writeText "dd_acp_signup_pending_modify.json" (
    builtins.toJSON {
      class = [
        "object"
        "access_control_profile"
        "access_control_modify"
        "access_control_search"
        "access_control_receiver_group"
        "access_control_target_scope"
      ];
      name = [ "dd_acp_signup_pending_modify" ];
      description = [ "signup service: credentials and expiry of pending persons only" ];
      acp_receiver_group = [ "RECEIVER_UUID" ];
      acp_targetscope = [
        (scope [
          {
            eq = [
              "memberof"
              "PENDING_UUID"
            ];
          }
        ])
      ];
      acp_modify_presentattr = credAttrs;
      acp_modify_removedattr = credAttrs;
      acp_search_attr = credAttrs ++ [
        "class"
        "mail"
        "memberof"
        "name"
        "spn"
        "uuid"
      ];
    }
  );
  acpSearch = pkgs.writeText "dd_acp_signup_pending_search.json" (
    builtins.toJSON {
      class = [
        "object"
        "access_control_profile"
        "access_control_search"
        "access_control_receiver_group"
        "access_control_target_scope"
      ];
      name = [ "dd_acp_signup_pending_search" ];
      description = [ "signup service: read the pending group" ];
      acp_receiver_group = [ "RECEIVER_UUID" ];
      acp_targetscope = [
        (builtins.toJSON {
          "and" = [
            {
              eq = [
                "class"
                "group"
              ];
            }
            {
              eq = [
                "uuid"
                "PENDING_UUID"
              ];
            }
          ];
        })
      ];
      acp_search_attr = [
        "class"
        "member"
        "name"
        "uuid"
      ];
    }
  );
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
    path = [
      pkgs.coreutils
      pkgs.jq
      pkgs.gnused
    ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      StateDirectory = "kanidm-service-accounts"; # HOME for the cli token cache
      StateDirectoryMode = "0700";
      ExecStart = converge;
    };
  };
}
