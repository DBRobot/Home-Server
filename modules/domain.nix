{ lib, ... }:
{
  # The public name every service hangs off. One place, because a real domain
  # will replace duckdns eventually and that move is a config change here and
  # a documented migration in kanidm - not an edit in eleven files.
  options.dd.domain = lib.mkOption {
    type = lib.types.str;
    description = "Base domain; services are subdomains of it.";
  };
}
