#!/usr/bin/env bash
# Where a box is: a site id from its egress address and gateway, a region from geoip.
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
if info=$(curl -4 -sf -m 15 "$URL"); then
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
    "$ip" "$mac" "$country" "$area" "$site" "$region" > $STATE/last.json.tmp
  mv $STATE/last.json.tmp $STATE/last.json
elif [ -s $STATE/last.json ]; then
  site=$(jq -r .site $STATE/last.json); region=$(jq -r .region $STATE/last.json); source=cached
fi

{
  echo "# HELP dd_box_location Where this box thinks it is: ids only, the names are private."
  echo "# TYPE dd_box_location gauge"
  echo "dd_box_location{box=\"$host\",site=\"$site\",region=\"$region\",source=\"$source\"} 1"
} > $FACTS/dd_location.prom.tmp
mv $FACTS/dd_location.prom.tmp $FACTS/dd_location.prom
printf '{"site":"%s","region":"%s","source":"%s"}\n' "$site" "$region" "$source" > $FACTS/location.json.tmp
mv $FACTS/location.json.tmp $FACTS/location.json
echo "site $site region $region ($source)"
