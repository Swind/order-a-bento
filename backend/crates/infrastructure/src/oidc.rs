//! Generic OpenID Connect adapter (Authorization Code + PKCE).
//!
//! Provider discovery is lazy: the first login attempt fetches the issuer's metadata
//! and the result is cached, so the server starts (and tests run) offline.

use application::ports::{IdentityError, IdentityProvider, VerifiedIdentity};
use async_trait::async_trait;
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, reqwest,
};
use tokio::sync::OnceCell;

pub struct OidcProvider {
    client_id: ClientId,
    client_secret: Option<ClientSecret>,
    additional_audiences: Vec<String>,
    redirect_url: RedirectUrl,
    issuer: IssuerUrl,
    http: reqwest::Client,
    metadata: OnceCell<CoreProviderMetadata>,
}

fn perr(e: impl std::fmt::Display) -> IdentityError {
    IdentityError::Provider(e.to_string())
}

impl OidcProvider {
    /// `redirect_url` must equal `APP_BASE_URL + /auth/oidc/callback`.
    pub fn new(
        client_id: &str,
        client_secret: Option<&str>,
        redirect_url: &str,
        issuer: &str,
    ) -> Result<Self, IdentityError> {
        let http = reqwest::ClientBuilder::new()
            // Following redirects would open the client up to SSRF.
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(perr)?;
        Ok(Self {
            client_id: ClientId::new(client_id.to_owned()),
            client_secret: client_secret.map(|s| ClientSecret::new(s.to_owned())),
            additional_audiences: Vec::new(),
            redirect_url: RedirectUrl::new(redirect_url.to_owned()).map_err(perr)?,
            issuer: IssuerUrl::new(issuer.to_owned()).map_err(perr)?,
            http,
            metadata: OnceCell::new(),
        })
    }

    /// Pre-seed discovery metadata (used by tests to avoid the network).
    pub fn with_metadata(self, metadata: CoreProviderMetadata) -> Self {
        let _ = self.metadata.set(metadata);
        self
    }

    /// Exact extra audiences trusted for this issuer; the client ID remains required.
    pub fn with_additional_audiences(mut self, audiences: Vec<String>) -> Self {
        self.additional_audiences = audiences;
        self
    }

    async fn metadata(&self) -> Result<&CoreProviderMetadata, IdentityError> {
        self.metadata
            .get_or_try_init(|| async {
                CoreProviderMetadata::discover_async(self.issuer.clone(), &self.http)
                    .await
                    .map_err(perr)
            })
            .await
    }

    async fn client(
        &self,
    ) -> Result<
        CoreClient<
            openidconnect::EndpointSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointMaybeSet,
            openidconnect::EndpointMaybeSet,
        >,
        IdentityError,
    > {
        let meta = self.metadata().await?.clone();
        Ok(CoreClient::from_provider_metadata(
            meta,
            self.client_id.clone(),
            self.client_secret.clone(),
        )
        .set_redirect_uri(self.redirect_url.clone()))
    }
}

#[async_trait]
impl IdentityProvider for OidcProvider {
    async fn authorize_url(
        &self,
        state: &str,
        nonce: &str,
        pkce_challenge: &str,
    ) -> Result<String, IdentityError> {
        let client = self.client().await?;
        let (state, nonce) = (state.to_owned(), nonce.to_owned());
        let (url, _, _) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                move || CsrfToken::new(state),
                move || Nonce::new(nonce),
            )
            .add_scope(Scope::new("email".into()))
            .add_scope(Scope::new("profile".into()))
            .add_extra_param("code_challenge", pkce_challenge)
            .add_extra_param("code_challenge_method", "S256")
            .url();
        Ok(url.to_string())
    }

    async fn exchange(
        &self,
        code: &str,
        pkce_verifier: &str,
        nonce: &str,
    ) -> Result<VerifiedIdentity, IdentityError> {
        let client = self.client().await?;
        let token = client
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .map_err(perr)?
            .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_owned()))
            .request_async(&self.http)
            .await
            .map_err(perr)?;
        let id_token = token
            .id_token()
            .ok_or_else(|| perr("token response has no id_token"))?;
        let verifier = client
            .id_token_verifier()
            .set_other_audience_verifier_fn(|aud| {
                self.additional_audiences
                    .iter()
                    .any(|trusted| trusted == aud.as_str())
            });
        // Verifies signature, iss, aud, exp and nonce.
        let claims = id_token
            .claims(&verifier, &Nonce::new(nonce.to_owned()))
            .map_err(perr)?;
        // openidconnect 4 does not check azp. Bind multi-audience tokens to this client.
        match claims.authorized_party() {
            Some(azp) if azp.as_str() != self.client_id.as_str() => {
                return Err(perr("ID token authorized party does not match client ID"));
            }
            None if claims.audiences().len() > 1 => {
                return Err(perr(
                    "multi-audience ID token has no authorized party (azp)",
                ));
            }
            _ => {}
        }
        if let Some(expected) = claims.access_token_hash() {
            let actual = AccessTokenHash::from_token(
                token.access_token(),
                id_token.signing_alg().map_err(perr)?,
                id_token.signing_key(&verifier).map_err(perr)?,
            )
            .map_err(perr)?;
            if actual != *expected {
                return Err(perr("access token hash mismatch"));
            }
        }
        // Standard providers may return profile/email only from UserInfo.
        // Bind that response to the already verified ID-token subject.
        let userinfo: Option<openidconnect::core::CoreUserInfoClaims> = if claims.email().is_none()
        {
            Some(
                client
                    .user_info(token.access_token().clone(), Some(claims.subject().clone()))
                    .map_err(perr)?
                    .request_async(&self.http)
                    .await
                    .map_err(perr)?,
            )
        } else {
            None
        };
        let email = userinfo
            .as_ref()
            .and_then(|u| u.email())
            .or_else(|| claims.email())
            .map(|e| e.as_str().to_owned())
            .ok_or_else(|| perr("provider has no email claim"))?;
        Ok(VerifiedIdentity {
            issuer: claims.issuer().as_str().to_owned(),
            sub: claims.subject().as_str().to_owned(),
            email,
            email_verified: userinfo
                .as_ref()
                .map(|u| u.email_verified().unwrap_or(false))
                .unwrap_or_else(|| claims.email_verified().unwrap_or(false)),
            name: userinfo
                .as_ref()
                .and_then(|u| u.name())
                .or_else(|| claims.name())
                .and_then(|n| n.get(None))
                .map(|n| n.as_str().to_owned()),
            picture: userinfo
                .as_ref()
                .and_then(|u| u.picture())
                .or_else(|| claims.picture())
                .and_then(|p| p.get(None))
                .map(|p| p.as_str().to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use openidconnect::core::{
        CoreJwsSigningAlgorithm, CoreProviderMetadata, CoreResponseType, CoreSubjectIdentifierType,
    };
    use openidconnect::{
        AuthUrl, EmptyAdditionalProviderMetadata, JsonWebKeySetUrl, ResponseTypes, TokenUrl,
    };

    use super::*;

    fn provider() -> OidcProvider {
        let meta = CoreProviderMetadata::new(
            IssuerUrl::new("https://accounts.example".into()).unwrap(),
            AuthUrl::new("https://accounts.example/auth".into()).unwrap(),
            JsonWebKeySetUrl::new("https://accounts.example/jwks".into()).unwrap(),
            vec![ResponseTypes::new(vec![CoreResponseType::Code])],
            vec![CoreSubjectIdentifierType::Public],
            vec![CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256],
            EmptyAdditionalProviderMetadata {},
        )
        .set_token_endpoint(Some(
            TokenUrl::new("https://accounts.example/token".into()).unwrap(),
        ));
        OidcProvider::new(
            "cid",
            None,
            "http://localhost:3000/auth/oidc/callback",
            "https://accounts.example",
        )
        .unwrap()
        .with_metadata(meta)
    }

    #[tokio::test]
    async fn authorize_url_carries_pkce_state_nonce_and_redirect() {
        let url = provider()
            .authorize_url("st4te", "n0nce", "chAllenge")
            .await
            .unwrap();
        let u = url::Url::parse(&url).unwrap();
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(u.host_str(), Some("accounts.example"));
        assert_eq!(q["state"], "st4te");
        assert_eq!(q["nonce"], "n0nce");
        assert_eq!(q["code_challenge"], "chAllenge");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["client_id"], "cid");
        assert_eq!(q["response_type"], "code");
        assert_eq!(
            q["redirect_uri"],
            "http://localhost:3000/auth/oidc/callback"
        );
        assert!(q["scope"].contains("openid") && q["scope"].contains("email"));
    }

    // Public fixture key, used only for signing tokens from the in-process test issuer.
    const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEICWeYPLxoZKHZlQ6rkBi11E9JwchynXtljATLqym/XS9\n-----END PRIVATE KEY-----";

    fn signing_key() -> openidconnect::core::CoreEdDsaPrivateSigningKey {
        openidconnect::core::CoreEdDsaPrivateSigningKey::from_ed25519_pem(TEST_KEY, None).unwrap()
    }

    fn signed(claims: serde_json::Value) -> String {
        openidconnect::core::CoreIdToken::new(
            serde_json::from_value::<openidconnect::core::CoreIdTokenClaims>(claims).unwrap(),
            &signing_key(),
            CoreJwsSigningAlgorithm::EdDsa,
            None,
            None,
        )
        .unwrap()
        .to_string()
    }

    struct MockIssuer {
        provider: OidcProvider,
        claims: serde_json::Value,
        response: std::sync::Arc<std::sync::Mutex<serde_json::Value>>,
        userinfo: std::sync::Arc<std::sync::Mutex<serde_json::Value>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for MockIssuer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl MockIssuer {
        async fn new(secret: Option<&str>) -> Self {
            use axum::{
                Form, Json, Router,
                http::HeaderMap,
                routing::{get, post},
            };
            use openidconnect::PrivateSigningKey;
            use serde_json::json;
            use std::sync::{Arc, Mutex};
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let issuer = format!("http://{}", listener.local_addr().unwrap());
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let claims = json!({"iss":issuer, "sub":"subject-1", "aud":"cid", "exp":now+300, "iat":now, "nonce":"nonce", "email":"a@example.com", "email_verified":true});
            let response = Arc::new(Mutex::new(
                json!({"access_token":"access", "token_type":"Bearer", "id_token":signed(claims.clone())}),
            ));
            let userinfo = Arc::new(Mutex::new(
                json!({"sub":"subject-1", "email":"a@example.com", "email_verified":true, "name":"Alice"}),
            ));
            let token_response = response.clone();
            let userinfo_response = userinfo.clone();
            let confidential = secret.is_some();
            let discovery = json!({"issuer":issuer, "authorization_endpoint":format!("{issuer}/authorize"), "token_endpoint":format!("{issuer}/token"), "userinfo_endpoint":format!("{issuer}/userinfo"), "jwks_uri":format!("{issuer}/jwks"), "response_types_supported":["code"], "subject_types_supported":["public"], "id_token_signing_alg_values_supported":["EdDSA"], "token_endpoint_auth_methods_supported":["none","client_secret_basic"], "code_challenge_methods_supported":["S256"]});
            let jwks = json!({"keys":[signing_key().as_verification_key()]});
            let app = Router::new()
                .route("/.well-known/openid-configuration", get(move || async move { Json(discovery) }))
                .route("/jwks", get(move || async move { Json(jwks) }))
                .route("/token", post(move |headers: HeaderMap, Form(form): Form<std::collections::HashMap<String, String>>| async move {
                    assert_eq!(form["code_verifier"], "verifier");
                    assert_eq!(form["code"], "code");
                    assert_eq!(form["grant_type"], "authorization_code");
                    assert_eq!(form["redirect_uri"], "https://bento.example/auth/oidc/callback");
                    if confidential {
                        assert_eq!(headers["authorization"], "Basic Y2lkOnNlY3JldA==");
                    } else {
                        assert!(headers.get("authorization").is_none());
                        assert!(!form.contains_key("client_secret"));
                        assert_eq!(form["client_id"], "cid");
                    }
                    Json(token_response.lock().unwrap().clone())
                }))
                .route("/userinfo", get(move |headers: HeaderMap| async move {
                    assert_eq!(headers["authorization"], "Bearer access");
                    Json(userinfo_response.lock().unwrap().clone())
                }));
            let task = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let provider = OidcProvider::new(
                "cid",
                secret,
                "https://bento.example/auth/oidc/callback",
                &issuer,
            )
            .unwrap();
            Self {
                provider,
                claims,
                response,
                userinfo,
                task,
            }
        }

        fn set_claims(&self, claims: serde_json::Value) {
            self.response.lock().unwrap()["id_token"] = signed(claims).into();
        }
    }

    #[tokio::test]
    async fn public_pkce_client_discovers_and_verifies_tokens_with_userinfo_fallback() {
        let m = MockIssuer::new(None).await;
        let identity = m
            .provider
            .exchange("code", "verifier", "nonce")
            .await
            .unwrap();
        assert_eq!(identity.sub, "subject-1");
        assert!(identity.email_verified);
        let mut claims = m.claims.clone();
        claims.as_object_mut().unwrap().remove("email");
        claims.as_object_mut().unwrap().remove("email_verified");
        m.set_claims(claims);
        let identity = m
            .provider
            .exchange("code", "verifier", "nonce")
            .await
            .unwrap();
        assert_eq!(identity.name.as_deref(), Some("Alice"));
        m.userinfo.lock().unwrap()["sub"] = "different-subject".into();
        assert!(
            m.provider
                .exchange("code", "verifier", "nonce")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn additional_audiences_are_explicit_and_never_replace_client_id_or_azp() {
        use serde_json::json;
        let mut m = MockIssuer::new(None).await;
        let mut claims = m.claims.clone();
        claims["aud"] = json!(["cid", "project-id"]);
        claims["azp"] = json!("cid");
        m.set_claims(claims.clone());
        let error = m
            .provider
            .exchange("code", "verifier", "nonce")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not a trusted audience"));

        m.provider.additional_audiences = vec!["project-id".into()];
        m.provider
            .exchange("code", "verifier", "nonce")
            .await
            .unwrap();

        for aud in [
            json!(["project-id"]),
            json!(["other-client", "project-id"]),
            json!(["cid", "project-id", "untrusted-project"]),
        ] {
            let mut invalid = claims.clone();
            invalid["aud"] = aud;
            m.set_claims(invalid);
            assert!(
                m.provider
                    .exchange("code", "verifier", "nonce")
                    .await
                    .is_err()
            );
        }
        for aud in [json!(["cid", "project-id"]), json!("cid")] {
            let mut invalid = claims.clone();
            invalid["aud"] = aud;
            invalid["azp"] = json!("other-client");
            m.set_claims(invalid);
            assert!(
                m.provider
                    .exchange("code", "verifier", "nonce")
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("authorized party")
            );
        }
        claims.as_object_mut().unwrap().remove("azp");
        m.set_claims(claims);
        assert!(
            m.provider
                .exchange("code", "verifier", "nonce")
                .await
                .unwrap_err()
                .to_string()
                .contains("azp")
        );

        // A '*' is a literal audience, never a bypass for arbitrary audiences.
        m.provider.additional_audiences = vec!["*".into()];
        let mut claims = m.claims.clone();
        claims["aud"] = json!(["cid", "project-id"]);
        claims["azp"] = json!("cid");
        m.set_claims(claims);
        assert!(
            m.provider
                .exchange("code", "verifier", "nonce")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn confidential_client_and_id_token_validation() {
        use serde_json::json;
        let mut m = MockIssuer::new(Some("secret")).await;
        m.provider.additional_audiences = vec!["project-id".into()];
        m.claims["aud"] = json!(["cid", "project-id"]);
        m.claims["azp"] = json!("cid");
        m.set_claims(m.claims.clone());
        m.provider
            .exchange("code", "verifier", "nonce")
            .await
            .unwrap();
        for (field, value) in [
            ("iss", json!("https://wrong.example")),
            ("aud", json!("wrong-client")),
            ("exp", json!(1)),
            ("nonce", json!("wrong-nonce")),
            ("at_hash", json!("wrong-hash")),
        ] {
            let mut claims = m.claims.clone();
            claims[field] = value;
            m.set_claims(claims);
            assert!(
                m.provider
                    .exchange("code", "verifier", "nonce")
                    .await
                    .is_err(),
                "{field}"
            );
        }
        // Validly shaped but tampered signature must also be rejected.
        let jwt = signed(m.claims.clone());
        let (payload, _) = jwt.rsplit_once('.').unwrap();
        use base64::Engine;
        let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 64]);
        m.response.lock().unwrap()["id_token"] = format!("{payload}.{signature}").into();
        assert!(
            m.provider
                .exchange("code", "verifier", "nonce")
                .await
                .is_err()
        );
    }
}
