# A box measures itself: the facts unit writes what the box is, the
# exporter serves it, prometheus on the box scrapes it. Nothing reaches
# across boxes.
{ self, ... }:
{
  name = "metrics";
  node.specialArgs = { inherit self; };
  defaults.virtualisation.memorySize = 1024;
  defaults.virtualisation.diskSize = 2048;
  nodes.box = {
    imports = [ ./box.nix ];
  };
  testScript = ''
    # a oneshot with no RemainAfterExit is "inactive" once done; run it and look
    box.succeed("systemctl start dd-facts.service")
    box.succeed("grep -q 'dd_box_cpu_cores{box=\"box\"}' /var/lib/dd-facts/dd_box.prom")
    box.wait_for_unit("prometheus-node-exporter.service")
    box.wait_until_succeeds("curl -sf http://127.0.0.1:9100/metrics -o /tmp/out && grep -q dd_box_memory_bytes /tmp/out")
    box.wait_for_unit("prometheus.service")
    box.wait_until_succeeds("curl -sf 'http://127.0.0.1:9090/api/v1/query?query=dd_box_cpu_cores' -o /tmp/out && grep -q '\"value\"' /tmp/out")
  '';
}
