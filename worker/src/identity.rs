//! 身分服務（Google OIDC，S-08.2）。以 trait 包裝，測試時換成假實作。
//!
//! Workers 版不用 `openidconnect`（依賴 reqwest 與 Tokio），直接實作授權碼流程：PKCE（S256）、state、nonce。
//! ID token 由後端以 client secret 直接向 Google 的 token 端點換得（TLS），依 OIDC Core 3.1.3.7 第 6 點，
//! 這種情況可以用 TLS 伺服器驗證取代簽章驗證；仍檢查 `iss`、`aud`、`exp` 與 `nonce`。

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];

/// 導向 Google 授權頁所需的資料；`state`、`nonce`、`pkce_verifier` 需暫存到資料庫。
#[derive(Debug, Clone)]
pub struct AuthRequest {
    pub url: String,
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
}

/// 通過 ID token 驗證後的 Google 身分。
#[derive(Debug, Clone)]
pub struct Identity {
    pub sub: String,
    pub email: String,
    pub email_verified: bool,
    pub name: Option<String>,
}

#[derive(Debug)]
pub struct IdentityError;

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    fn start(&self) -> AuthRequest;
    /// 以授權碼換 token 並驗證 ID token（iss、aud、exp、nonce）。
    async fn finish(
        &self,
        code: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<Identity, IdentityError>;
}

pub struct GoogleIdentity {
    client_id: String,
    client_secret: String,
    redirect_url: String,
    http: crate::http::Client,
}

impl GoogleIdentity {
    pub fn new(client_id: &str, client_secret: &str, redirect_url: &str) -> Self {
        Self {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_url: redirect_url.to_string(),
            // 不跟隨轉址，避免 SSRF
            http: crate::http::Client::new()
                .without_redirects()
                .with_timeout(std::time::Duration::from_secs(10)),
        }
    }
}

/// 32 個隨機位元組的 base64url（state、nonce、PKCE verifier）。
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("crypto.getRandomValues is available");
    URL_SAFE_NO_PAD.encode(bytes)
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: Audience,
    exp: i64,
    sub: String,
    nonce: Option<String>,
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<BoolOrString>,
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BoolOrString {
    Bool(bool),
    Text(String),
}

/// 檢查 ID token 的 claims；回傳身分或錯誤。
fn verify_claims(
    id_token: &str,
    client_id: &str,
    nonce: &str,
    now_secs: i64,
) -> Result<Identity, IdentityError> {
    let payload = id_token.split('.').nth(1).ok_or(IdentityError)?;
    let raw = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| IdentityError)?;
    let claims: Claims = serde_json::from_slice(&raw).map_err(|_| IdentityError)?;
    let aud_ok = match &claims.aud {
        Audience::One(a) => a == client_id,
        Audience::Many(list) => list.iter().any(|a| a == client_id),
    };
    if !ISSUERS.contains(&claims.iss.as_str())
        || !aud_ok
        || claims.exp <= now_secs
        || claims.nonce.as_deref() != Some(nonce)
    {
        return Err(IdentityError);
    }
    Ok(Identity {
        sub: claims.sub,
        email: claims.email.ok_or(IdentityError)?,
        email_verified: match claims.email_verified {
            Some(BoolOrString::Bool(b)) => b,
            Some(BoolOrString::Text(s)) => s == "true",
            None => false,
        },
        name: claims.name,
    })
}

#[async_trait]
impl IdentityProvider for GoogleIdentity {
    fn start(&self) -> AuthRequest {
        let state = random_token();
        let nonce = random_token();
        let verifier = random_token();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let query = serde_urlencoded::to_string([
            ("response_type", "code"),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect_url.as_str()),
            ("scope", "openid email profile"),
            ("state", state.as_str()),
            ("nonce", nonce.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ])
        .unwrap_or_default();
        AuthRequest {
            url: format!("{AUTH_ENDPOINT}?{query}"),
            state,
            nonce,
            pkce_verifier: verifier,
        }
    }

    async fn finish(
        &self,
        code: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<Identity, IdentityError> {
        let res = self
            .http
            .post(TOKEN_ENDPOINT)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.redirect_url.as_str()),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("code_verifier", pkce_verifier),
            ])
            .send()
            .await
            .map_err(|_| IdentityError)?;
        if !res.status().is_success() {
            tracing::warn!(status = %res.status(), "google token endpoint returned an error status");
            return Err(IdentityError);
        }
        let token: TokenResponse = res.json().await.map_err(|_| IdentityError)?;
        let id_token = token.id_token.ok_or(IdentityError)?;
        verify_claims(
            &id_token,
            &self.client_id,
            nonce,
            chrono::Utc::now().timestamp(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(claims: serde_json::Value) -> String {
        format!(
            "e30.{}.sig",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    #[test]
    fn verifies_issuer_audience_expiry_and_nonce() {
        let good = serde_json::json!({
            "iss": "https://accounts.google.com", "aud": "cid", "exp": 2000, "sub": "s1",
            "nonce": "n", "email": "a@example.com", "email_verified": true, "name": "A"
        });
        let id = verify_claims(&token(good.clone()), "cid", "n", 1000).unwrap();
        assert_eq!((id.sub.as_str(), id.email_verified), ("s1", true));
        for (field, value) in [
            ("iss", serde_json::json!("https://evil.example")),
            ("aud", serde_json::json!("other")),
            ("exp", serde_json::json!(999)),
            ("nonce", serde_json::json!("x")),
        ] {
            let mut bad = good.clone();
            bad[field] = value;
            assert!(
                verify_claims(&token(bad), "cid", "n", 1000).is_err(),
                "{field}"
            );
        }
    }
}
