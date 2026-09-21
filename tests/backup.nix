# Two storage boxes form the garage cluster, each backs itself up into it,
# and a box that has lost everything gets its files back from the other:
# the copy is on both boxes, reads survive a box going away, and the
# restore path is the one that matters, not the backup path.
{ pkgs, self, ... }:
let
  rpc = pkgs.writeText "garage.env" "GARAGE_RPC_SECRET=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n";
  key = box: {
    id = "GK${builtins.substring 0 24 (builtins.hashString "sha256" box)}";
    secret = builtins.hashString "sha256" "${box}-secret";
  };
  storageBox = name: {
    imports = [
      ./box.nix
      ../modules/garage.nix
      ../modules/backup.nix
    ];
    dd.garage = {
      zone = "r-test";
      capacity = "1G";
      dataDir = "/srv/garage";
      publicAddr = "${name}:3901";
      envFile = rpc;
      setupEnvFiles = [
        (pkgs.writeText "backup-key.env" ''
          GARAGE_BACKUP_KEY_ID=${(key name).id}
          GARAGE_BACKUP_KEY_SECRET=${(key name).secret}
        '')
      ];
      setup = ''
        garage bucket create backups-${name} 2>/dev/null || true
        garage key import "$GARAGE_BACKUP_KEY_ID" "$GARAGE_BACKUP_KEY_SECRET" --yes -n backup-${name} 2>/dev/null || true
        garage bucket allow --read --write --owner backups-${name} --key "$GARAGE_BACKUP_KEY_ID" 2>/dev/null || true
      '';
    };
    dd.backup = {
      paths = [ "/srv/state" ];
      envFile = pkgs.writeText "restic.env" ''
        AWS_ACCESS_KEY_ID=${(key name).id}
        AWS_SECRET_ACCESS_KEY=${(key name).secret}
        AWS_DEFAULT_REGION=us-east-1
      '';
      passwordFile = pkgs.writeText "restic-password" "${name}-password\n";
    };
    environment.systemPackages = [
      pkgs.restic
      pkgs.awscli2
    ];
  };
in
{
  name = "backup";
  node.specialArgs = { inherit self; };
  defaults.virtualisation.memorySize = 1536;
  defaults.virtualisation.cores = 2; # services start in parallel instead of queueing on one thread
  defaults.virtualisation.diskSize = 4096;
  nodes = {
    a = storageBox "a";
    b = storageBox "b";
  };
  testScript = ''
    start_all()
    for m in (a, b):
        m.wait_for_unit("garage.service")
        m.wait_for_open_port(3901)

    # the boxes find each other the way the real ones do: b is told a's id
    # (in the fleet it comes from the box list), then each stages its own
    # role and the layout applies once both are in
    a_id = a.succeed("set -a; . ${rpc}; garage node id -q").strip().split("@")[0]
    b.succeed(f"set -a; . ${rpc}; garage node connect {a_id}@a:3901")
    for m in (a, b):
        m.succeed("systemctl restart garage-setup.service")
    # both boxes hold the applied layout before the buckets and keys are made
    for m in (a, b):
        m.wait_until_succeeds("set -a; . ${rpc}; garage layout show > /tmp/l && grep -q 'Current cluster layout version: [1-9]' /tmp/l")
    for m in (a, b):
        m.succeed("systemctl restart garage-setup.service") # buckets and keys, now that the layout exists

    # a backs up, b backs up
    a.succeed("mkdir -p /srv/state && echo 'the thing worth having tomorrow' > /srv/state/file && head -c 2000000 /dev/urandom > /srv/state/blob")
    b.succeed("mkdir -p /srv/state && echo 'b has state too' > /srv/state/file")
    a.succeed("systemctl start restic-backups-dd.service")
    b.succeed("systemctl start restic-backups-dd.service")
    a.succeed("grep -q dd_backup_last_success_seconds /var/lib/dd-facts/backup.prom")

    # every block is on both boxes
    a.wait_until_succeeds("[ $(find /srv/garage -type f | wc -l) -ge 3 ]")
    b.wait_until_succeeds("[ $(find /srv/garage -type f | wc -l) -ge 3 ]")

    # a is gone. b still answers, and a's repository - a's password, a's key -
    # restores a's files on b. Without the lock: a lock is a write, and the
    # cluster takes no writes with one box down
    a.shutdown()
    b.succeed(
        "set -a; . ${(storageBox "a").dd.backup.envFile}; "
        "restic -r s3:http://127.0.0.1:3900/backups-a --password-file ${(storageBox "a").dd.backup.passwordFile} --no-lock restore latest --target /tmp/restore"
    )
    b.succeed("grep -q 'worth having tomorrow' /tmp/restore/srv/state/file")
    b.succeed("[ $(stat -c %s /tmp/restore/srv/state/blob) = 2000000 ]")

    # with a down the cluster refuses new writes: two copies or nothing.
    # Asked directly, not through restic, which retries for a quarter hour
    # before it agrees
    # b learns a is gone a moment after it is; until then a write may still
    # land on a as it dies. Wait for the refusal rather than expect it at once
    b.wait_until_fails(
        "set -a; . ${(storageBox "b").dd.backup.envFile}; "
        "AWS_MAX_ATTEMPTS=1 aws --endpoint-url http://127.0.0.1:3900 s3 cp /etc/hostname s3://backups-b/probe"
    )
  '';
}
