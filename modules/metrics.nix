{ ... }:
{
  # Samples, not events: alerting.nix shouts when something breaks, this
  # notices the gradual things. Started before any dashboard exists because
  # history cannot be backfilled.
  services.prometheus = {
    enable = true;
    port = 9090;
    listenAddress = "127.0.0.1"; # tailnet only, via nginx if ever wanted
    retentionTime = "180d"; # a few hundred MB; enough to answer "when did this start"

    exporters.node = {
      enable = true;
      listenAddress = "127.0.0.1";
      port = 9100;
      enabledCollectors = [
        "systemd" # unit states: catches a failed backup even without the mail
        "zfs" # pool state, arc stats, per-dataset space
        "hwmon" # drive and cpu temperatures
        "textfile"
      ];
    };

    scrapeConfigs = [
      {
        job_name = "node";
        static_configs = [ { targets = [ "127.0.0.1:9100" ]; } ];
      }
      {
        # garage already exports; nothing to install
        job_name = "garage";
        static_configs = [ { targets = [ "127.0.0.1:2112" ]; } ];
      }
    ];
  };
}
