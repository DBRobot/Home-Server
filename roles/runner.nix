{ config, ... }:
{
  # A CI runner for the forge, registered under this box's name with the
  # nix label. Stateless: any box can run one, jobs land on whichever is
  # free, and CI keeps going while another box is being reinstalled. What
  # it builds goes to the nix-cache bucket, signed with this box's key, so
  # the next run and the next reinstall find it there.
  imports = [
    ./_sops.nix
    ../modules/forgejo-runner.nix
    ../modules/cache.nix
  ];

  sops.secrets.nix-cache-signing-key = { };
  dd.cache.signingKeyFile = config.sops.secrets.nix-cache-signing-key.path;
  # the box's cache key, as nix reads credentials; write is granted below
  sops.templates."aws-credentials" = {
    path = "/root/.aws/credentials";
    mode = "0600";
    content = ''
      [default]
      aws_access_key_id = ${config.sops.placeholder.cache-key-id}
      aws_secret_access_key = ${config.sops.placeholder.cache-key-secret}
    '';
  };
  dd.garage.setup = ''
    # a runner writes the cache it reads
    garage bucket allow --write nix-cache --key "$GARAGE_CACHE_KEY_ID" 2>/dev/null || true
  '';
}
