{
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
        ExecStartPre = "+${pkgs.coreutils}/bin/install -d -m 0755 -o node-exporter -g node-exporter ${facts}";
        User = "node-exporter";
        Group = "node-exporter";
        StateDirectory = "dd-locate";
      };
      script = ''
        set -euo pipefail
        host=$(uname -n)
        site=unknown; region=unknown; source=none

        # the router this box is behind: the mac of its default gateway
        gw=$(ip -4 route show default | awk '{print $3; exit}' || true)
        mac=""
        if [ -n "$gw" ]; then
          ip neigh show "$gw" >/dev/null 2>&1 || true
          mac=$(ip neigh show "$gw" | awk '/lladdr/ {print $5; exit}' || true)
        fi

        # what the internet sees, and where that is
        if info=$(curl -4 -sf -m 15 "${cfg.url}"); then
          ip=$(printf '%s' "$info" | jq -r '.ip // empty')
          country=$(printf '%s' "$info" | jq -r '.country // empty')
          area=$(printf '%s' "$info" | jq -r '.region // empty')
          if [ -n "$ip" ]; then
            site="s-$(printf '%s|%s' "$ip" "$mac" | sha256sum | cut -c1-8)"
            source=live
          fi
          if [ -n "$country" ]; then
            region="r-$(printf '%s|%s' "$country" "$area" | sha256sum | cut -c1-8)"
          fi
          # the names stay here, readable by nobody but root and this unit
          printf '{"ip":"%s","gateway_mac":"%s","country":"%s","area":"%s","site":"%s","region":"%s"}\n' \
            "$ip" "$mac" "$country" "$area" "$site" "$region" > ${state}/last.json.tmp
          mv ${state}/last.json.tmp ${state}/last.json
        elif [ -s ${state}/last.json ]; then
          site=$(jq -r .site ${state}/last.json); region=$(jq -r .region ${state}/last.json); source=cached
        fi

        {
          echo "# HELP dd_box_location Where this box thinks it is: ids only, the names are private."
          echo "# TYPE dd_box_location gauge"
          echo "dd_box_location{box=\"$host\",site=\"$site\",region=\"$region\",source=\"$source\"} 1"
        } > ${facts}/dd_location.prom.tmp
        mv ${facts}/dd_location.prom.tmp ${facts}/dd_location.prom
        printf '{"site":"%s","region":"%s","source":"%s"}\n' "$site" "$region" "$source" > ${facts}/location.json.tmp
        mv ${facts}/location.json.tmp ${facts}/location.json
        echo "site $site region $region ($source)"
      '';
    };
  };
}
