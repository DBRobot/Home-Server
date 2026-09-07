{ config, pkgs, lib, ... }:
let
  base = "distributed-datacenter.duckdns.org";
  host = "files.${base}";

  # Pinned by digest, not tag. The semver tags stop at v0.17.0 (April 2024);
  # master is the only channel that still moves, so the tag alone would drift
  # under us on every pull.
  image = "ghcr.io/peergos/web-ui@sha256:5063db00fd65ce0948dd7d1dd9d9e72af899d92bc04ece957f9589dcddf11c1c";

  # Upstream takes the S3 credentials as command line arguments, and oci-containers
  # puts cmd into a unit file in the world readable nix store. Build the argument
  # list inside the container instead, from an EnvironmentFile, so the secrets stay
  # in /run/secrets like everything else here.
  # Peergos picks its S3 addressing mode from the endpoint hostname:
  #   useHttps = ! host.endsWith("localhost") && ! host.contains("localhost:")
  #   folder   = (useHttps ? "" : bucket + "/")            (S3BlockStorage.java)
  # and builds the host as bucket + "." + endpoint. Pointing it at
  # "localhost:3900" therefore gives http and PATH style, which is what garage
  # wants. An https endpoint would give virtual host style against
  # peergos.s3.<base>, and the wildcard cert only covers one label deep.
  entry = pkgs.writeText "peergos-entry.sh" ''
    exec /opt/peergos/docker-entrypoint.sh daemon \
      -listen-host 127.0.0.1 \
      -port 8000 \
      -public-domain ${host} \
      -public-server true \
      -generate-token true \
      -useIPFS false \
      -s3.bucket peergos \
      -s3.region us-east-1 \
      -s3.region.endpoint localhost:3900 \
      -s3.accessKey "$PEERGOS_S3_KEY_ID" \
      -s3.secretKey "$PEERGOS_S3_KEY_SECRET" \
      -authed-s3-reads false \
      -direct-s3-writes false
  '';
in
{
  virtualisation.oci-containers.containers.peergos = {
    inherit image;
    # Host networking so the container reaches garage on 127.0.0.1:3900 without
    # publishing anything: peergos binds 127.0.0.1:8000 and nginx proxies to it.
    # 4001 is deliberately not opened - federation needs a routable address and
    # this host has no inbound ports at all.
    extraOptions = [ "--network=host" ];
    entrypoint = "/bin/sh";
    cmd = [ "/entry.sh" ];
    volumes = [
      "/var/lib/peergos:/var/lib/peergos"
      "${entry}:/entry.sh:ro"
    ];
    environment.PEERGOS_PATH = "/var/lib/peergos";
    environmentFiles = [ config.sops.templates."peergos.env".path ];
  };

  systemd.tmpfiles.rules = [ "d /var/lib/peergos 0700 root root -" ];

  # bucket + "." + endpoint, so peergos connects to peergos.localhost:3900.
  # Nothing resolves that by default.
  networking.hosts."127.0.0.1" = [ "peergos.localhost" ];

  systemd.services.podman-peergos = {
    after = [ "garage-setup.service" ];
    requires = [ "garage-setup.service" ];
  };

  services.nginx.virtualHosts.${host} = {
    useACMEHost = base;
    forceSSL = true;
    locations."/" = {
      proxyPass = "http://127.0.0.1:8000";
      proxyWebsockets = true;
      extraConfig = ''
        client_max_body_size 0;
        proxy_request_buffering off;
      '';
    };
  };
}
