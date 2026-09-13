# Roles

A box is its hardware plus a list of roles from `fleet/boxes.json`. A role is
a bundle of modules that have to move together, with the secrets those
modules read declared beside them. `flake.nix` builds one `nixosConfiguration`
per box from its entry; `hosts/<box>/hardware.nix` holds only what is true of
that machine (disks, network, lid).

    core      every box: identity, directory replica, metrics, tailnet, ssh, admin
    gateway   nginx, certificates, the browser login: a box with a public name
    storage   garage and the pool's datasets
    cache     harmonia, the nix binary cache
    photos    ente
    media     jellyfin, the media tier, uploads, per-user directories
    llm       llama-cpp and its gateway
    forge     forgejo, its runner, the github mirror
    observe   grafana, alerting, mail, database dumps

No box is trusted. Roles that read people's plaintext register with
`dd.box.plaintext`; the box publishes that list as a metric so the services
still to fix stay in view (modules/box.nix). Nothing refuses anything.
