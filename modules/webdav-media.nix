{ ... }:
let
  base = "distributed-datacenter.duckdns.org";
  host = "files.${base}";
  root = "/srv/users";
in
{
  # Authenticated upload with no new service: nginx already ships the dav
  # modules, and oauth2-proxy from modules/llm-auth.nix already validates
  # kanidm tokens. The same directories and acls samba writes to, so a file
  # arriving either way is indistinguishable to jellyfin.
  #
  # Not mountable in Finder or Explorer: those speak basic auth only and
  # cannot present a bearer token. Use rclone or `dd upload`.
  services.nginx.virtualHosts.${host} = {
    useACMEHost = base;
    forceSSL = true;

    locations."/" = {
      extraConfig = ''
        auth_request /oauth2/auth;
        # oauth2-proxy hands back the identity from the token; the upload is
        # scoped to that user's directory rather than a path they choose
        auth_request_set $dav_user $upstream_http_x_auth_request_user;
        alias ${root}/$dav_user/;

        dav_methods PUT DELETE MKCOL COPY MOVE;
        dav_ext_methods PROPFIND OPTIONS;
        dav_access user:rw group:rw;
        create_full_put_path on;

        client_max_body_size 0; # movies
        client_body_timeout 600s;
        send_timeout 600s;
        autoindex on;
      '';
    };

    locations."= /oauth2/auth" = {
      proxyPass = "http://127.0.0.1:4180";
      extraConfig = ''
        internal;
        proxy_pass_request_body off;
        proxy_set_header Content-Length "";
        proxy_set_header X-Original-URI $request_uri;
      '';
    };

    locations."/oauth2/" = {
      proxyPass = "http://127.0.0.1:4180";
      extraConfig = ''
        proxy_set_header X-Scheme $scheme;
      '';
    };
  };

  # nginx's unit is sandboxed with a read-only /srv, so every PUT failed
  # with "mkdir() ... (30: Read-only file system)" despite correct auth.
  systemd.services.nginx.serviceConfig.ReadWritePaths = [ root ];

  # nginx writes the upload here before moving it into place; without a
  # temp path on the same filesystem every PUT is a cross-device copy
  systemd.tmpfiles.rules = [
    "d /srv/upload-tmp 0700 nginx nginx -"
  ];
}
