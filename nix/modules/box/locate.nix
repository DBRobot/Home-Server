{
  ddScript,
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.dd.locate;
  facts = "/var/lib/dd-facts";
  state = "/var/lib/dd-locate";
in
{
  # Where a box is, worked out by the box: a site id from its public
  # egress address and the gateway it sits behind (two boxes on the same
  # router agree without either publishing the address), and a region id
  # from a geoip lookup of that address (state or province, the grain a
  # wildfire or a power cut has). Both are hashes, so what the box
  # publishes says nothing on its own; the private half of the box list
  # holds the names. The signed list stays the authority for placement:
  # this is what a box proposes, and a mismatch with the list is visible.
  options.dd.locate = {
    url = lib.mkOption {
      type = lib.types.str;
      default = "https://ipinfo.io/json";
      description = "Answers with the caller's ip, country and region as json; the vm test points this at itself.";
    };
  };

  config = {
    systemd.services.dd-locate = {
      description = "Work out this box's site and region ids";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      startAt = "hourly";
      path = with pkgs; [
        coreutils
        curl
        jq
        iproute2
        gawk
        gnused
      ];
      serviceConfig = {
        Type = "oneshot";

        User = "node-exporter";
        Group = "node-exporter";
        StateDirectory = "dd-locate";
      };
      script = ddScript ./locate.sh {
        URL = cfg.url;
        FACTS = facts;
        STATE = state;
      };
    };
  };
}
