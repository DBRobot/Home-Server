{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.dd.forgejo;
  port = 3001;
  # From this box, forgejo takes the reverse-proxy header at face value
  # (127.0.0.0/8 is its trusted proxy), so the admin's name is the whole
  # credential here. No forgejo token to mint or keep.
  api = "http://127.0.0.1:${toString port}/api/v1";
in
{
  # Repositories mirrored out to GitHub after every push, so the public
  # copy there stays current once the forge is the source of truth.
  options.dd.forgejo.mirrors = lib.mkOption {
    type = lib.types.listOf (
      lib.types.submodule {
        options = {
          repo = lib.mkOption {
            type = lib.types.str;
            example = "david/Home-Server";
          };
          to = lib.mkOption {
            type = lib.types.str;
            example = "https://github.com/DBRobot/Home-Server.git";
          };
          user = lib.mkOption {
            type = lib.types.str;
            description = "GitHub login the token belongs to.";
          };
        };
      }
    );
    default = [ ];
  };

  config = lib.mkIf (cfg.mirrors != [ ]) {
    # a fine-grained GitHub token with contents read/write on the mirrored
    # repositories; forgejo keeps its own encrypted copy once told
    sops.secrets.github-mirror-token = { };

    systemd.services.forgejo-mirrors = {
      description = "Make sure every declared push mirror exists";
      after = [ "forgejo-admin.service" ];
      requires = [ "forgejo.service" ];
      wantedBy = [ "multi-user.target" ];
      path = [
        pkgs.curl
        pkgs.jq
      ];
      serviceConfig = {
        Type = "oneshot";
        LoadCredential = "token:${config.sops.secrets.github-mirror-token.path}";
      };
      script = ''
        set -euo pipefail
        token=$(cat "$CREDENTIALS_DIRECTORY/token")
        as_admin() { curl -fsS -H 'X-WEBAUTH-USER: ${cfg.admin}' "$@"; }
        ${lib.concatMapStringsSep "\n" (m: ''
          if as_admin ${api}/repos/${m.repo}/push_mirrors | jq -e '.[] | select(.remote_address == "${m.to}")' >/dev/null; then
            echo "${m.repo} -> ${m.to}: present"
          else
            jq -n --arg addr "${m.to}" --arg user "${m.user}" --arg pw "$token" \
              '{remote_address: $addr, remote_username: $user, remote_password: $pw, interval: "8h0m0s", sync_on_commit: true}' \
              | as_admin -X POST -H 'content-type: application/json' -d @- ${api}/repos/${m.repo}/push_mirrors >/dev/null
            as_admin -X POST ${api}/repos/${m.repo}/push_mirrors-sync >/dev/null
            echo "${m.repo} -> ${m.to}: added and first sync requested"
          fi
        '') cfg.mirrors}
      '';
    };
  };
}
