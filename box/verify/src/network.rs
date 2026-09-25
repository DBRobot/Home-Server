//! The network's door: an admitted device asks for a key to join the
//! fleet's own network, the gate asks Headscale for one in that member's
//! name, single use, minutes to live. The control server itself is behind
//! the same host; this only vouches for who is asking.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};

use crate::App;

/// how long a join key is good for: long enough to hand it over, no more
const TTL: Duration = Duration::from_secs(600);

pub struct Door {
    /// headscale's api, on this box
    api: String,
    /// what a device is told to join
    control_url: String,
    key: String,
    http: reqwest::Client,
}

pub fn from_env() -> Result<Option<Door>> {
    let Ok(api) = std::env::var("VERIFY_HEADSCALE_API") else {
        return Ok(None);
    };
    let key = std::fs::read_to_string(
        std::env::var("VERIFY_HEADSCALE_KEY_FILE").context("VERIFY_HEADSCALE_KEY_FILE")?,
    )
    .context("reading the headscale api key")?;
    Ok(Some(Door {
        api: api.trim_end_matches('/').to_string(),
        control_url: std::env::var("VERIFY_HEADSCALE_URL").context("VERIFY_HEADSCALE_URL")?,
        key: key.trim().to_string(),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?,
    }))
}

impl Door {
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let mut r = self
            .http
            .request(method, format!("{}{path}", self.api))
            .bearer_auth(&self.key);
        if let Some(b) = body {
            r = r.json(&b);
        }
        let r = r.send().await?;
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("headscale: {status} {text}");
        }
        Ok(serde_json::from_str(&text).unwrap_or(serde_json::Value::Null))
    }

    /// the member's user in headscale, made on first ask
    async fn user_id(&self, name: &str) -> Result<u64> {
        let v = self
            .call(
                reqwest::Method::GET,
                &format!("/api/v1/user?name={name}"),
                None,
            )
            .await?;
        // matched by name here: the filter is not trusted to
        if let Some(u) = v["users"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|u| u["name"].as_str() == Some(name))
        {
            return id_of(u);
        }
        let v = self
            .call(
                reqwest::Method::POST,
                "/api/v1/user",
                Some(serde_json::json!({ "name": name })),
            )
            .await?;
        id_of(&v["user"])
    }

    /// The machines this member has on the network, as the control
    /// server has them. Read only: a page shows them, nothing here takes
    /// one away.
    pub async fn mine(&self, name: &str) -> Result<Vec<serde_json::Value>> {
        let v = self
            .call(
                reqwest::Method::GET,
                &format!("/api/v1/node?user={name}"),
                None,
            )
            .await?;
        Ok(v["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            // headscale's filter is not trusted to: match the owner here
            .filter(|n| n["user"]["name"].as_str() == Some(name))
            .map(|n| {
                serde_json::json!({
                    "name": n["givenName"].as_str().or(n["name"].as_str()),
                    "addresses": n["ipAddresses"],
                    "lastSeen": n["lastSeen"],
                    "online": n["online"],
                    "os": n["hostinfo"]["OS"],
                })
            })
            .collect())
    }

    /// one key, one device, ten minutes
    pub async fn join_key(&self, name: &str) -> Result<(String, u64)> {
        let user = self.user_id(name).await?;
        let expires = SystemTime::now() + TTL;
        let stamp = humantime(expires);
        let v = self
            .call(
                reqwest::Method::POST,
                "/api/v1/preauthkey",
                Some(serde_json::json!({
                    "user": user,
                    "reusable": false,
                    "ephemeral": false,
                    "expiration": stamp
                })),
            )
            .await?;
        let key = v["preAuthKey"]["key"]
            .as_str()
            .context("headscale gave no key")?
            .to_string();
        Ok((
            key,
            expires
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        ))
    }
}

/// headscale gives ids as strings in json
fn id_of(u: &serde_json::Value) -> Result<u64> {
    match &u["id"] {
        serde_json::Value::String(s) => s.parse().context("user id"),
        serde_json::Value::Number(n) => n.as_u64().context("user id"),
        _ => bail!("headscale gave no user id"),
    }
}

/// RFC 3339, whole seconds, for a timestamp field
fn humantime(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d, hh, mm, ss) = civil(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

fn civil(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = secs / 86400;
    let rem = secs % 86400;
    // days since 1970-01-01 to civil date (Howard Hinnant's algorithm)
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (
        y as u64,
        m as u64,
        d as u64,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
    )
}

/// POST /_dd/network/join, bearer device token -> { control_url, key, expires }
pub(crate) async fn join(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(door) = app.network.as_ref() else {
        return (StatusCode::NOT_FOUND, "this box has no network door").into_response();
    };
    let Some(token) = crate::bearer(&headers) else {
        return (StatusCode::UNAUTHORIZED, "a device token is required").into_response();
    };
    let user = match app.verify_biscuit(token, "access") {
        Ok(u) => u,
        Err(e) => return (StatusCode::UNAUTHORIZED, e.to_string()).into_response(),
    };
    match door.join_key(&user).await {
        Ok((key, expires)) => Json(serde_json::json!({
            "control_url": door.control_url,
            "key": key,
            "expires": expires,
            "user": user
        }))
        .into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, format!("{e:#}")).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps() {
        assert_eq!(humantime(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            humantime(UNIX_EPOCH + Duration::from_secs(1_790_000_000)),
            "2026-09-21T14:13:20Z"
        );
    }
}
