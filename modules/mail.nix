{ config, pkgs, ... }:
{
  # Outbound mail for the machine, through the same relay ente uses. Its own
  # module because zed, smartd and the backup alerts all want it.
  programs.msmtp = {
    enable = true;
    setSendmail = true;
    # so every sender here can write "alerts" instead of a real address
    defaults.aliases = config.sops.templates."msmtp-aliases".path;
    accounts.default = {
      host = "smtp.gmail.com";
      port = 587;
      tls = true;
      tls_starttls = true;
      auth = "login";
      from = "distributed.datacenter@gmail.com";
      user = "distributed.datacenter@gmail.com";
      passwordeval = "${pkgs.coreutils}/bin/cat ${config.sops.secrets.ente-smtp-password.path}";
    };
  };
}
