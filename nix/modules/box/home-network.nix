# The house network: a box on the router by cable when it has one, by wifi
# when it does not. Two NetworkManager profiles, ordered by route metric, so
# the best path that is up carries the default route and the other waits.
# Every box gets a fixed address on the house network (the router's
# reservation matches), since the port forward names it.
{
  config,
  lib,
  ...
}:
let
  cfg = config.dd.home;
in
{
  options.dd.home = {
    ssid = lib.mkOption {
      type = lib.types.str;
      description = "the house wifi";
    };
    pskFile = lib.mkOption {
      type = lib.types.path;
      description = "file holding the wifi password: `psk=<...>`, as NetworkManager's ensureProfiles reads secrets";
    };
    address = lib.mkOption {
      type = lib.types.str;
      description = "this box's fixed address on the house network, with prefix (192.168.1.20/24); the router reserves it";
    };
    gateway = lib.mkOption {
      type = lib.types.str;
      default = "192.168.1.1";
    };
    wired = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "the cabled adapter to the router, by mac address (an interface name would encode the usb port)";
    };
    wifi = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "the wifi interface, if any";
    };
  };

  config = {
    networking.networkmanager.ensureProfiles = {
      environmentFiles = [ cfg.pskFile ];
      profiles =
        lib.optionalAttrs (cfg.wired != null) {
          house-wired = {
            connection = {
              id = "house-wired";
              type = "ethernet";
              autoconnect = true;
              autoconnect-priority = 100;
            };
            ethernet.mac-address = cfg.wired;
            ipv4 = {
              method = "manual";
              address1 = cfg.address;
              gateway = cfg.gateway;
              dns = "1.1.1.1;9.9.9.9;";
              route-metric = 10;
            };
            ipv6.method = "auto";
          };
        }
        // lib.optionalAttrs (cfg.wifi != null) {
          house-wifi = {
            connection = {
              id = "house-wifi";
              type = "wifi";
              interface-name = cfg.wifi;
              autoconnect = true;
              autoconnect-priority = 50;
            };
            wifi = {
              ssid = cfg.ssid;
              mode = "infrastructure";
              # the card's own address, not a random one per connection:
              # the router's reservation names it
              cloned-mac-address = "permanent";
            };
            wifi-security = {
              key-mgmt = "wpa-psk";
              psk = "$psk";
            };
            ipv4 = {
              # dhcp: two links cannot share the fixed address (two macs,
              # one ip), and the forward names the wired one. On wifi alone
              # the box is still on the tailnet and its name still follows
              # its public address
              method = "auto";
              route-metric = 20;
            };
            ipv6.method = "auto";
          };
        };
    };
    networking.networkmanager.wifi.powersave = false;
    # a profile that leaves the config leaves the box: ensureProfiles only
    # writes, so without this an old profile stays active until a reboot
    systemd.services.NetworkManager-ensure-profiles.preStart =
      let
        keep = lib.concatMapStringsSep " " (n: "-not -name ${lib.escapeShellArg "${n}.nmconnection"}") (
          lib.attrNames config.networking.networkmanager.ensureProfiles.profiles
        );
      in
      ''
        mkdir -p /run/NetworkManager/system-connections
        find /run/NetworkManager/system-connections -name '*.nmconnection' ${keep} -delete
      '';
  };
}
