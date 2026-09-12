{ config, ... }:
let
  base = config.dd.domain;
  host = "files.${base}";
  root = "/srv/users";
  images = "/srv/images";

  # One dav block, parameterised on where it writes. Both locations share the
  # auth and the $dav_dir whitelist; they differ only in the root - and in what
  # reads the result afterwards. /srv/users is media, so jellyfin has an acl on
  # it. /srv/images is per-user archives that arrive already encrypted (rclone
  # crypt on the client), so nothing on this host can read them and nothing
  # gets an acl. See modules/user-accounts.nix for the directories.
  dav = dir: listing: ''
    auth_request /oauth2/auth;
    # NOT x_auth_request_user: oauth2-proxy fills that from the `sub`
    # claim and there is no flag to change it - providers/provider_data.go
    # hardcodes UserClaim to "sub". It is the account uuid, so nginx built
    # /srv/users/bb02d34e-... and every PUT landed nowhere. The
    # preferred-username header is the same identity as a name.
    auth_request_set $dav_user $upstream_http_x_auth_request_preferred_username;
    # $dav_dir is $dav_user filtered through the map below, so a missing
    # or malformed header cannot name a directory
    alias ${dir}/$dav_dir/;

    dav_methods PUT DELETE MKCOL COPY MOVE;
    dav_ext_methods PROPFIND OPTIONS;
    dav_access user:rw group:rw;
    create_full_put_path on;

    client_max_body_size 0; # movies, disk images
    client_body_timeout 600s;
    send_timeout 600s;
    autoindex on;
    # html for a person in a browser; json for `dd image`, which lists a
    # restic repository's type directories this way rather than parsing
    # PROPFIND xml
    autoindex_format ${listing};
  '';
in
{
  # Authenticated upload with no new service: nginx already ships the dav
  # modules, and oauth2-proxy from modules/llm-auth.nix already validates
  # kanidm tokens. This is the only way files reach /srv/users now - samba was
  # retired with modules/smb-media.nix, having never carried a file.
  #
  # Present the ID TOKEN here, not the access token. oauth2-proxy's bearer
  # mode is built for id tokens - providers/oidc.go names the function
  # "CreateSessionFromToken converts Bearer IDTokens into sessions" and
  # explicitly skips the profile url in that path ("we can't hit the
  # ProfileURL"). Kanidm's access token carries nothing but `sub`, so an
  # access token authenticates fine and then has no name to build a path
  # from. Verified: access token 500, id token 201.
  #
  # Not mountable in Finder or Explorer: those speak basic auth only and
  # cannot present a bearer token. Use rclone or `dd upload`.
  services.nginx.virtualHosts.${host} = {
    useACMEHost = base;
    forceSSL = true;

    locations."/".extraConfig = dav root "html";
    # Verified with rclone crypt -> chunker -> webdav: an unknown-size stream
    # (`dd if=/dev/sdX | zstd | rclone rcat`) arrives as fixed-size chunk PUTs,
    # so no scratch copy of the image is ever needed on the client.
    locations."/images/".extraConfig = dav images "json";

    locations."= /oauth2/auth" = {
      proxyPass = "http://127.0.0.1:4180";
      extraConfig = ''
        internal;
        proxy_pass_request_body off;
        proxy_set_header Content-Length "";
        proxy_set_header X-Original-URI $request_uri;
        # The subrequest inherits nothing from the dav location, so it ran with
        # nginx's default 1m limit. When the body was still arriving as the
        # auth phase ran, discarding it tripped that limit: "auth request
        # unexpected status: 413", surfaced to the client as a 500. Small
        # uploads passed only because they had already fully arrived. The
        # limit belongs on the dav location; here it must not exist.
        client_max_body_size 0;
      '';
    };

    locations."/oauth2/" = {
      proxyPass = "http://127.0.0.1:4180";
      extraConfig = ''
        proxy_set_header X-Scheme $scheme;
      '';
    };
  };

  # An empty $dav_user would make the alias /srv/users/ - the root itself,
  # shared by everyone - and create_full_put_path would happily mkdir in it.
  # Kanidm will not issue a name with a slash in it, but the directory name
  # here comes from a header, so it gets whitelisted rather than trusted.
  # This is a map and not `if ($dav_user = "")` because if runs in the rewrite
  # phase, before auth_request has set the variable: the test would always see
  # an empty string. Map variables are evaluated where they are used.
  services.nginx.appendHttpConfig = ''
    map $dav_user $dav_dir {
      # __denied__ is a real 0555 root-owned directory created by
      # user-accounts in modules/user-accounts.nix. It exists so a request that
      # got past auth with no usable username lands somewhere provably
      # unwritable instead of somewhere nginx might create. The response is
      # still a 500, not a 403: nginx's dav module maps the EACCES from
      # open() to 500 and no permission arrangement changes that. Masking it
      # with error_page would also swallow real server errors here, so it is
      # left honest and ugly.
      default            "__denied__";
      "~^[a-zA-Z0-9._-]+$" $dav_user;
    }
  '';

  # nginx's unit is sandboxed with a read-only /srv, so every PUT failed
  # with "mkdir() ... (30: Read-only file system)" despite correct auth.
  systemd.services.nginx.serviceConfig.ReadWritePaths = [
    root
    images
  ];

  # nginx writes the upload here before moving it into place; without a
  # temp path on the same filesystem every PUT is a cross-device copy
  systemd.tmpfiles.rules = [
    "d /srv/upload-tmp 0700 nginx nginx -"
  ];
}
