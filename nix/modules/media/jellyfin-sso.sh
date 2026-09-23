#!/usr/bin/env bash
# Jellyfin's SSO plugin, configured to trust the gate.
dir=$DATA_DIR/plugins/configurations
mkdir -p "$dir"
cfg=$dir/SSO-Auth.xml
want="https://jellyfin.$BASE/_dd/oidc"
if [ -f "$cfg" ] && $GREP/bin/grep -q "<string>dd</string>" "$cfg"; then
  # the provider is there; only its endpoint may have moved (a domain
  # change). Everything else in the file, the account links above all,
  # is left as it is.
  if $GREP/bin/grep -q "<OidEndpoint>$want</OidEndpoint>" "$cfg"; then
    echo "dd provider already present; leaving config alone"
  else
    $SED/bin/sed -i "s|<OidEndpoint>[^<]*</OidEndpoint>|<OidEndpoint>$want</OidEndpoint>|" "$cfg"
    echo "dd provider endpoint moved to $want"
  fi
  exit 0
fi
secret=$(cat $OAUTH_SECRET_FILE)
cat > "$cfg" <<XML
<?xml version="1.0" encoding="utf-8"?>
<PluginConfiguration xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">
  <SamlConfigs />
  <OidConfigs>
    <item>
      <key>
        <string>dd</string>
      </key>
      <value>
        <PluginConfiguration>
          <OidEndpoint>https://jellyfin.$BASE/_dd/oidc</OidEndpoint>
          <OidClientId>jellyfin</OidClientId>
          <OidSecret>$secret</OidSecret>
          <Enabled>true</Enabled>
          <EnableAuthorization>false</EnableAuthorization>
          <EnableAllFolders>true</EnableAllFolders>
          <EnabledFolders />
          <AdminRoles />
          <Roles />
          <EnableFolderRoles>false</EnableFolderRoles>
          <EnableLiveTvRoles>false</EnableLiveTvRoles>
          <EnableLiveTv>false</EnableLiveTv>
          <EnableLiveTvManagement>false</EnableLiveTvManagement>
          <LiveTvRoles />
          <LiveTvManagementRoles />
          <FolderRoleMappings />
          <OidScopes />
          <PortOverride xsi:nil="true" />
          <SchemeOverride>https</SchemeOverride>
          <NewPath>false</NewPath>
          <CanonicalLinks />
          <DisableHttps>false</DisableHttps>
          <DisablePushedAuthorization>false</DisablePushedAuthorization>
          <DoNotValidateEndpoints>false</DoNotValidateEndpoints>
          <DoNotValidateIssuerName>false</DoNotValidateIssuerName>
        </PluginConfiguration>
      </value>
    </item>
  </OidConfigs>
</PluginConfiguration>
XML
$SED/bin/sed -i "s/^      //" "$cfg"
chown jellyfin:$GROUP "$cfg"
chmod 0600 "$cfg"
