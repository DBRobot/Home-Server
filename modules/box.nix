{ config, lib, ... }:
{
  # No box is trusted. A service that reads people's plaintext exposes it
  # to whichever box runs it, and until that service is fixed that is a
  # fact to see, not a rule to enforce. Each such module registers itself
  # here; the box publishes the list as a metric and `dd box list` shows it,
  # so the work left is always in view and nothing is hidden behind a flag.
  options.dd.box.plaintext = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    description = "Services on this box that read people's plaintext.";
  };

  config.environment.etc."dd/plaintext".text =
    lib.concatStringsSep "\n" config.dd.box.plaintext + "\n";
}
