{ config, pkgs, lib, ... }:
let
  base = "distributed-datacenter.duckdns.org";
  d = sub: "${sub}.${base}";
  entePorts = map d [ "api" "accounts" "albums" "cast" "photos" ];

  # Locker is a fourth ente app - notes, credentials, physical records and
  # documents - sharing the account and museum that photos already uses. The
  # nixos module only knows about accounts/albums/cast/photos, so it is built
  # and served here with the same overrides the module applies to those.
  lockerPkg = pkgs.ente-web.override {
    enteApp = "locker";
    enteMainUrl = "https://${d "photos"}";
    extraBuildEnv = {
      NEXT_PUBLIC_ENTE_ENDPOINT = "https://${d "api"}";
      NEXT_PUBLIC_ENTE_ALBUMS_ENDPOINT = "https://${d "albums"}";
      NEXT_TELEMETRY_DISABLED = "1";
    };
  };
in
{
  # One wildcard cert for every subdomain. DNS-01 needs no inbound ports,
  # which is why nothing here opens 80/443 - tailscale0 is already trusted.
  security.acme = {
    acceptTerms = true;
    defaults.email = "davidsprojects7@gmail.com";
    certs.${base} = {
      domain = "*.${base}";
      dnsProvider = "duckdns";
      environmentFile = config.sops.templates."duckdns.env".path;
      group = "nginx";
    };
  };

  services.ente = {
    web = {
      enable = true;
      domains = {
        accounts = d "accounts";
        albums = d "albums";
        cast = d "cast";
        photos = d "photos";
      };
    };
    api = {
      enable = true;
      nginx.enable = true;
      enableLocalDB = true; # peer auth over a socket, so no db password exists
      domain = d "api";
      settings = {
        s3 = {
          use_path_style_urls = true;
          b2-eu-cen = {
            endpoint = "https://${d "s3"}";
            region = "us-east-1"; # required internally by ente regardless of reality
            bucket = "ente";
            key._secret = config.sops.secrets.garage-key-id.path;
            secret._secret = config.sops.secrets.garage-key-secret.path;
          };
        };
        key = {
          encryption._secret = config.sops.secrets.ente-key-encryption.path;
          hash._secret = config.sops.secrets.ente-key-hash.path;
        };
        jwt.secret._secret = config.sops.secrets.ente-jwt-secret.path;
        # gomail does STARTTLS on its own unless SSL is set, so 587/tls rather
        # than 465/ssl. Gmail rewrites From to the authenticated account, so
        # the sender address cannot be one of our own subdomains.
        smtp = {
          host = "smtp.gmail.com";
          port = 587;
          encryption = "tls";
          username = "distributed.datacenter@gmail.com";
          email = "distributed.datacenter@gmail.com";
          sender-name = "Ente";
          password._secret = config.sops.secrets.ente-smtp-password.path;
        };
      };
    };
  };

  services.nginx = {
    enable = true;
    recommendedProxySettings = true;
    virtualHosts =
      lib.genAttrs entePorts (_: {
        useACMEHost = base;
        forceSSL = true;
      })
      // {
        ${d "locker"} = {
          useACMEHost = base;
          forceSSL = true;
          locations."/" = {
            root = lockerPkg;
            tryFiles = "$uri $uri.html /index.html";
          };
        };

        ${d "s3"} = {
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
      };
  };
}
