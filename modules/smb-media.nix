{
  config,
  pkgs,
  lib,
  ...
}:
let
  root = "/srv/users";

  # Add a name here and the account, directory, acl, share and password all
  # follow from a rebuild. The password comes from the sops key
  # smb-password-<name>, which has to exist before the switch.
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
    # nginx needs write for the webdav endpoint in modules/webdav-media.nix;
    # jellyfin only ever reads
    setfacl -m u:nginx:rwx ${root}/${name}
    setfacl -d -m u:nginx:rwx ${root}/${name}
    setfacl -d -m u:${name}:rwx ${root}/${name}
  '';
in
{
  sops.secrets = lib.genAttrs (map (n: "smb-password-${n}") mediaUsers) (_: { });

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

  # Samba keeps passwords in its own tdb, which no nixos option writes to, so
  # this is the declarative equivalent: set them from sops on every rebuild.
  # Idempotent - re-setting the same password is a no-op, and rotating the
  # secret takes effect on the next switch.
  systemd.services.samba-passwords = {
    description = "Apply samba passwords from sops";
    after = [ "samba-smbd.service" ];
    requires = [ "samba-smbd.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = lib.concatMapStrings (name: ''
      pw=$(cat ${config.sops.secrets."smb-password-${name}".path})
      printf '%s\n%s\n' "$pw" "$pw" \
        | ${config.services.samba.package}/bin/smbpasswd -s -a ${name}
      ${config.services.samba.package}/bin/smbpasswd -e ${name} >/dev/null
    '') mediaUsers;
  };

  services.samba = {
    enable = true;
    openFirewall = false; # tailscale0 is already a trusted interface
    settings = {
      global = {
        security = "user";
        "server min protocol" = "SMB3";
        "server smb encrypt" = "required";
        # No "interfaces"/"bind interfaces only" here. smbd will not select
        # tailscale0's address by any spelling - interface name, bare ip, /32,
        # or the 100.64.0.0/10 range all leave it bound to loopback alone, and
        # it logs nothing about refusing. Tested all four on the host.
        #
        # The firewall is the boundary instead, and a better one: nixos-fw
        # accepts everything arriving on tailscale0 and 445 appears in no other
        # accept rule, so the tailnet reaches it and nothing else does.
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
