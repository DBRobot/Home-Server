{ config, ... }:
let
  base = config.dd.domain;
  host = "llm.${base}";
  idm = "https://idm.${base}";
in
{
  # Bearer-only gateway in front of llama-server. No browser flow: clients
  # present a kanidm access token and oauth2-proxy verifies the ES256
  # signature against the jwks kanidm publishes. Nothing here can mint a
  # token, and revoking the session in kanidm ends access.
  #
  # This replaces litellm, whose jwt auth turned out to be a paid feature -
  # "JWT Auth is an enterprise only feature".
  services.oauth2-proxy = {
    enable = true;
    provider = "oidc";
    oidcIssuerUrl = "${idm}/oauth2/openid/llm";
    clientID = "llm";
    clientSecretFile = config.sops.secrets.llm-oauth-secret.path;
    keyFile = config.sops.templates."oauth2-proxy.env".path;
    httpAddress = "http://127.0.0.1:4180";
    email.domains = [ "*" ];
    setXauthrequest = true;
    extraConfig = {
      # accept verified bearer JWTs instead of redirecting to a login page
      skip-jwt-bearer-tokens = "true";
      # 403 rather than a redirect: this fronts an api, and a client that
      # gets a login page instead of an error is a worse failure
      bearer-token-login-fallback = "false";
      oidc-jwks-url = "${idm}/oauth2/openid/llm/public_key.jwk";
      # the tokens actually presented are minted for the dd cli, so their
      # audience is dd, not llm. issuer=audience.
      extra-jwt-issuers = "${idm}/oauth2/openid/dd=dd";
      # nothing is proxied by oauth2-proxy itself; nginx does that after
      # auth_request returns 202
      upstream = "static://202";
      reverse-proxy = "true";
      # without this every connecting ip is trusted to supply X-Forwarded-*,
      # which oauth2-proxy warns about. nginx on this host is the only
      # legitimate source.
      trusted-proxy-ip = "127.0.0.1/32";
      skip-provider-button = "true";
      # kanidm enforces PKCE on every client and oauth2-proxy does not send a
      # challenge unless asked: "No PKCE code challenge was provided with
      # client in enforced PKCE mode". Bearer requests never reach the
      # authorise step, so this only matters for the browser login.
      code-challenge-method = "S256";
      # Browser sessions live in a cookie and are otherwise trusted until they
      # expire. Refreshing every five minutes makes oauth2-proxy go back to
      # kanidm with the refresh token, and a session revoked there fails that
      # refresh - "fatal refresh error, clearing session" - so a browser is
      # out within five minutes. Bearer clients cannot be helped this way:
      # oauth2-proxy never validates a bearer session, and kanidm's access
      # token is a hard-coded fifteen minutes. That is the bound for dd.
      cookie-refresh = "5m";
      # No oidc-email-claim or profile-url here on purpose. Clients present
      # an ID token (see modules/webdav-media.nix), which already carries
      # `email` and `preferred_username`, and the bearer path never calls the
      # profile url anyway - providers/oidc.go says so in as many words.
    };
  };

  services.nginx.virtualHosts.${host} = {
    useACMEHost = base;
    forceSSL = true;

    locations."/" = {
      proxyPass = "http://127.0.0.1:8081";
      extraConfig = ''
        auth_request /oauth2/auth;
        # A client with a bearer token (dd, curl, an editor) is answered by
        # auth_request alone. A browser has no token and got a bare 401 with
        # nowhere to go, so llama-server's own chat ui was unreachable by
        # anyone. Send the no-credential case into oauth2-proxy's login
        # instead: it runs the kanidm code flow, sets a cookie, and every later
        # auth_request passes on that cookie. bearer-token-login-fallback=false
        # is unaffected - that governs a token that is PRESENT and bad.
        error_page 401 = @login;
        proxy_buffering off; # streamed completions
        proxy_read_timeout 600s; # cpu generation is slow
        client_max_body_size 0;
      '';
    };

    # @login is defined once for every browser-facing vhost in modules/verify.nix:
    # the box's own passkey page, not oauth2-proxy's.

    # the subrequest nginx makes for every request above
    # The verifier first (modules/verify.nix): a user-signed biscuit is checked
    # against the device keys on file, anything else is relayed to
    # oauth2-proxy. Same variables come out either way.
    locations."= /oauth2/auth" = {
      proxyPass = "http://127.0.0.1:4181/verify";
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
}
