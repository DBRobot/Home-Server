use serde::Deserialize;

use crate::Error;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SrpAttributes {
    #[serde(rename = "srpUserID")]
    pub srp_user_id: String,
    pub srp_salt: String,
    pub kek_salt: String,
    pub mem_limit: u64,
    pub ops_limit: u64,
    #[serde(default)]
    pub is_email_mfa_enabled: bool,
}

#[derive(Deserialize)]
struct Envelope {
    attributes: SrpAttributes,
}

pub async fn srp_attributes(base: &str, email: &str) -> Result<SrpAttributes, Error> {
    let response = reqwest::Client::new()
        .get(format!("{base}/users/srp/attributes"))
        .query(&[("email", email)])
        .send()
        .await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(Error::UnknownAccount);
    }

    let envelope: Envelope = response.error_for_status()?.json().await?;
    Ok(envelope.attributes)
}
