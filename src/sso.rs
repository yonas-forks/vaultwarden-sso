use std::time::Duration;
use url::Url;

use jsonwebtoken::DecodingKey;
use mini_moka::sync::Cache;
use once_cell::sync::Lazy;
use openidconnect::core::{CoreClient, CoreProviderMetadata, CoreResponseType, CoreUserInfoClaims};
use openidconnect::reqwest::async_http_client;
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, Nonce, OAuth2TokenResponse, Scope,
};

use crate::{
    api::ApiResult,
    db::{models::SsoNonce, DbConn},
    CONFIG,
};

static AC_CACHE: Lazy<Cache<String, AuthenticatedUser>> =
    Lazy::new(|| Cache::builder().max_capacity(1000).time_to_live(Duration::from_secs(10 * 60)).build());

async fn get_client() -> ApiResult<CoreClient> {
    let client_id = ClientId::new(CONFIG.sso_client_id());
    let client_secret = ClientSecret::new(CONFIG.sso_client_secret());

    let issuer_url = CONFIG.sso_issuer_url()?;

    let provider_metadata = match CoreProviderMetadata::discover_async(issuer_url, async_http_client).await {
        Err(err) => err!(format!("Failed to discover OpenID provider: {err}")),
        Ok(metadata) => metadata,
    };

    Ok(CoreClient::from_provider_metadata(provider_metadata, client_id, Some(client_secret))
        .set_redirect_uri(CONFIG.sso_redirect_url()?))
}

// The `nonce` allow to protect against replay attacks
pub async fn authorize_url(mut conn: DbConn) -> ApiResult<Url> {
    let client = get_client().await?;

    let (auth_url, _csrf_state, nonce) = client
        .authorize_url(
            AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .url();

    let sso_nonce = SsoNonce::new(nonce.secret().to_string());
    sso_nonce.save(&mut conn).await?;

    Ok(auth_url)
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenPayload {
    exp: i64,
    email: Option<String>,
    nonce: String,
}

#[derive(Clone, Debug)]
struct AuthenticatedUser {
    pub nonce: String,
    pub refresh_token: String,
    pub email: String,
}

// During the 2FA flow we will
//  - retrieve the user information and then only discover he needs 2FA.
//  - second time we will rely on the `AC_CACHE` since the `code` has already been exchanged.
// The `nonce` will ensure that the user is authorized only once.
// We return only the `email` to force calling `redeem` to obtain the `refresh_token`.
pub async fn exchange_code(code: &String) -> ApiResult<String> {
    if let Some(authenticated_user) = AC_CACHE.get(code) {
        return Ok(authenticated_user.email);
    }

    let oidc_code = AuthorizationCode::new(code.clone());
    let client = get_client().await?;

    match client.exchange_code(oidc_code).request_async(async_http_client).await {
        Ok(token_response) => {
            let refresh_token =
                token_response.refresh_token().map_or(String::new(), |token| token.secret().to_string());

            let id_token = match token_response.extra_fields().id_token() {
                None => err!("Token response did not contain an id_token"),
                Some(token) => token.to_string(),
            };

            let endpoint = match client.user_info(token_response.access_token().to_owned(), None) {
                Err(err) => err!(format!("No user_info endpoint: {err}")),
                Ok(endpoint) => endpoint,
            };

            let user_info: CoreUserInfoClaims = match endpoint.request_async(async_http_client).await {
                Err(err) => err!(format!("Request to user_info endpoint failed: {err}")),
                Ok(user_info) => user_info,
            };

            let mut validation = jsonwebtoken::Validation::default();
            validation.insecure_disable_signature_validation();
            let token = match jsonwebtoken::decode::<TokenPayload>(
                id_token.as_str(),
                &DecodingKey::from_secret(&[]),
                &validation,
            ) {
                Err(_err) => err!("Could not decode id token"),
                Ok(payload) => payload.claims,
            };

            let email = match token.email {
                Some(email) => email,
                None => match user_info.email() {
                    None => err!("Neither id token nor userinfo contained an email"),
                    Some(email) => email.to_owned().to_string(),
                },
            };

            let authenticated_user = AuthenticatedUser {
                nonce: token.nonce,
                refresh_token: refresh_token,
                email: email.clone(),
            };

            AC_CACHE.insert(code.clone(), authenticated_user.clone());

            Ok(email)
        }
        Err(err) => err!(format!("Failed to contact token endpoint: {err}")),
    }
}

// User has passed 2FA flow we can delete `nonce` and clear the cache.
pub async fn redeem(code: &String, conn: &mut DbConn) -> ApiResult<String> {
    if let Some(au) = AC_CACHE.get(code) {
        AC_CACHE.invalidate(code);

        if let Some(sso_nonce) = SsoNonce::find(&au.nonce, conn).await {
            match sso_nonce.delete(conn).await {
                Err(msg) => err!(format!("Failed to delete nonce: {msg}")),
                Ok(_) => Ok(au.refresh_token),
            }
        } else {
            err!("Failed to retrive nonce from db")
        }
    } else {
        err!("Failed to retrieve user info from sso cache")
    }
}