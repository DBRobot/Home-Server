{ pkgs, ... }:
let
  to = "davidsprojects7@gmail.com";
in
{
  # Everything here ends at an email. metrics.nix records; this interrupts.
  # alertmanager belongs here too when it arrives.
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
