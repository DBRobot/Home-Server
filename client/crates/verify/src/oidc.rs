//! A per-box OpenID Connect issuer, for the one service that speaks nothing
//! else: jellyfin's SSO plugin. Its signing key is generated here on first
//! start and trusted by exactly one client on exactly this box - so it is a
//! mint whose whole blast radius is "watch this box's movies", not a token
//! every service everywhere accepts. The person still signs in with a passkey
//! at the verifier; this only translates that session into the shape the
//! plugin insists on.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use p256::ecdsa::SigningKey;
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use serde::Serialize;

use crate::session::{now, random_id};

pub struct Issuer {
    pub issuer: String,
    pub client_id: String,
    client_secret: String,
    pub redirect_uri: String,
    key: SigningKey,
    encoding: jsonwebtoken::EncodingKey,
    kid: String,
    codes: Mutex<HashMap<String, Grant>>,
    tokens: Mutex<HashMap<String, Grant>>,
}

#[derive(Clone)]
struct Grant {
    user: String,
    nonce: Option<String>,
    issued: u64,
}

impl Issuer {
    pub fn open(
        dir: &Path,
        issuer: String,
        client_id: String,
        client_secret: String,
        redirect_uri: String,
    ) -> Result<Self> {
        let p = dir.join("oidc.key");
        let key = match std::fs::read(&p) {
            Ok(der) => {
                SigningKey::from_pkcs8_der(&der).context("oidc.key is not a pkcs8 p256 key")?
            }
            Err(_) => {
                let k = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
                let der = k.to_pkcs8_der()?;
                std::fs::write(&p, der.as_bytes()).context("writing oidc.key")?;
                k
            }
        };
        let der = key.to_pkcs8_der()?;
        let encoding = jsonwebtoken::EncodingKey::from_ec_der(der.as_bytes());
        let point = key.verifying_key().to_encoded_point(false);
        let kid: String = point
            .x()
            .unwrap()
            .iter()
            .take(6)
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(Self {
            issuer,
            client_id,
            client_secret,
            redirect_uri,
            key,
            encoding,
            kid,
            codes: Mutex::new(HashMap::new()),
            tokens: Mutex::new(HashMap::new()),
        })
    }

    pub fn discovery(&self) -> serde_json::Value {
        let i = &self.issuer;
        serde_json::json!({
            "issuer": i,
            "authorization_endpoint": format!("{i}/authorize"),
            "token_endpoint": format!("{i}/token"),
            "userinfo_endpoint": format!("{i}/userinfo"),
            "jwks_uri": format!("{i}/jwks"),
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["ES256"],
            "scopes_supported": ["openid", "profile", "email"],
            "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
            "claims_supported": ["sub", "preferred_username", "name"],
            "grant_types_supported": ["authorization_code"],
        })
    }

    pub fn jwks(&self) -> serde_json::Value {
        let point = self.key.verifying_key().to_encoded_point(false);
        serde_json::json!({ "keys": [{
            "kty": "EC", "crv": "P-256", "use": "sig", "alg": "ES256", "kid": self.kid,
            "x": B64.encode(point.x().unwrap()),
            "y": B64.encode(point.y().unwrap()),
        }]})
    }

    /// An authorization code for a user who has just proven a session.
    pub fn code(&self, user: &str, nonce: Option<String>) -> String {
        let code = random_id();
        self.codes.lock().unwrap().insert(
            code.clone(),
            Grant {
                user: user.to_string(),
                nonce,
                issued: now(),
            },
        );
        code
    }

    pub fn client_ok(&self, id: &str, secret: &str) -> bool {
        id == self.client_id && secret == self.client_secret
    }

    /// Redeem a code: an id token plus an opaque access token for userinfo.
    pub fn redeem(&self, code: &str) -> Option<serde_json::Value> {
        let grant = self.codes.lock().unwrap().remove(code)?;
        if now() > grant.issued + 300 {
            return None;
        }
        #[derive(Serialize)]
        struct Claims<'a> {
            iss: &'a str,
            sub: &'a str,
            aud: &'a str,
            exp: u64,
            iat: u64,
            preferred_username: &'a str,
            name: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            nonce: Option<String>,
        }
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
        header.kid = Some(self.kid.clone());
        let id_token = jsonwebtoken::encode(
            &header,
            &Claims {
                iss: &self.issuer,
                sub: &grant.user,
                aud: &self.client_id,
                exp: now() + 3600,
                iat: now(),
                preferred_username: &grant.user,
                name: &grant.user,
                nonce: grant.nonce.clone(),
            },
            &self.encoding,
        )
        .ok()?;
        let access = random_id();
        self.tokens.lock().unwrap().insert(access.clone(), grant);
        Some(serde_json::json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": 3600,
            "id_token": id_token,
        }))
    }

    pub fn userinfo(&self, access: &str) -> Option<serde_json::Value> {
        let g = self.tokens.lock().unwrap().get(access).cloned()?;
        if now() > g.issued + 3600 {
            return None;
        }
        Some(serde_json::json!({ "sub": g.user, "preferred_username": g.user, "name": g.user }))
    }
}
