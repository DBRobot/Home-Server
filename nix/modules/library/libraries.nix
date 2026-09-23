# The members' encrypted libraries (crate library): one bucket, a prefix per
# library, and the verifier's gate as the only way in. The box holds
# ciphertext and one key of its own for the bucket; a member's device never
# sees that key, it asks the gate for a url to one object at a time.
{
  config,
  lib,
  ...
}:
let
  cfg = config.dd.libraries;
in
{
  options.dd.libraries = {
    enable = lib.mkEnableOption "the members' encrypted libraries on this box";
    keyFile = lib.mkOption {
      type = lib.types.path;
      description = "env file with LIBRARY_ID and LIBRARY_SECRET: the gate's garage key, imported at setup";
    };
  };

  config = lib.mkIf cfg.enable {
    dd.garage.setupEnvFiles = [ cfg.keyFile ];
    dd.garage.buckets.libraries = {
      key = {
        name = "dd-library";
        envPrefix = "LIBRARY";
      };
      allow = [
        "read"
        "write"
      ];
      # devices fetch and upload chunks straight to the bucket with the urls
      # the gate signs; the objects are ciphertext, the signature is the
      # access control
      cors = {
        CORSRules = [
          {
            AllowedOrigins = [ "*" ];
            AllowedMethods = [
              "GET"
              "PUT"
            ];
            AllowedHeaders = [ "*" ];
            MaxAgeSeconds = 3600;
          }
        ];
      };
    };
    dd.verify.library = {
      s3 = "https://s3.${config.dd.domain}";
      bucket = "libraries";
      keyFile = cfg.keyFile;
    };
  };
}
