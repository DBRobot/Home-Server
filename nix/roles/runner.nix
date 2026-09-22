{ config, ... }:
{
  # A CI runner for the forge, registered under this box's name with the
  # nix label. Stateless: any box can run one, jobs land on whichever is
  # free, and CI keeps going while another box is being reinstalled. What
  # it builds goes to the nix-cache bucket, signed with this box's key, so
  # the next run and the next reinstall find it there.
  imports = [
    ./_sops.nix
    ../modules/forge/forgejo-runner.nix
    ../modules/forge/members-runner.nix
    ../modules/storage/cache.nix
  ];

  # the members' runner registers for every repo; its token is global
  sops.secrets.forgejo-members-runner-token = { };
  sops.templates."forgejo-members-runner.env".content = ''
    TOKEN=${config.sops.placeholder.forgejo-members-runner-token}
  '';
  dd.membersRunner.tokenFile = config.sops.templates."forgejo-members-runner.env".path;
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
  # a runner writes the cache it reads: the storage role's key, more rights
  dd.garage.buckets.nix-cache.allow = [
    "read"
    "write"
  ];
}
