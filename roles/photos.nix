{ config, ... }:
{
  imports = [
    ./_sops.nix
    ../modules/ente.nix
  ];
  sops.secrets = {
    # museum's verification code for addresses under users.<domain>: the
    # photos page sends it when it makes a person's account, so it is read
    # by museum (as ente) and by the verifier (as dd-verify), one key twice
    ente-ott.owner = "ente";
    ente-ott-verify = {
      key = "ente-ott";
      owner = "dd-verify";
    };
    garage-key-id.owner = "ente";
    garage-key-secret.owner = "ente";
    ente-key-encryption.owner = "ente";
    ente-key-hash.owner = "ente";
    ente-jwt-secret.owner = "ente";
    ente-smtp-password.owner = "ente";
  };

  dd.verify.photos = {
    api = "https://api.${config.dd.domain}";
    emailSuffix = "@users.${config.dd.domain}";
    codeFile = config.sops.secrets.ente-ott-verify.path;
  };
  dd.home.services = [
    {
      name = "Photos";
      # the passkey opens it: the page makes or opens the ente account in
      # the browser and hands ente's app the session
      url = "https://photos.${config.dd.domain}/_dd/photos";
      # the demo sees a public album, if one is shared (dd.demo.album)
      demo = config.dd.demo.album;
      description = "Your photos and videos, backed up from your phone. Encrypted with a key only your passkey makes.";
      icon = "photos";
      color = "#d9822b";
      rank = 10;
    }
  ];

  # ente's bucket and key. The key is IMPORTED from sops rather than minted
  # here, so ente is configured with the same credentials at deploy time.
  sops.templates."garage-ente-key.env".content = ''
    GARAGE_KEY_ID=${config.sops.placeholder.garage-key-id}
    GARAGE_KEY_SECRET=${config.sops.placeholder.garage-key-secret}
  '';
  dd.garage.setupEnvFiles = [ config.sops.templates."garage-ente-key.env".path ];
  dd.garage.setup = ''
    garage bucket create ente 2>/dev/null || true
    garage key import "$GARAGE_KEY_ID" "$GARAGE_KEY_SECRET" --yes -n ente 2>/dev/null || true
    garage bucket allow --read --write --owner ente --key "$GARAGE_KEY_ID" 2>/dev/null || true

    # Browsers upload blobs straight to garage, so the bucket needs CORS or
    # every upload fails the preflight with "This CORS request is not
    # allowed". garage has no CLI for this - it is an S3 API call.
    export AWS_ACCESS_KEY_ID="$GARAGE_KEY_ID"
    export AWS_SECRET_ACCESS_KEY="$GARAGE_KEY_SECRET"
    export AWS_DEFAULT_REGION=us-east-1
    aws --endpoint-url http://127.0.0.1:3900 s3api put-bucket-cors \
      --bucket ente --cors-configuration '${
        builtins.toJSON {
          CORSRules = [
            {
              # Must be "*". Ente decrypts in a web worker, which is a sandboxed
              # context, so the browser sends Origin: null - it cannot send the
              # photos origin even in principle. Listing real origins also makes
              # garage emit them comma-separated, which is invalid and silently
              # rejected. CORS is not the access control here: the presigned URL
              # signature is, and the objects are ciphertext regardless.
              AllowedOrigins = [ "*" ];
              AllowedMethods = [
                "GET"
                "PUT"
                "POST"
                "DELETE"
                "HEAD"
              ];
              AllowedHeaders = [ "*" ];
              ExposeHeaders = [
                "etag"
                "ETag"
                "x-amz-request-id"
              ];
              MaxAgeSeconds = 3000;
            }
          ];
        }
      }' 2>/dev/null || true
  '';
}
