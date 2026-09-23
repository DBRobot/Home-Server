{ lib, ... }:
{
  # The public name every service hangs off. One place: the move from the
  # duckdns name to commonty.org was a change here, not an edit in eleven
  # files.
  options.dd.domain = lib.mkOption {
    type = lib.types.str;
    description = "Base domain; services are subdomains of it.";
  };
  # the fleet's repository on its forge, as owner/name: where releases are
  # read from, what ci protects, what the mirror pushes
  options.dd.repo = lib.mkOption {
    type = lib.types.str;
    description = "the fleet's repository on the forge, owner/name";
  };
}
