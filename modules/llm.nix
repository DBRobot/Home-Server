{ config, ... }:
let
  base = config.dd.domain;
  host = "llm.${base}";
in
{
  # The gateway in front of llama-server: nginx asks the verifier who this is
  # (a device-signed token, or a passkey session on this box) and proxies.
  # Nothing here can mint a token.
  services.nginx.virtualHosts.${host} = {
    useACMEHost = base;
    forceSSL = true;

    locations."/" = {
      proxyPass = "http://127.0.0.1:8081";
      extraConfig = ''
        auth_request /_dd/verify;
        # a browser with no credential gets the box's passkey page (@login is
        # defined for every browser-facing vhost in modules/verify.nix)
        error_page 401 = @login;
        proxy_buffering off; # streamed completions
        proxy_read_timeout 600s; # cpu generation is slow
        client_max_body_size 0;
      '';
    };
  };
}
