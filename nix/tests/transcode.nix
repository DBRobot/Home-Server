# The compute side of the libraries: a clip is chunked and encrypted the
# way a device would (nix/tests/transcode-chunks.py), served over plain
# http like the bucket would, and the box turns it into HLS with only the
# file key, sealed to its ephemeral key. The session is wiped on request.
{ self, pkgs, ... }:
{
  name = "transcode";
  node.specialArgs = { inherit self; };
  defaults.virtualisation.memorySize = 2048;
  defaults.virtualisation.cores = 2;
  nodes.box = {
    imports = [
      ./box.nix
      ../modules/library/transcode.nix
    ];
    dd.transcode.enable = true;
    environment.systemPackages = [
      pkgs.ffmpeg-headless
      pkgs.curl
      (pkgs.python3.withPackages (p: [ p.pynacl ]))
    ];
  };
  scriptEnv = {
    chunker = ./transcode-chunks.py;
  };
}
