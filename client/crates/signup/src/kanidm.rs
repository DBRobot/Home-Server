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

    async fn person_exists(&self, name: &str) -> Result<bool> {
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
    async fn is_pending(&self, group: &str, name: &str) -> Result<bool> {
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

    pub async fn sign_up(
        &self,
        group: &str,
        name: &str,
        display_name: &str,
        email: &str,
        ttl_secs: u64,
    ) -> Result<Outcome> {
        if self.person_exists(name).await? {
            if self.is_pending(group, name).await? {
                self.send_intent(name, email, ttl_secs).await?;
                return Ok(Outcome::Resent);
            }
            return Ok(Outcome::NameTaken);
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

        // Group first, mail second. If the mail fails, the account is already
        // in `pending`, which is what makes the retry path above work; the
        // other order would strand it somewhere neither branch can find.
        let r = self
            .post(&format!("/v1/group/{group}/_attr/member"), json!([name]))
            .await?;
        if !r.status().is_success() {
            let s = r.status();
            bail!("kanidm answered {s} adding the account to {group}");
        }

        self.send_intent(name, email, ttl_secs).await?;
        Ok(Outcome::Created)
    }
}
