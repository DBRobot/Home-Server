{ config, ... }:
{
  # A box on the fleet's own network that is not the control box: the
  # boxes' join key comes through sops, copied from the control box's
  # /var/lib/headscale/box.key by hand, once.
  imports = [ ./_sops.nix ];
  sops.secrets.headscale-box-key = { };
  dd.net.keyFile = config.sops.secrets.headscale-box-key.path;
}
