# The smallest thing that is still one of our boxes: the modules under test
# and nothing that needs real hardware, real dns or a real certificate. A
# test boots two or three of these in a few seconds; the real hosts import
# the same modules, so what passes here is what runs there.
{ ... }:
{
  imports = [
    ../modules/domain.nix
    ../modules/box.nix
    ../modules/verify.nix
    ../modules/metrics.nix
  ];
  dd.domain = "test.invalid";
  dd.verify.role = "directory";
  dd.verify.syncSeconds = 2;
  networking.firewall.enable = false; # the test network is the only network
}
