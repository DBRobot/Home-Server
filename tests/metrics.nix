# A box measures itself: the facts unit writes what the box is, the
# exporter serves it, prometheus on the box scrapes it. Nothing reaches
# across boxes.
{ self, ... }:
{
  name = "metrics";
  node.specialArgs = { inherit self; };
  defaults.virtualisation.memorySize = 1024;
  defaults.virtualisation.cores = 2; # services start in parallel instead of queueing on one thread
  defaults.virtualisation.diskSize = 2048;
  nodes.box = {
    imports = [
      ./box.nix
      ../modules/locate.nix
    ];
    dd.locate.url = "http://geo/json";
  };
  # stands in for the geoip service: answers with a fixed address and place
  nodes.geo = {
    services.nginx = {
      enable = true;
      virtualHosts.geo.locations."/json".return = ''200 '{"ip":"203.0.113.7","country":"US","region":"Massachusetts"}' '';
    };
    networking.firewall.enable = false;
  };
  testScript = ''
    # a oneshot with no RemainAfterExit is "inactive" once done; run it and look
    box.succeed("systemctl start dd-facts.service")
    box.succeed("grep -q 'dd_box_cpu_cores{box=\"box\"}' /var/lib/dd-facts/dd_box.prom")
    box.wait_for_unit("prometheus-node-exporter.service")
    box.wait_until_succeeds("curl -sf http://127.0.0.1:9100/metrics -o /tmp/out && grep -q dd_box_memory_bytes /tmp/out")
    # the box works out where it is; the ids are hashes with the right shape
    geo.wait_for_unit("nginx.service")
    box.succeed("systemctl start dd-locate.service")
    box.succeed("grep -qE 'dd_box_location\\{box=\"box\",site=\"s-[0-9a-f]{8}\",region=\"r-[0-9a-f]{8}\",source=\"live\"\\} 1' /var/lib/dd-facts/dd_location.prom")
    # the same place gives the same ids again, and a box that cannot ask
    # keeps what it had
    first = box.succeed("cat /var/lib/dd-facts/location.json")
    geo.succeed("systemctl stop nginx.service")
    box.succeed("systemctl start dd-locate.service")
    assert box.succeed("cat /var/lib/dd-facts/location.json").replace("live", "cached") == first.replace("live", "cached")
    box.succeed("grep -q 'source=\"cached\"' /var/lib/dd-facts/dd_location.prom")
    box.wait_for_unit("prometheus.service")
    box.wait_until_succeeds("curl -sf 'http://127.0.0.1:9090/api/v1/query?query=dd_box_cpu_cores' -o /tmp/out && grep -q '\"value\"' /tmp/out")
  '';
}
