{
  config,
  pkgs,
  lib,
  ...
}:
let
  base = config.dd.domain;
in
{
  services.garage = {
    enable = true;
    package = pkgs.garage; # no default; nixpkgs ships several majors
    environmentFile = config.sops.templates."garage.env".path;
    settings = {
      metadata_dir = "/var/lib/garage/meta";
      data_dir = "/vault/photos"; # recordsize=1M, auto-snapshot, already declared
      db_engine = "lmdb";
      replication_factor = 1; # single vdev, single node
      rpc_bind_addr = "[::1]:3901";
      rpc_public_addr = "[::1]:3901";
      s3_api = {
        s3_region = "us-east-1"; # ente requires this string regardless of reality
        api_bind_addr = "127.0.0.1:3900";
        root_domain = ".s3.${base}";
      };
    };
  };

  # The module defaults to DynamicUser, which allocates a UID at runtime - so
  # there is no stable owner for data on /vault, and garage cannot write there.
  # A static user is the normal answer for persistent data on an external path.
  users.users.garage = {
    isSystemUser = true;
    group = "garage";
    home = "/var/lib/garage";
  };
  users.groups.garage = { };

  systemd.services.garage.serviceConfig = {
    DynamicUser = false;
    User = "garage";
    Group = "garage";
  };

  systemd.tmpfiles.rules = [
    "d /vault/photos 0750 garage garage -"
    "d /var/lib/garage 0750 garage garage -"
    "d /var/lib/garage/meta 0700 garage garage -"
  ];

  # Converging, like modules/zfs-datasets.nix: safe to re-run on every rebuild.
  # The access key is IMPORTED from sops rather than minted here, so ente can be
  # configured with the same credentials at deploy time.
  systemd.services.garage-setup = {
    description = "Converge garage layout, bucket and access key";
    after = [ "garage.service" ];
    requires = [ "garage.service" ];
    wantedBy = [ "multi-user.target" ];
    path = [
      config.services.garage.package
      pkgs.awscli2
      pkgs.coreutils
      pkgs.gnugrep
    ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      EnvironmentFile = [
        config.sops.templates."garage.env".path
        config.sops.templates."garage-key.env".path
      ];
    };
    script = ''
      for i in $(seq 1 60); do garage status >/dev/null 2>&1 && break; sleep 2; done

      NODE=$(garage node id -q | cut -d@ -f1)
      if ! garage layout show 2>/dev/null | grep -q "$NODE"; then
        garage layout assign -z home -c 3T "$NODE"
      fi
      # garage prints the version to apply; computing it ourselves gets
      # "Invalid new layout version"
      VER=$(garage layout show 2>/dev/null | grep -oE -- "--version [0-9]+" | grep -oE "[0-9]+" | head -1)
      if [ -n "$VER" ]; then
        garage layout apply --version "$VER"
      fi

      garage bucket create ente 2>/dev/null || true
      garage key import "$GARAGE_KEY_ID" "$GARAGE_KEY_SECRET" --yes -n ente 2>/dev/null || true
      garage bucket allow --read --write --owner ente --key "$GARAGE_KEY_ID" 2>/dev/null || true

      # The encrypted media tier. No CORS here: nothing browser-facing touches
      # it, rclone is a server-side client.
      garage bucket create media 2>/dev/null || true
      garage key import "$GARAGE_MEDIA_KEY_ID" "$GARAGE_MEDIA_KEY_SECRET" --yes -n media 2>/dev/null || true
      garage bucket allow --read --write --owner media --key "$GARAGE_MEDIA_KEY_ID" 2>/dev/null || true


      # Browsers upload blobs straight to garage, so the bucket needs CORS or
      # every upload fails the preflight with "This CORS request is not
      # allowed". garage has no CLI for this - it is an S3 API call.
      export AWS_ACCESS_KEY_ID="$GARAGE_KEY_ID"
      export AWS_SECRET_ACCESS_KEY="$GARAGE_KEY_SECRET"
      export AWS_DEFAULT_REGION=us-east-1
      aws --endpoint-url http://127.0.0.1:3900 s3api put-bucket-cors \
        --bucket ente --cors-configuration '${
          builtins.toJSON {
            CORSRules = [
              {
                # Must be "*". Ente decrypts in a web worker, which is a sandboxed
                # context, so the browser sends Origin: null - it cannot send the
                # photos origin even in principle. Listing real origins also makes
                # garage emit them comma-separated, which is invalid and silently
                # rejected. CORS is not the access control here: the presigned URL
                # signature is, and the objects are ciphertext regardless.
                AllowedOrigins = [ "*" ];
                AllowedMethods = [
                  "GET"
                  "PUT"
                  "POST"
                  "DELETE"
                  "HEAD"
                ];
                AllowedHeaders = [ "*" ];
                ExposeHeaders = [
                  "etag"
                  "ETag"
                  "x-amz-request-id"
                ];
                MaxAgeSeconds = 3000;
              }
            ];
          }
        }' 2>/dev/null || true
    '';
  };

  # garage's public face. Lives here rather than in ente.nix, where it ended up
  # only because ente needed it first; every future s3 consumer wants it too.
  services.nginx.virtualHosts."s3.${base}" = {
    useACMEHost = base;
    forceSSL = true;
    locations."/" = {
      proxyPass = "http://127.0.0.1:3900";
      extraConfig = ''
        client_max_body_size 0;
        proxy_request_buffering off;
      '';
    };
  };
}
