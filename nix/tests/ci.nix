# Evaluation only: the ci workflow's vm matrix names exactly the tests the
# flake runs. The workflow is yaml and cannot read the flake, so this is
# what keeps a new test from being forgotten there.
{
  pkgs,
  lib,
  vmTests,
  ...
}:
let
  workflow = builtins.readFile ../../.forgejo/workflows/nix_checks.yml;
  line = builtins.head (builtins.match ".*test: [[]([^]]*)[]].*" workflow);
  inMatrix = map (lib.strings.trim) (lib.splitString "," line);
  wanted = lib.sort builtins.lessThan vmTests;
  have = lib.sort builtins.lessThan inMatrix;
in
pkgs.runCommand "ci-matrix-matches" { } (
  if have == wanted then
    "echo ok > $out"
  else
    ''
      echo "ci matrix ${builtins.toJSON have} != flake vm tests ${builtins.toJSON wanted}" >&2
      exit 1
    ''
)
