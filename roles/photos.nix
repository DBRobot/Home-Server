{ ... }:
{
  # ente: end-to-end encrypted photos. Its pre-start reads these as the ente
  # user; sops defaults to root-only, which made museum fail with
  # "Permission denied".
  imports = [
    ./_sops.nix
    ../modules/ente.nix
  ];
  sops.secrets = {
    garage-key-id.owner = "ente";
    garage-key-secret.owner = "ente";
    ente-key-encryption.owner = "ente";
    ente-key-hash.owner = "ente";
    ente-jwt-secret.owner = "ente";
    ente-smtp-password.owner = "ente";
  };
}
