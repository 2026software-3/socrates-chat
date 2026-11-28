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
        crate::telemetry::in_component_span("auth", "google_oidc_exchange", async {
            let token = self
                .client
                .exchange_code(AuthorizationCode::new(code.to_string()))
                .map_err(|_| {
                    tracing::warn!(
                        stage = "token_endpoint_missing",
                        "google oidc exchange failed"
                    );
                    IdentityError
                })?
                .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_string()))
                .request_async(&self.http)
                .await
                .map_err(|_| {
                    tracing::warn!(stage = "token_exchange", "google oidc exchange failed");
                    IdentityError
                })?;
            let id_token = token.id_token().ok_or_else(|| {
                tracing::warn!(stage = "missing_id_token", "google oidc exchange failed");
                IdentityError
            })?;
            let verifier = self.client.id_token_verifier();
            let claims = id_token
                .claims(&verifier, &Nonce::new(nonce.to_string()))
                .map_err(|_| {
                    tracing::warn!(
                        stage = "id_token_verification",
                        "google oidc exchange failed"
                    );
                    IdentityError
                })?;
            let email = claims.email().map(|e| e.to_string()).ok_or_else(|| {
                tracing::warn!(stage = "missing_email", "google oidc exchange failed");
                IdentityError
            })?;
            Ok(Identity {
                sub: claims.subject().to_string(),
                email,
                email_verified: claims.email_verified().unwrap_or(false),
                name: claims
                    .name()
                    .and_then(|n| n.get(None))
                    .map(|n| n.to_string()),
            })
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    use axum::{Router, http::StatusCode, routing::post};
    use serde_json::{Value, json};
    use tracing::Instrument;
    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Sink;
        fn make_writer(&'a self) -> Sink {
            Sink(self.0.clone())
        }
    }

    #[tokio::test]
    async fn finish_runs_in_auth_component_span_and_never_logs_oauth_secrets() {
        use axum::{Json, response::IntoResponse};
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        let captured = Captured::default();
        let subscriber = crate::logging::json_subscriber()
            .with_env_filter(crate::logging::env_filter(Some("info")))
            .with_writer(captured.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        const SECRET_CODE: &str = "synthetic-oauth-auth-code-12345";
        const SECRET_NONCE: &str = "synthetic-oauth-nonce-67890";
        const SECRET_VERIFIER: &str = "synthetic-pkce-verifier-abcdef";
        const SECRET_CLIENT_SECRET: &str = "synthetic-client-secret-xyz";
        const SECRET_UPSTREAM_ERROR: &str = "SENSITIVE-UPSTREAM-OIDC-ERROR-BODY";
        const SECRET_ACCESS_TOKEN: &str = "SENSITIVE-OAUTH-ACCESS-TOKEN-777";
        const SECRET_EMAIL: &str = "secret-oidc-student@example.com";
        const SECRET_SUB: &str = "secret-oidc-subject-888";

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fake_header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","kid":"missing-key"}"#);
        let fake_claims = URL_SAFE_NO_PAD.encode(
            json!({
                "iss": format!("http://{addr}"),
                "sub": SECRET_SUB,
                "aud": "synthetic-client-id",
                "exp": 9_999_999_999u64,
                "iat": 1_700_000_000u64,
                "nonce": SECRET_NONCE,
                "email": SECRET_EMAIL,
                "email_verified": true
            })
            .to_string(),
        );
        let fake_id_token = format!("{fake_header}.{fake_claims}.AAAA");
        let app = Router::new()
            .route(
                "/token-error",
                post(|| async { (StatusCode::BAD_REQUEST, SECRET_UPSTREAM_ERROR) }),
            )
            .route(
                "/token-no-id",
                post(|| async {
                    Json(json!({
                        "access_token": SECRET_ACCESS_TOKEN,
                        "token_type": "Bearer"
                    }))
                    .into_response()
                }),
            )
            .route(
                "/token-bad-id",
                post({
                    let fake_id_token = fake_id_token.clone();
                    move || {
                        let fake_id_token = fake_id_token.clone();
                        async move {
                            Json(json!({
                                "access_token": SECRET_ACCESS_TOKEN,
                                "token_type": "Bearer",
                                "id_token": fake_id_token
                            }))
                            .into_response()
                        }
                    }
                }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let build_provider = |token_path: Option<&str>| {
            let mut raw = json!({
                "issuer": format!("http://{addr}"),
                "authorization_endpoint": format!("http://{addr}/auth"),
                "jwks_uri": format!("http://{addr}/jwks"),
                "response_types_supported": ["code"],
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["RS256"]
            });
            if let Some(path) = token_path {
                raw["token_endpoint"] = json!(format!("http://{addr}{path}"));
            }
            let metadata: CoreProviderMetadata = serde_json::from_value(raw).unwrap();
            let client = CoreClient::from_provider_metadata(
                metadata,
                ClientId::new("synthetic-client-id".to_string()),
                Some(ClientSecret::new(SECRET_CLIENT_SECRET.to_string())),
            )
            .set_redirect_uri(
                RedirectUrl::new("https://app.test/api/auth/google/callback".into()).unwrap(),
            );
            let http = reqwest::ClientBuilder::new()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()
                .unwrap();
            GoogleIdentity { client, http }
        };

        let parent_trace = crate::telemetry::TraceContext::new_root();
        let expected_trace_id = parent_trace.trace_id.clone();
        let expected_parent_span = parent_trace.span_id.clone();
        let req_id = uuid::Uuid::new_v4().to_string();
        let expected_req_id = req_id.clone();
        let root_span = tracing::info_span!(
            "http_request",
            request_id = %req_id,
            trace_id = %parent_trace.trace_id,
            span_id = %parent_trace.span_id,
            method = "GET",
            route = "/api/auth/google/callback"
        );

        let providers = [
            build_provider(None),
            build_provider(Some("/token-error")),
            build_provider(Some("/token-no-id")),
            build_provider(Some("/token-bad-id")),
        ];

        crate::error::REQUEST_ID
            .scope(
                req_id,
                crate::telemetry::scope_trace(
                    parent_trace,
                    async {
                        for provider in &providers {
                            assert!(
                                provider
                                    .finish(SECRET_CODE, SECRET_NONCE, SECRET_VERIFIER)
                                    .await
                                    .is_err()
                            );
                        }
                    }
                    .instrument(root_span),
                ),
            )
            .await;

        server.abort();

        let out = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        for secret in [
            SECRET_CODE,
            SECRET_NONCE,
            SECRET_VERIFIER,
            SECRET_CLIENT_SECRET,
            SECRET_UPSTREAM_ERROR,
            SECRET_ACCESS_TOKEN,
            SECRET_EMAIL,
            SECRET_SUB,
            &fake_id_token,
        ] {
            assert!(!out.contains(secret), "secret leaked into logs: {secret}");
        }

        let entries: Vec<Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let expected_stages = [
            "token_endpoint_missing",
            "token_exchange",
            "missing_id_token",
            "id_token_verification",
        ];
        assert_eq!(entries.len(), expected_stages.len());
        for (entry, expected_stage) in entries.iter().zip(expected_stages) {
            assert_eq!(entry["level"], "WARN");
            assert_eq!(entry["fields"]["message"], "google oidc exchange failed");
            assert_eq!(entry["fields"]["stage"], expected_stage);
            assert_eq!(entry["span"]["component"], "auth");
            assert_eq!(entry["span"]["operation"], "google_oidc_exchange");
            assert_eq!(entry["span"]["trace_id"], expected_trace_id);
            assert_eq!(entry["span"]["parent_span_id"], expected_parent_span);
            assert_eq!(entry["span"]["request_id"], expected_req_id);

            let spans = entry["spans"].as_array().expect("span tree");
            assert_eq!(spans.len(), 2);
            assert_eq!(spans[0]["name"], "http_request");
            assert_eq!(spans[1]["name"], "component");
            assert_eq!(spans[1]["component"], "auth");
        }
    }
}
