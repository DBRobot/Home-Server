#!/usr/bin/env bash
# Jellyfin's SSO plugin, configured to trust the gate.
dir=$DATA_DIR/plugins/configurations
mkdir -p "$dir"
cfg=$dir/SSO-Auth.xml
if [ -f "$cfg" ] && $GREP/bin/grep -q "<string>dd</string>" "$cfg"; then
  echo "dd provider already present; leaving config alone"
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
