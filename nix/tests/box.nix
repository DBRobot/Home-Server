# The smallest thing that is still one of our boxes: the modules under test
# and nothing that needs real hardware, real dns or a real certificate. A
# test boots two or three of these in a few seconds; the real hosts import
# the same modules, so what passes here is what runs there.
{ ... }:
{
  imports = [
    ../modules/box/domain.nix
    ../modules/box/box.nix
    ../modules/gate/verify.nix
    ../modules/observe/metrics.nix
  ];
  dd.domain = "test.invalid";
  dd.repo = "tester/commonty";
  dd.verify.role = "directory";
  dd.verify.syncSeconds = 2;
  networking.firewall.enable = false; # the test network is the only network
}
