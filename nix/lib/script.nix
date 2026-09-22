# A unit's script as a file beside its module, with the values the Nix side
# knows handed over as environment variables: `script ./setup.sh { URL = ...; }`
# gives the shell `$URL`, and the file reads as shell.
{ lib }:
file: vars:
lib.concatStringsSep "\n" (
  lib.mapAttrsToList (k: v: "export ${k}=${lib.escapeShellArg (toString v)}") vars
)
+ "\n"
+ builtins.readFile file
