{ config, pkgs, lib, ... }:
let
  to = "davidsprojects7@gmail.com";
in
{
  # Outbound mail for the machine, through the same relay ente uses. Lives here
  # rather than in a service module because several things need to shout.
  programs.msmtp = {
    enable = true;
    setSendmail = true; # zed and smartd both just want /usr/sbin/sendmail
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

  # zfs-zed was already running and writing pool events to a log nobody reads.
  # vault is a single vdev: it can DETECT bit rot but not repair it, so hearing
  # about a checksum error the day it happens is the entire value.
  services.zfs.zed = {
    enableMail = false; # we provide sendmail via msmtp, not the mail package
    settings = {
      ZED_EMAIL_ADDR = [ to ];
      ZED_EMAIL_PROG = "${pkgs.msmtp}/bin/msmtp";
      ZED_EMAIL_OPTS = "-a default @ADDRESS@";
      ZED_NOTIFY_VERBOSE = true; # also report scrub completions, not just faults
      ZED_NOTIFY_INTERVAL_SECS = 3600;
      ZED_USE_ENCLOSURE_LEDS = false; # usb dock, no backplane
    };
  };

  # SMART works through the VL817 bridge - verified: model reported, health
  # PASSED, attributes readable. Reallocated and pending sectors climbing is the
  # earliest warning a disk is going, which on a single vdev is the difference
  # between buying a drive and losing the pool.
  services.smartd = {
    enable = true;
    notifications.mail = {
      enable = true;
      sender = "distributed.datacenter@gmail.com";
      recipient = to;
      mailer = "${pkgs.msmtp}/bin/msmtp";
    };
    defaults.monitored = "-a -o on -S on -s (S/../.././02|L/../../6/03)";
    devices = [
      { device = "/dev/disk/by-id/ata-WDC_WD40EFZZ-68CPAN0_WD-WX42D46N1PEJ"; } # vault
      { device = "/dev/nvme0n1"; } # root + postgres
    ];
  };
}
