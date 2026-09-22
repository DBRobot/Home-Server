# The house network: a box on the router by cable when it has one, by wifi
# when it does not, and by the laptop's link only if both are gone. Three
# NetworkManager profiles, ordered by route metric, so the best path that
# is up carries the default route and the others wait. Every box gets a
# fixed address on the house network (the router's reservation matches),
# since the port forward names it.
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
      description = "the cabled interface to the router, if any (interface-name)";
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
              interface-name = cfg.wired;
              autoconnect = true;
              autoconnect-priority = 100;
            };
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
  };
}
