{ ... }:
{
  # What is true of this machine and no other. Everything it runs is a role
  # in fleet/boxes.json. hardware-configuration.nix is the output of
  # `nixos-generate-config --no-filesystems --show-hardware-config` on the
  # box, unedited: disko owns the mounts, the generator owns the rest.
  imports = [
    ./hardware-configuration.nix
    ./disko.nix
  ];

  # tank is the root pool and imports itself; vault is the usb disk
  boot.zfs.extraPools = [ "vault" ];

  # Which pool holds what. vault is the 4T usb disk; tank the nvme root.
  # 62G and 8 threads: four ci jobs at once, vm tests mostly wait on
  # timers (the 14G box takes the default two)
  dd.runner.capacity = 4;
  dd.zfs.datasets = {
    "vault/photos" = {
      # garage's blocks: ente's and the media tier's ciphertext
      recordsize = "1M"; # large sequential reads
      "com.sun:auto-snapshot" = "true"; # cheap on immutable files, saves you from rm -rf
    };

    # The ente key hierarchy lives in postgres on the ext4 root: no snapshots,
    # no checksums, no redundancy. Without it the blobs on vault cannot be
    # decrypted by anyone, including with the recovery key, because
    # master_key_encrypted_with_recovery_key is itself a row in that database.
    # Dumps land here: different physical disk to the source, snapshotted,
    # and in this box's backup.
    "vault/backups" = {
      recordsize = "128K"; # small compressible dumps, not media
      compression = "zstd";
      "com.sun:auto-snapshot" = "true";
    };

    # Media is the one dataset where losing the single vdev costs only time:
    # it is all re-rippable. No snapshots either - a snapshot of a library is
    # the size of the library.
    "vault/media" = {
      recordsize = "1M"; # large sequential reads
      # h264/hevc is already compressed, so this should do nothing - but
      # vault/photos gets 1.93x on blobs that "should" be incompressible too,
      # and lz4 early-aborts on the ones that really are. Costs nothing to try.
      compression = "lz4";
      "com.sun:auto-snapshot" = "false";
    };

    # Per-user archives of old computers - disk images or copied-out user
    # folders - uploaded already encrypted (rclone crypt, key on the client),
    # so this dataset only ever holds ciphertext. Separate from vault/users on
    # purpose: jellyfin has an acl on every media dir and has no business
    # traversing these, and NO snapshots - an archive is written once and never
    # changed, so a snapshot buys nothing and would keep a deleted 80 G image
    # on disk for a year.
    "vault/images" = {
      mountpoint = "/srv/images";
      recordsize = "1M"; # gigabyte chunks, sequential
      compression = "lz4"; # ciphertext is incompressible; lz4 early-aborts, costs nothing
      "com.sun:auto-snapshot" = "false";
    };

    # Per-user uploads. User data belongs on the pool with everything else.
    # Snapshots are on: unlike vault/media this is not re-rippable, and they
    # cost nothing until a file changes.
    "vault/users" = {
      mountpoint = "/srv/users";
      recordsize = "1M";
      compression = "lz4";
      "com.sun:auto-snapshot" = "true";
    };

    # The forge: repositories, lfs objects and its own config. Small, hot,
    # irreplaceable, so snapshots are on and it sits on the pool with the
    # rest of what people made rather than on the root disk.
    "vault/forgejo" = {
      mountpoint = "/vault/forgejo";
      recordsize = "128K";
      compression = "zstd";
      "com.sun:auto-snapshot" = "true";
    };

    "tank/games" = {
      # members' game servers: each one's record, saves and guest disk
      mountpoint = "/var/lib/dd-games";
    };

    "tank/models" = {
      # the template's pool root has no mountpoint, so a dataset needs its own
      mountpoint = "/tank/models";
      recordsize = "1M"; # large sequential reads of GGUF weights
      compression = "off"; # zstd is inherited pool-wide; weights are incompressible
      primarycache = "metadata"; # llama-server mlocks the weights; ARC caching them is waste
      "com.sun:auto-snapshot" = "false"; # a snapshot of a 23G model costs 23G
    };
  };
  # default c_max is ~all of RAM (measured 61.5G); with no swap that collides
  # with the model's mlocked 23G. Raise once photos land on the HDD pool.
  boot.extraModprobeConfig = ''
    options zfs zfs_arc_max=8589934592
  '';

  networking.hostId = "0195f284";
  # the usb ethernet adapter must never autosuspend - it is the only way in
  boot.kernelParams = [ "usbcore.autosuspend=-1" ];
  networking.networkmanager.ensureProfiles.profiles.direct-link = {
    connection = {
      id = "direct-link";
      type = "ethernet";
      autoconnect = true;
      autoconnect-priority = -999;
    };
    ethernet.mac-address = "44:ED:57:10:00:40"; # not interface-name; that encodes the USB port
    ipv4 = {
      method = "manual";
      address1 = "10.10.10.2/24";
      gateway = "10.10.10.1"; # NATed out the Legion's wifi until home internet exists
      dns = "1.1.1.1;9.9.9.9;";
    };
    ipv6.method = "link-local";
  };
}
