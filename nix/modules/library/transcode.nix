# The compute side of the encrypted libraries: a member's device asks this
# box to play a file its own player cannot. The device brings the chunk
# urls and the file key sealed to this box's key; the box decrypts in
# memory, transcodes, streams, and wipes the session. Plaintext exists
# here only in RAM while working, which is the fleet's rule for compute.
{
  config,
  pkgs,
  lib,
  self,
  ...
}:
let
  cfg = config.dd.transcode;
  base = config.dd.domain;
  port = 4190;
in
{
  options.dd.transcode.enable = lib.mkEnableOption "transcoding one file at a time for the libraries' players";

  config = lib.mkIf cfg.enable {
    # a label, for honesty: prompts to llama.cpp are the same shape
    dd.box.plaintext = [ "transcode (a file in memory while it plays)" ];

    systemd.services.dd-transcode = {
      description = "Transcode one encrypted file for one viewer, in memory";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      environment = {
        TRANSCODE_BIND = "127.0.0.1:${toString port}";
        TRANSCODE_PLAIN_BIND = "127.0.0.1:${toString (port + 1)}";
        TRANSCODE_DIR = "/run/dd-transcode";
        TRANSCODE_FFMPEG = "${pkgs.ffmpeg-headless}/bin/ffmpeg";
      };
      serviceConfig = {
        Type = "simple";
        DynamicUser = true;
        # the sessions live in tmpfs and go with the unit
        RuntimeDirectory = "dd-transcode";
        RuntimeDirectoryMode = "0700";
        ExecStart = "${self.packages.${pkgs.stdenv.hostPlatform.system}.transcode}/bin/dd-transcode";
        Restart = "on-failure";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        MemoryMax = "4G";
      };
    };

    # reached through the gate's host, by a signed-in member only; the plain
    # listener (port + 1) is for ffmpeg and is proxied by nothing
    services.nginx.virtualHosts."files.${base}".locations."/_dd/transcode/" = {
      proxyPass = "http://127.0.0.1:${toString port}/";
      extraConfig = ''
        auth_request /_dd/verify;
        client_max_body_size 8m;
        proxy_read_timeout 120s;
      '';
    };
  };
}
