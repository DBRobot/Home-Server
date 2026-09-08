{ config, ... }:
let
  base = "distributed-datacenter.duckdns.org";
  host = "llm.${base}";
  user = "litellm";
in
{
  # Peer auth over the socket, same as ente and kanidm - no db password
  # exists. ensureDBOwnership needs the db name to match the role.
  services.postgresql = {
    ensureDatabases = [ user ];
    ensureUsers = [
      {
        name = user;
        ensureDBOwnership = true;
      }
    ];
  };

  services.litellm = {
    enable = true;
    host = "127.0.0.1";
    port = 4000;
    openFirewall = false; # nginx is the only way in
    environmentFile = config.sops.templates."litellm.env".path;
    settings = {
      model_list = [
        {
          model_name = "qwen3.6-35b";
          litellm_params = {
            # llama.cpp speaks the openai api, so it is just another provider
            model = "openai/qwen3.6-35b";
            api_base = "http://127.0.0.1:8081/v1";
            api_key = "none"; # llama-server is localhost-only now
          };
        }
      ];
      general_settings = {
        # Kanidm signs the tokens; litellm checks them against the jwks the
        # provider publishes, so no shared secret is involved in verification.
        master_key = "os.environ/LITELLM_MASTER_KEY";
      };
      litellm_settings = {
        drop_params = true;
        # No request or response logging: people's conversations are their
        # business, and the whole point of running this here is that nobody
        # else holds them.
        turn_off_message_logging = true;
      };
    };
  };

  services.nginx.virtualHosts.${host} = {
    useACMEHost = base;
    forceSSL = true;
    locations."/" = {
      proxyPass = "http://127.0.0.1:4000";
      proxyWebsockets = true;
      extraConfig = ''
        client_max_body_size 0;
        proxy_buffering off; # streamed completions
        proxy_read_timeout 600s; # generation can be slow on cpu
      '';
    };
  };
}
