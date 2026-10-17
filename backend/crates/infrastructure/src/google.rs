//! Google OpenID Connect adapter (Authorization Code + PKCE) built on `openidconnect`.
//!
//! Provider discovery is lazy: the first login attempt fetches Google's metadata
//! and the result is cached, so the server starts (and tests run) offline.

use application::ports::{IdentityError, IdentityProvider, VerifiedIdentity};
use async_trait::async_trait;
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, reqwest,
};
use tokio::sync::OnceCell;

pub const GOOGLE_ISSUER: &str = "https://accounts.google.com";

pub struct GoogleOidcProvider {
    client_id: ClientId,
    client_secret: ClientSecret,
    redirect_url: RedirectUrl,
    issuer: IssuerUrl,
    http: reqwest::Client,
    metadata: OnceCell<CoreProviderMetadata>,
}

fn perr(e: impl std::fmt::Display) -> IdentityError {
    IdentityError::Provider(e.to_string())
}

impl GoogleOidcProvider {
    /// `redirect_url` must equal `APP_BASE_URL + /auth/google/callback`.
    pub fn new(
        client_id: &str,
        client_secret: &str,
        redirect_url: &str,
    ) -> Result<Self, IdentityError> {
        Self::with_issuer(client_id, client_secret, redirect_url, GOOGLE_ISSUER)
    }

    pub fn with_issuer(
        client_id: &str,
        client_secret: &str,
        redirect_url: &str,
        issuer: &str,
    ) -> Result<Self, IdentityError> {
        let http = reqwest::ClientBuilder::new()
            // Following redirects would open the client up to SSRF.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(perr)?;
        Ok(Self {
            client_id: ClientId::new(client_id.to_owned()),
            client_secret: ClientSecret::new(client_secret.to_owned()),
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
            Some(self.client_secret.clone()),
        )
        .set_redirect_uri(self.redirect_url.clone()))
    }
}

#[async_trait]
impl IdentityProvider for GoogleOidcProvider {
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
        let verifier = client.id_token_verifier();
        // Verifies signature, iss, aud, exp and nonce.
        let claims = id_token
            .claims(&verifier, &Nonce::new(nonce.to_owned()))
            .map_err(perr)?;
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
        let email = claims
            .email()
            .map(|e| e.as_str().to_owned())
            .ok_or_else(|| perr("id_token has no email claim"))?;
        Ok(VerifiedIdentity {
            sub: claims.subject().as_str().to_owned(),
            email,
            email_verified: claims.email_verified().unwrap_or(false),
            name: claims
                .name()
                .and_then(|n| n.get(None))
                .map(|n| n.as_str().to_owned()),
            picture: claims
                .picture()
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

    fn provider() -> GoogleOidcProvider {
        let meta = CoreProviderMetadata::new(
            IssuerUrl::new(GOOGLE_ISSUER.into()).unwrap(),
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
        GoogleOidcProvider::new(
            "cid",
            "secret",
            "http://localhost:3000/auth/google/callback",
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
            "http://localhost:3000/auth/google/callback"
        );
        assert!(q["scope"].contains("openid") && q["scope"].contains("email"));
    }
}
