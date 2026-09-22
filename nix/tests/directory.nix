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
  defaults.virtualisation.cores = 2; # services start in parallel instead of queueing on one thread
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
  scriptEnv = {
    dd = "${self.packages.${pkgs.stdenv.hostPlatform.system}.dd}/bin/dd";
  };
}
