{ config, pkgs, ... }:
{
  # Looking at the fleet: grafana over every box's own prometheus, alerts,
  # the mail they go out by, and the database dumps.
  imports = [
    ./_sops.nix
    ../modules/grafana.nix
    ../modules/alerting.nix
    ../modules/mail.nix
    ../modules/postgres-backup.nix
  ];
  dd.home.services = [
    {
      name = "Metrics";
      url = "https://grafana.${config.dd.domain}/";
      description = "How the machines are doing.";
      icon = "metrics";
      color = "#c4562d";
      rank = 60;
    }
  ];

  dd.backup.paths = [
    "/var/lib/grafana" # dashboards people made, not the provisioned ones
    "/vault/backups" # the postgres dumps: the ente key hierarchy lives there
  ];
  environment.systemPackages = with pkgs; [
    vim
    git
    htop
  ];

  sops.secrets.grafana-secret-key.owner = "grafana";
  # Where machine mail actually goes. Read only through the msmtp aliases
  # template below, so root-only is right.
  sops.secrets.alert-recipient = { };
  # msmtp expands local names through this, so zed, smartd and the backup
  # alerts can all address "alerts" and the real destination stays here.
  # One place to change it, and nothing names a person in the repo.
  sops.templates."msmtp-aliases".content = ''
    alerts: ${config.sops.placeholder.alert-recipient}
    default: ${config.sops.placeholder.alert-recipient}
  '';
}
