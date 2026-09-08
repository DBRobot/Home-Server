{ config, ... }:
let
  base = "distributed-datacenter.duckdns.org";
  host = "llm.${base}";
  user = "litellm";
in
{
  # Deliberately NO database. nixpkgs' litellm ships without the prisma
  # package at all, so any db-backed feature dies on `from prisma import
  # Prisma`. JWT auth does not need it - handle_jwt.py takes prisma_client as
  # Optional, because validation is against the provider's jwks, not a table.
  #
  # The cost is that there are no virtual keys: identity comes from a kanidm
  # token on every request rather than from a key litellm minted.

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
        master_key = "os.environ/LITELLM_MASTER_KEY";
        # NO jwt auth, and not by choice: litellm gates it behind a paid
        # licence - "JWT Auth is an enterprise only feature". Admin-UI SSO is
        # the same. So kanidm cannot front this without buying a licence, and
        # the GENERIC_* env vars below are inert until one exists.
        #
        # Until then the master key is the only credential. That is one shared
        # secret rather than per-user identity, which is worse than the design
        # we wanted - llama.cpp's own --api-key-file takes a list of keys and
        # would give per-user credentials without litellm at all.
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
