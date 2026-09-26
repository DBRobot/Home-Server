# The restore check on the postgres dumps. It proves a dump restores, which
# is the only proof there is; this proves it does so without handing the
# database's contents a superuser. What is in ente's database is whatever
# can write to it, and a restore runs some of it - an index built on a
# function runs that function - with the restoring role's rights.
{ pkgs, ... }:
{
  name = "pgrestore";
  nodes.box =
    { ... }:
    {
      imports = [ ../modules/storage/postgres-backup.nix ];
      services.postgresql = {
        enable = true;
        ensureDatabases = [
          "ente"
          "forgejo"
        ];
      };
      environment.systemPackages = [ pkgs.zstd ];
    };
  testScript = builtins.readFile ./pgrestore.py;
}
