{
  ddScript,
  config,
  pkgs,
  ...
}:
let
  base = config.dd.domain;
  host = "jellyfin.${base}";
  library = "/srv/media";

  # Not in nixpkgs, and jellyfin has no plugin option, so it is fetched and
  # dropped into the plugin dir. The upstream repo is ARCHIVED - it targets
  # abi 10.11.0.0 which matches jellyfin 10.11.11 today, but a future
  # jellyfin bump may strand it and there is no maintained replacement.
  ssoPlugin = pkgs.stdenvNoCC.mkDerivation {
    pname = "jellyfin-plugin-sso";
    version = "4.0.0.3";
    src = pkgs.fetchurl {
      url = "https://github.com/9p4/jellyfin-plugin-sso/releases/download/v4.0.0.3/sso-authentication_4.0.0.3.zip";
      hash = "sha256-3glRJVvsTtZGA3ZB5+CqEhCzoAoUFAZUgIe+2ZTLm90=";
    };
    nativeBuildInputs = [ pkgs.unzip ];
    unpackPhase = "unzip $src -d .";
    installPhase = "mkdir -p $out && cp *.dll meta.json $out/";
  };
in
{
  # what jellyfin reads is the demo's library: films that are nobody's, in
  # a plain folder. A member's media never comes here (client/media)
  dd.box.plaintext = [ "jellyfin (the demo's films)" ];
  services.jellyfin = {
    enable = true;
    group = "media";
  };
  users.users.media = {
    isSystemUser = true;
    group = "media";
  };
  users.groups.media = { };
  # the demo's library; jellyfin's own config names it, so the path stays
  systemd.tmpfiles.rules = [ "d ${library} 2775 media media -" ];

  # Tiger Lake iris xe does several 4k transcodes at once, but only with the
  # VA-API driver present. Without it jellyfin silently falls back to software
  # and pegs all 8 cores on a single stream.
  hardware.graphics = {
    enable = true;
    extraPackages = with pkgs; [
      intel-media-driver
      vpl-gpu-rt
    ];
  };

  # A symlink into the store does not work: jellyfin rewrites meta.json when
  # it loads a plugin, so the directory has to be a real writable copy. With
  # the symlink it loaded the assemblies and then threw
  # UnauthorizedAccessException from SaveManifest, leaving the endpoint 503.
  systemd.services.jellyfin-plugins = {
    description = "Install jellyfin plugins from the store into its data dir";
    before = [ "jellyfin.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      dir=${config.services.jellyfin.dataDir}/plugins/SSO-Auth_4.0.0.3
      rm -rf "$dir"
      mkdir -p "$dir"
      cp ${ssoPlugin}/* "$dir"/
      chown -R jellyfin:${config.services.jellyfin.group} "$dir"
      chmod -R u+w "$dir"
    '';
  };

  users.users.jellyfin.extraGroups = [
    "render" # /dev/dri/renderD128
    "video"
  ];

  systemd.services.jellyfin = {
    # the sso provider's endpoint carries the fleet's name; a new name is
    # written by the seed and read at start
    restartTriggers = [ base ];
  };

  # The plugin's own config is xml in jellyfin's data dir, not nix. This
  # writes it once, from sops, and then leaves it alone: the same file holds
  # CanonicalLinks - the sso-identity to jellyfin-user mapping - so
  # overwriting on every rebuild would unlink every account.
  #
  # This structure was copied from what jellyfin itself wrote after the
  # provider was added once through the plugin's page - not derived. Reading
  # SerializableDictionary.WriteXml got item/key/value right but the value
  # element is <PluginConfiguration>, not <OidConfig>, and the field is
  # FolderRoleMappings, not FolderRoleMapping. A wrong name here is ignored
  # silently: jellyfin falls back to defaults and persists them over this.
  systemd.services.jellyfin-sso-config = {
    description = "Seed the jellyfin sso provider config";
    # Before jellyfin, not after. The first version ended with
    # "systemctl restart jellyfin" while declaring Requires=jellyfin.service,
    # so restarting jellyfin tore this unit down mid-write; the retry then
    # matched its own half-written file and skipped, and jellyfin persisted
    # its empty in-memory config over the remains.
    before = [ "jellyfin.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ddScript ./jellyfin-sso.sh {
      BASE = base;
      DATA_DIR = config.services.jellyfin.dataDir;
      GROUP = config.services.jellyfin.group;
      OAUTH_SECRET_FILE = config.sops.secrets.jellyfin-oauth-secret.path;
      GREP = pkgs.gnugrep;
      SED = pkgs.gnused;
    };
  };

  services.nginx.virtualHosts.${host} = {
    useACMEHost = base;
    forceSSL = true;
    locations."/" = {
      proxyPass = "http://127.0.0.1:8096";
      proxyWebsockets = true;
      extraConfig = ''
        # jellyfin has its own sign-in (oidc, from this box) and never
        # needs a member's device token. It used to receive one anyway,
        # on every request, and this is the box's most exposed service.
        proxy_set_header Authorization "";
        proxy_set_header Cookie $dd_cookie_stripped;
        client_max_body_size 0;
        proxy_buffering off; # streams, not pages
      '';
    };
  };
}
