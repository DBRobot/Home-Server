# Two boxes share the directory; a client publishes to one and the other
# catches up; a squat on the second box is refused; a box whose peer is gone
# takes no new names. The same rules the in-process tests prove, now across
# real boots and a real network.
{
  pkgs,
  self,
  ...
}:
let
  dd = "${self.packages.${pkgs.stdenv.hostPlatform.system}.dd}/bin/dd";
in
{
  name = "directory";
  node.specialArgs = { inherit self; };
  defaults.virtualisation.memorySize = 1024;
  defaults.virtualisation.diskSize = 2048;
  nodes = {
    a = {
      imports = [ ./box.nix ];
      dd.verify.peers = [ "http://b:4181/_dd/directory" ];
    };
    b = {
      imports = [ ./box.nix ];
      dd.verify.peers = [ "http://a:4181/_dd/directory" ];
    };
    # a peer that will never answer: c may serve what it has but takes no new names
    c = {
      imports = [ ./box.nix ];
      # TEST-NET-1: routable nowhere, so the pull fails instead of - as a
      # loopback alias did - answering from c itself
      dd.verify.peers = [ "http://192.0.2.1:4181/_dd/directory" ];
    };
    client = {
      environment.systemPackages = [ pkgs.curl ];
    };
  };
  testScript = ''
    start_all()
    for m in (a, b, c):
        m.wait_for_unit("dd-verify.service")
        m.wait_for_open_port(4181)
    a.wait_until_succeeds("journalctl -u dd-verify > /tmp/j && grep -q 'every peer pulled once' /tmp/j")
    b.wait_until_succeeds("journalctl -u dd-verify > /tmp/j && grep -q 'every peer pulled once' /tmp/j")

    env = "DD_KEYRING_FILE=/root/laptop.json"
    client.succeed(f"{env} ${dd} identity new --name sarah --directory http://a:4181/_dd/directory")
    b.wait_until_succeeds("curl -sf http://localhost:4181/_dd/directory/sarah")

    # a stranger's sarah on b: b asks a, which holds her under another root
    client.fail("DD_KEYRING_FILE=/root/stranger.json ${dd} identity new --name sarah --directory http://b:4181/_dd/directory")

    # c never pulled its peer: it refuses a new name
    out = client.fail("DD_KEYRING_FILE=/root/tom.json ${dd} identity new --name tom --directory http://c:4181/_dd/directory 2>&1")
    assert "503" in out or "not yet pulled" in out, out

    # a device added through b alone reaches a
    pub = client.succeed("DD_KEYRING_FILE=/root/phone.json ${dd} device show | tail -1").strip()
    client.succeed(f"{env} ${dd} device admit {pub} --directory http://b:4181/_dd/directory")
    a.wait_until_succeeds("curl -sf http://localhost:4181/_dd/directory/sarah -o /tmp/out && grep -q '\"version\":2' /tmp/out")

    # b restarts and still has sarah at version 2
    b.succeed("systemctl restart dd-verify")
    b.wait_for_open_port(4181)
    b.wait_until_succeeds("curl -sf http://localhost:4181/_dd/directory/sarah -o /tmp/out && grep -q '\"version\":2' /tmp/out")
  '';
}
