//! 身分服務（Google OIDC，S-08.2）。以 trait 包裝，測試時換成假實作。

use async_trait::async_trait;
use openidconnect::core::{CoreClient, CoreProviderMetadata, CoreResponseType};
use openidconnect::reqwest;
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, TokenResponse,
};

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
    /// 以授權碼換 token 並驗證 ID token（簽章、iss、aud、exp、nonce）。
    async fn finish(
        &self,
        code: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<Identity, IdentityError>;
}

type GoogleClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

pub struct GoogleIdentity {
    client: GoogleClient,
    http: reqwest::Client,
}

impl GoogleIdentity {
    /// 啟動時探索 Google 的 OIDC 設定；失敗就不該啟動服務。
    pub async fn discover(
        client_id: &str,
        client_secret: &str,
        redirect_url: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // 不跟隨轉址，避免 SSRF
        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let issuer = IssuerUrl::new("https://accounts.google.com".to_string())?;
        let metadata = CoreProviderMetadata::discover_async(issuer, &http).await?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(client_id.to_string()),
            Some(ClientSecret::new(client_secret.to_string())),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_url.to_string())?);
        Ok(Self { client, http })
    }
}

#[async_trait]
impl IdentityProvider for GoogleIdentity {
    fn start(&self) -> AuthRequest {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = self
            .client
            .authorize_url(
                AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("profile".to_string()))
            .set_pkce_challenge(challenge)
            .url();
        AuthRequest {
            url: url.to_string(),
            state: state.secret().clone(),
            nonce: nonce.secret().clone(),
            pkce_verifier: verifier.secret().clone(),
        }
    }

    async fn finish(
        &self,
        code: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<Identity, IdentityError> {
        let token = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .map_err(|_| IdentityError)?
            .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_string()))
            .request_async(&self.http)
            .await
            .map_err(|_| IdentityError)?;
        let id_token = token.id_token().ok_or(IdentityError)?;
        let verifier = self.client.id_token_verifier();
        let claims = id_token
            .claims(&verifier, &Nonce::new(nonce.to_string()))
            .map_err(|_| IdentityError)?;
        Ok(Identity {
            sub: claims.subject().to_string(),
            email: claims.email().map(|e| e.to_string()).ok_or(IdentityError)?,
            email_verified: claims.email_verified().unwrap_or(false),
            name: claims
                .name()
                .and_then(|n| n.get(None))
                .map(|n| n.to_string()),
        })
    }
}
