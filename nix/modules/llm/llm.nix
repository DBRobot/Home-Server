{ config, pkgs, ... }:
let
  base = config.dd.domain;
  host = "llm.${base}";
  # nginx asks the verifier who is calling; llama-server has no idea and
  # would answer anyone who reached 127.0.0.1:8081. This box runs CI jobs
  # and game guests, and both reach loopback, so "on this machine" is not
  # an identity here. A secret made at boot, in a file each side reads,
  # makes nginx the only caller llama-server will answer.
  run = "/run/dd-llm";
in
{
  # reads plaintext: only on a box whose owner is trusted with it (modules/box.nix)
  dd.box.plaintext = [ "llm (prompts and answers)" ];
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
        # the caller's own bearer never reaches llama-server: it is replaced
        # by the one llama-server was started with
        include ${run}/proxy.conf;
        # a browser with no credential gets the box's passkey page (@login is
        # defined for every browser-facing vhost in modules/verify.nix)
        error_page 401 = @login;
        error_page 403 = @waiting;
        # the demo's prompts are counted at the gate (rate:N on the tile)
        proxy_buffering off; # streamed completions
        proxy_read_timeout 600s; # cpu generation is slow
        client_max_body_size 0;
      '';
    };
  };

  # the secret, before either side that needs it. llama-cpp runs under a
  # DynamicUser, so the key reaches it as an environment file systemd reads
  # as root rather than as a file it would have to be given a group for.
  systemd.services.dd-llm-key = {
    description = "the key nginx proves itself to llama-server with";
    wantedBy = [ "multi-user.target" ];
    before = [
      "nginx.service"
      "llama-cpp.service"
    ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      UMask = "0077";
    };
    script = ''
      install -d -m 0751 ${run}
      key=$(${pkgs.openssl}/bin/openssl rand -hex 32)
      printf 'LLAMA_API_KEY=%s\n' "$key" > ${run}/env
      printf 'proxy_set_header Authorization "Bearer %s";\n' "$key" > ${run}/proxy.conf
      chgrp nginx ${run}/proxy.conf && chmod 0640 ${run}/proxy.conf
    '';
  };
  systemd.services.llama-cpp = {
    after = [ "dd-llm-key.service" ];
    requires = [ "dd-llm-key.service" ];
    serviceConfig.EnvironmentFile = "${run}/env";
  };
  systemd.services.nginx.after = [ "dd-llm-key.service" ];
  # nginx refuses to start on an include it cannot open, and nginx is the
  # whole box's front door. The file exists from boot whatever the key unit
  # does, so a failure here costs the llm and nothing else.
  systemd.tmpfiles.rules = [
    "d ${run} 0751 root root -"
    "f ${run}/proxy.conf 0640 root nginx -"
  ];
}
