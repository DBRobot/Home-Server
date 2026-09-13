//! The three calls this service makes, and nothing else.
//!
//! Deliberately hand-written against kanidm's REST api rather than pulled in
//! through `kanidm_client`: that crate carries most of kanidm's tree and pins
//! itself to a server version, which is a large dependency to take on for three
//! requests. The cost is that these paths and shapes are ours to keep in step
//! with the server - they come from server/core/src/https/v1.rs.

use anyhow::{Context, Result, bail};
use serde_json::json;

pub struct Client {
    http: reqwest::Client,
    base: String,
    token: String,
}

/// What a signup attempt did, so the handler can answer differently without
/// caring how it was worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Created,
    /// The account already existed but has never left `pending`, so this is a
    /// retry of one whose mail never arrived rather than a new person.
    Resent,
    NameTaken,
}

impl Client {
    pub fn new(base: String, token: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            // a redirect on an authenticated api call is not something we want
            // to follow carrying a bearer token
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { http, base, token })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base.trim_end_matches('/'), path)
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> Result<reqwest::Response> {
        self.http
            .post(self.url(path))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {path}"))
    }

    /// Whether the account already carries a credential - a passkey or a
    /// password. The worker refuses to touch such an account no matter what
    /// the spool says: an enrolled person is never a signup.
    pub async fn has_credential(&self, name: &str) -> Result<bool> {
        let r = self
            .http
            .get(self.url(&format!("/v1/person/{name}")))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("GET person")?;
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        let v: Option<serde_json::Value> = r.error_for_status()?.json().await?;
        let attrs = match v.as_ref().and_then(|v| v.get("attrs")) {
            Some(a) => a,
            None => return Ok(false),
        };
        let has = |k: &str| {
            attrs
                .get(k)
                .and_then(|x| x.as_array())
                .is_some_and(|a| !a.is_empty())
        };
        Ok(has("passkeys") || has("primary_credential") || has("attested_passkeys"))
    }

    pub async fn person_exists(&self, name: &str) -> Result<bool> {
        let r = self
            .http
            .get(self.url(&format!("/v1/person/{name}")))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("GET person")?;
        match r.status() {
            // kanidm answers 200 with a null body for a name that does not
            // exist, so the status alone does not settle it.
            reqwest::StatusCode::OK => Ok(r.json::<Option<serde_json::Value>>().await?.is_some()),
            reqwest::StatusCode::NOT_FOUND => Ok(false),
            s => bail!("kanidm answered {s} looking up a person"),
        }
    }

    /// Membership of `pending` is how a failed-mail retry is told apart from
    /// someone trying to take a name that is already in use. Anyone who has
    /// been granted access is in `users`, so they can never be reached here.
    pub async fn is_pending(&self, group: &str, name: &str) -> Result<bool> {
        let r = self
            .http
            .get(self.url(&format!("/v1/group/{group}/_attr/member")))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("GET group members")?;
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        let members: Option<Vec<String>> = r.error_for_status()?.json().await?;
        // members come back as spns - "david@idm.example.com"
        Ok(members
            .unwrap_or_default()
            .iter()
            .any(|m| m.split('@').next() == Some(name)))
    }

    /// A new account has a day to enrol. The sweep in modules/user-accounts.nix
    /// clears this the moment a credential exists and deletes the account if
    /// it expires with none - so open signup cannot pile up empty rows.
    async fn set_expiry(&self, name: &str, ttl_secs: u64) -> Result<()> {
        let when = std::time::SystemTime::now() + std::time::Duration::from_secs(ttl_secs);
        let secs = when.duration_since(std::time::UNIX_EPOCH)?.as_secs();
        let r = self
            .http
            .put(self.url(&format!("/v1/person/{name}/_attr/account_expire")))
            .bearer_auth(&self.token)
            .json(&json!([rfc3339(secs)]))
            .send()
            .await
            .context("PUT account_expire")?;
        if !r.status().is_success() {
            let s = r.status();
            bail!("kanidm answered {s} setting the enrolment deadline");
        }
        Ok(())
    }

    async fn send_intent(&self, name: &str, email: &str, ttl_secs: u64) -> Result<()> {
        let r = self
            .post(
                &format!("/v1/person/{name}/_credential/_update_intent_send"),
                json!({ "ttl": ttl_secs, "email": email }),
            )
            .await?;
        if !r.status().is_success() {
            let s = r.status();
            bail!("kanidm answered {s} queueing the enrolment mail");
        }
        Ok(())
    }

    /// What a signup for `name` would be. Needs only read access, so the
    /// internet-facing service can answer "taken" immediately without holding
    /// anything that writes. The worker runs the same check again with its
    /// own token before acting, and trusts nothing the spool says.
    pub async fn classify(&self, group: &str, name: &str) -> Result<Outcome> {
        if !self.person_exists(name).await? {
            return Ok(Outcome::Created);
        }
        if self.is_pending(group, name).await? && !self.has_credential(name).await? {
            return Ok(Outcome::Resent);
        }
        Ok(Outcome::NameTaken)
    }

    pub async fn sign_up(
        &self,
        group: &str,
        name: &str,
        display_name: &str,
        email: &str,
        ttl_secs: u64,
    ) -> Result<Outcome> {
        match self.classify(group, name).await? {
            Outcome::NameTaken => return Ok(Outcome::NameTaken),
            Outcome::Resent => {
                self.set_expiry(name, ttl_secs).await?;
                self.send_intent(name, email, ttl_secs).await?;
                return Ok(Outcome::Resent);
            }
            Outcome::Created => {}
        }

        let r = self
            .post(
                "/v1/person",
                json!({ "attrs": {
                    "name": [name],
                    "displayname": [display_name],
                    "mail": [email],
                }}),
            )
            .await?;
        if !r.status().is_success() {
            let s = r.status();
            // kanidm enforces its own name rules and denied-names list, so a
            // rejection here is usually a name we were never going to be able
            // to validate ourselves.
            if s == reqwest::StatusCode::BAD_REQUEST || s == reqwest::StatusCode::CONFLICT {
                return Ok(Outcome::NameTaken);
            }
            bail!("kanidm answered {s} creating the account");
        }

        // Group FIRST. Everything after this - the deadline, the mail - is
        // only permitted on members of `pending`, and if any of it fails the
        // account is already where the retry path above can find it.
        let r = self
            .post(&format!("/v1/group/{group}/_attr/member"), json!([name]))
            .await?;
        if !r.status().is_success() {
            let s = r.status();
            bail!("kanidm answered {s} adding the account to {group}");
        }

        self.set_expiry(name, ttl_secs).await?;
        self.send_intent(name, email, ttl_secs).await?;
        Ok(Outcome::Created)
    }
}

/// Seconds since the epoch as the rfc3339 string kanidm wants. Days-from-civil
/// is the standard algorithm; avoiding a date crate for one format.
fn rfc3339(secs: u64) -> String {
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    #[test]
    fn rfc3339_known_values() {
        assert_eq!(super::rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::rfc3339(951782400), "2000-02-29T00:00:00Z");
        assert_eq!(super::rfc3339(1_788_998_400), "2026-09-10T00:00:00Z");
    }
}
