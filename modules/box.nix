{ config, lib, ... }:
{
  # What this box may hold. A box whose owner would be handed the plaintext
  # anyway may run the services that read it: media, the language model,
  # the forge, people's uploads. Any other box gets only what is safe
  # anywhere: the directory, encrypted blobs, the nix cache. Each module
  # that reads plaintext asserts this, so putting one on the wrong box is a
  # build error, not a discovery.
  options.dd.box.ownerTrusted = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = "Whether the owner of this box is trusted with users' plaintext.";
  };

  options.dd.box.plaintext = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    internal = true;
    description = "Modules that read plaintext, registered by the modules themselves.";
  };

  config.assertions = lib.optional (config.dd.box.plaintext != [ ] && !config.dd.box.ownerTrusted) {
    assertion = false;
    message = ''
      ${config.networking.hostName} is not owner-trusted (dd.box.ownerTrusted) but would run
      services that read people's plaintext: ${lib.concatStringsSep ", " config.dd.box.plaintext}.
      A stranger's box holds ciphertext and the directory, nothing else.
    '';
  };
}
