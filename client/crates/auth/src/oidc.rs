use openidconnect::core::{CoreClient, CoreProviderMetadata, CoreResponseType};
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, RedirectUrl, Scope, TokenResponse,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;

use crate::{Error, Result};

/// What a successful kanidm login yields. Deliberately not the ente key -
/// these tokens authorise services, they do not decrypt anything.
pub struct Session {
    pub access_token: Zeroizing<String>,
    pub id_token: Option<String>,
    pub subject: String,
    pub preferred_username: Option<String>,
}

/// Authorization code flow with PKCE, redirecting to a loopback listener.
///
/// This is the "public client" shape: no client secret exists, because a
/// secret shipped in a binary on someone's laptop is not a secret. PKCE is
/// what stops an intercepted code being redeemed by anyone else.
pub async fn login(issuer: &str, client_id: &str) -> Result<Session> {
    let http = reqwest::Client::builder()
        // a redirect during token exchange would be an attack, not a feature
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| Error::Discovery(e.to_string()))?;

    let issuer_url =
        IssuerUrl::new(issuer.to_string()).map_err(|e| Error::Discovery(e.to_string()))?;
    let metadata = CoreProviderMetadata::discover_async(issuer_url, &http)
        .await
        .map_err(|e| Error::Discovery(e.to_string()))?;

    // Port 0 lets the OS pick, so two logins at once do not collide.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");

    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(client_id.to_string()),
        None, // public client: no secret
    )
    .set_redirect_uri(
        RedirectUrl::new(redirect.clone()).map_err(|e| Error::Discovery(e.to_string()))?,
    );

    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf, nonce) = client
        .authorize_url(
            AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("groups".to_string()))
        .set_pkce_challenge(challenge)
        .url();

    println!("open this to sign in:\n  {auth_url}\n");

    let (code, state) = wait_for_callback(&listener).await?;
    if state != *csrf.secret() {
        return Err(Error::StateMismatch);
    }

    let tokens = client
        .exchange_code(AuthorizationCode::new(code))
        .map_err(|e| Error::Exchange(e.to_string()))?
        .set_pkce_verifier(verifier)
        .request_async(&http)
        .await
        .map_err(|e| Error::Exchange(e.to_string()))?;

    let id_token = tokens.id_token();
    let claims = id_token
        .map(|t| {
            t.claims(&client.id_token_verifier(), &nonce)
                .map_err(|e| Error::Exchange(e.to_string()))
        })
        .transpose()?;

    Ok(Session {
        access_token: Zeroizing::new(tokens.access_token().secret().to_string()),
        id_token: id_token.map(|t| t.to_string()),
        subject: claims
            .map(|c| c.subject().to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        preferred_username: claims.and_then(|c| c.preferred_username().map(|u| u.to_string())),
    })
}

/// Accept exactly one request, pull code and state out of the query string.
async fn wait_for_callback(listener: &tokio::net::TcpListener) -> Result<(String, String)> {
    let (mut sock, _) = listener.accept().await?;

    let mut buf = [0u8; 2048];
    let n = sock.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let target = request
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("");
    // the base is a throwaway - Url needs an absolute one to parse a path
    let parsed =
        url::Url::parse(&format!("http://localhost{target}")).map_err(|_| Error::NoCode)?;

    let mut code = None;
    let mut state = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            _ => {}
        }
    }

    let body = if code.is_some() {
        "<h1>Signed in</h1><p>You can close this tab.</p>"
    } else {
        "<h1>Sign in failed</h1><p>No authorization code came back.</p>"
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    sock.write_all(response.as_bytes()).await.ok();
    sock.flush().await.ok();

    Ok((code.ok_or(Error::NoCode)?, state.unwrap_or_default()))
}
