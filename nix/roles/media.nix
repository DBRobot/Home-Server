{ config, ... }:
{
  # Media and people's files: jellyfin, the media tier, uploads, per-user
  # directories, the archive catalog. Plaintext, so owner-trusted only.
  imports = [
    ./_sops.nix
    ../modules/media/media-tier.nix
    ../modules/media/jellyfin.nix
    ../modules/media/archive-catalog.nix
    ../modules/gate/user-accounts.nix
    ../modules/media/webdav-media.nix
  ];
  users.users.admin.extraGroups = [ "media" ]; # copy files into /srv/media without sudo

  dd.home.services = [
    {
      name = "Videos";
      # straight into the sso plugin: jellyfin's own login page is for
      # nobody here, and a first-time person landing on it is lost
      url = "https://jellyfin.${config.dd.domain}/sso/OID/p/dd";
      # the demo is a jellyfin user like any other, made on first visit,
      # with jellyfin's defaults: watch, no admin. jellyfin is the limit
      demo = "full";
      description = "Movies and series, on any screen.";
      icon = "videos";
      color = "#b4457a";
      rank = 20;
    }
    {
      name = "Files";
      url = "https://files.${config.dd.domain}/";
      # a folder of samples; the gate refuses its writing methods
      demo = "read";
      description = "Your folder, from the browser or a file manager.";
      icon = "files";
      color = "#2f6fd6";
      rank = 30;
    }
  ];

  dd.backup.paths = [
    "/srv/users" # people's uploads
    "/srv/images" # archives of old computers, already ciphertext
    "/var/lib/jellyfin" # watch state, users, plugin config
  ];
  dd.backup.exclude = [
    "/var/lib/jellyfin/transcodes"
    "/var/lib/jellyfin/log"
    "/var/lib/jellyfin/cache"
  ];
  # the media tier's bucket and key; rclone encrypts before it writes
  sops.templates."garage-media-key.env".content = ''
    GARAGE_MEDIA_KEY_ID=''${config.sops.placeholder.garage-media-key-id}
    GARAGE_MEDIA_KEY_SECRET=''${config.sops.placeholder.garage-media-key-secret}
  '';
  dd.garage.setupEnvFiles = [ config.sops.templates."garage-media-key.env".path ];
  # No CORS here: nothing browser-facing touches it, rclone is a
  # server-side client.
  dd.garage.buckets.media = {
    key = {
      name = "media";
      envPrefix = "GARAGE_MEDIA_KEY";
    };
    allow = [
      "read"
      "write"
      "owner"
    ];
  };

  # read only through the rclone template, so root-only is fine
  sops.secrets = {
    garage-media-key-id = { };
    garage-media-key-secret = { };
    rclone-crypt-password = { };
    rclone-crypt-salt = { };
  };
  # rclone takes its whole config from the environment, so no config file
  # is written anywhere. PASSWORD/PASSWORD2 are rclone-obscured, which is
  # obfuscation not encryption - sops is what actually protects them.
  sops.templates."rclone.env".owner = "media";
  sops.templates."rclone.env".content = ''
    RCLONE_CONFIG_GARAGE_TYPE=s3
    RCLONE_CONFIG_GARAGE_PROVIDER=Other
    RCLONE_CONFIG_GARAGE_ENDPOINT=http://127.0.0.1:3900
    RCLONE_CONFIG_GARAGE_REGION=us-east-1
    RCLONE_CONFIG_GARAGE_ACCESS_KEY_ID=${config.sops.placeholder.garage-media-key-id}
    RCLONE_CONFIG_GARAGE_SECRET_ACCESS_KEY=${config.sops.placeholder.garage-media-key-secret}
    RCLONE_CONFIG_COLD_TYPE=crypt
    RCLONE_CONFIG_COLD_REMOTE=garage:media
    RCLONE_CONFIG_COLD_PASSWORD=${config.sops.placeholder.rclone-crypt-password}
    RCLONE_CONFIG_COLD_PASSWORD2=${config.sops.placeholder.rclone-crypt-salt}
  '';
}
