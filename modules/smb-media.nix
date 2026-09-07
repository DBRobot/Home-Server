{
  config,
  pkgs,
  lib,
  ...
}:
let
  root = "/srv/users";

  # Add a name here and the account, directory, acl and share all follow.
  # A samba password still has to be set once: sudo smbpasswd -a <name>
  mediaUsers = [ "david" ];

  # Each user gets their OWN primary group, not a shared one - a shared group
  # would let every media user read every other's files. Jellyfin gets in via
  # a named acl entry instead.
  #
  # 0750 rather than 0700 is load-bearing: on posix acls the group bits are the
  # mask that caps named entries, so with 0700 the mask is 0 and the jellyfin
  # entry is masked out to nothing. The group here contains only the user, so
  # the bits grant no one anything by themselves.
  mkDir = name: ''
    install -d -m 0750 -o ${name} -g ${name} ${root}/${name}
    setfacl -m u:jellyfin:r-x ${root}/${name}
    setfacl -d -m u:jellyfin:r-x ${root}/${name}
    setfacl -d -m u:${name}:rwx ${root}/${name}
  '';
in
{
  users.users = lib.genAttrs mediaUsers (name: {
    isNormalUser = true;
    group = name;
    home = "${root}/${name}";
    createHome = false; # the oneshot below owns this, after zfs mounts
    shell = "${pkgs.shadow}/bin/nologin"; # smb only, no shell
  });
  users.groups = lib.genAttrs mediaUsers (_: { });

  systemd.services.media-user-dirs = {
    description = "Per-user media directories, readable by jellyfin alone";
    after = [
      "zfs-mount.service"
      "zfs-datasets.service"
    ];
    requires = [ "zfs-datasets.service" ];
    before = [ "samba-smbd.service" ];
    wantedBy = [ "multi-user.target" ];
    path = [
      pkgs.acl
      pkgs.coreutils
    ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      install -d -m 0755 -o root -g root ${root}
    ''
    + lib.concatMapStrings mkDir mediaUsers;
  };

  # smbd binds at start, so the tailnet address has to exist by then
  systemd.services.samba-smbd = {
    after = [ "tailscaled.service" ];
    wants = [ "tailscaled.service" ];
  };

  services.samba = {
    enable = true;
    openFirewall = false; # tailscale0 is already a trusted interface
    settings = {
      global = {
        security = "user";
        "server min protocol" = "SMB3";
        "server smb encrypt" = "required";
        # Never on the wifi or the direct cable, only the tailnet. Matched by
        # network range, not by name: tailscale0 carries a /32, and samba
        # silently skips interfaces it cannot derive a subnet from - naming it
        # here leaves smbd bound to loopback with nothing logged.
        # 100.64.0.0/10 is the CGNAT range tailscale allocates from.
        interfaces = "lo 100.64.0.0/10";
        "bind interfaces only" = "yes";
        "invalid users" = [ "root" ];
        "guest ok" = "no";
        "load printers" = "no";
        "printcap name" = "/dev/null";
        "disable spoolss" = "yes";
      };
    }
    // lib.listToAttrs (
      map (name: {
        inherit name;
        value = {
          path = "${root}/${name}";
          "valid users" = name;
          "force user" = name;
          "force group" = name;
          browseable = "no"; # you only see your own share
          writable = "yes";
          "inherit acls" = "yes";
          # must leave the group bit set: it is the acl mask, and 0600 here
          # would strip jellyfin's access from every uploaded file
          "create mask" = "0640";
          "directory mask" = "0750";
        };
      }) mediaUsers
    );
  };
}
