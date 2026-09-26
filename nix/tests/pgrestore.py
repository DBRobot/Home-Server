# A dump that restores, then one that tries to bite whoever restores it.
box.wait_for_unit("postgresql.service")
box.wait_until_succeeds("sudo -u postgres psql -d ente -c 'select 1'")
ente = "sudo -u postgres psql -v ON_ERROR_STOP=1 -d ente"

box.succeed(f"{ente} -c 'create table key_attributes (id int); insert into key_attributes values (1); create table collection_files (id int);'")
box.succeed("systemctl start postgresqlBackup-ente.service")
box.succeed("systemctl start postgres-backup-verify.service")
assert "ok: key_attributes=1" in box.succeed("journalctl -u postgres-backup-verify -o cat")

# the role that restores can do nothing but own the scratch database
assert box.succeed("sudo -u postgres psql -tAc \"select rolsuper from pg_roles where rolname = 'ente_restore'\"").strip() == "f"

# A check that calls a function needing a superuser. pg_dump writes checks
# into the CREATE TABLE, so a restore runs it on every row it loads - with
# the restoring role's rights. Set up disarmed, so only a restore, where
# nothing sets bite.off, arms it.
box.succeed("""cat > /tmp/snare.sql <<'SQL'
create function bite(i int) returns boolean language plpgsql volatile as $$
begin
  if current_setting('bite.off', true) is distinct from 'yes' then
    execute 'copy (select 1) to program ' || quote_literal('touch /var/lib/postgresql/bitten');
  end if;
  return true;
end $$;
set bite.off = 'yes';
create table snare (id int check (bite(id)));
insert into snare values (1);
SQL""")
box.succeed(f"{ente} -f /tmp/snare.sql")
box.succeed("systemctl start postgresqlBackup-ente.service")
box.succeed("zstd -dc /vault/backups/postgres/ente.sql.zstd | grep -q 'CHECK (public.bite(id))'")

# the check fails, loudly - it is meant to mail - and nothing was run
box.fail("systemctl start postgres-backup-verify.service")
box.fail("test -e /var/lib/postgresql/bitten")
journal = box.succeed("journalctl -u postgres-backup-verify -o cat").lower()
assert "pg_execute_server_program" in journal or "must be superuser" in journal or "permission denied" in journal, journal[-600:]
