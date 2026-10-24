//! Port of Remix's `provider.test.ts` (`createOAuthProvider()`): a custom provider that keeps
//! opaque state in the transaction and has its own token type.

mod common;

use common::*;
use rsmix_auth::{refresh_external_auth, AuthError, OAuthAccount, OAuthProvider, OAuthResult, OAuthTokens, OAuthTransaction};
use rsmix_fetch::block_on;
use rsmix_url::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CommunityProfile {
    id: String,
}

/// `interface CommunityTokens extends OAuthTokens { proof: string }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CommunityTokens {
    #[serde(flatten)]
    tokens: OAuthTokens,
    proof: String,
}

struct Community;

impl OAuthProvider for Community {
    type Profile = CommunityProfile;
    type Tokens = CommunityTokens;

    fn name(&self) -> &str {
        "community"
    }

    async fn authorization_url(&self, transaction: &mut OAuthTransaction) -> Result<Url, AuthError> {
        transaction.provider_state = Some("encrypted-provider-state".into());
        let mut url = Url::parse("https://community.example.com/authorize").unwrap();
        let mut params = url.search_params();
        params.set("state", &transaction.state);
        url.set_search_params(&params);
        Ok(url)
    }

    async fn handle_callback(&self, callback: &Url, transaction: &OAuthTransaction) -> Result<OAuthResult<CommunityProfile, CommunityTokens>, AuthError> {
        assert_eq!(callback.search_params().get("code"), Some("good-code"));
        assert_eq!(transaction.provider_state.as_deref(), Some("encrypted-provider-state"));
        Ok(OAuthResult {
            provider: "community".into(),
            account: OAuthAccount { provider: "community".into(), provider_account_id: "community-user".into() },
            profile: CommunityProfile { id: "community-user".into() },
            tokens: CommunityTokens { tokens: OAuthTokens { access_token: "community-access-token".into(), ..Default::default() }, proof: "callback-proof".into() },
        })
    }

    async fn refresh_tokens(&self, tokens: &CommunityTokens) -> Result<CommunityTokens, AuthError> {
        Ok(CommunityTokens { tokens: OAuthTokens { access_token: "refreshed-community-access-token".into(), ..tokens.tokens.clone() }, proof: "refreshed-proof".into() })
    }
}

#[test]
fn supports_a_complete_external_auth_flow_from_the_public_package_api() {
    let router = app(Community);

    let login = get(&router, "/login", None);
    let callback = get(&router, &format!("/callback?code=good-code&state={}", state_of(&login)), Some(&login));

    assert_eq!(login.status, 302);
    assert_eq!(
        callback.json(),
        json!({
            "result": {
                "provider": "community",
                "account": { "provider": "community", "providerAccountId": "community-user" },
                "profile": { "id": "community-user" },
                "tokens": { "accessToken": "community-access-token", "proof": "callback-proof" },
            },
        })
    );
}

#[test]
fn preserves_provider_specific_token_types_when_refreshing() {
    let tokens = CommunityTokens {
        tokens: OAuthTokens { access_token: "community-access-token".into(), refresh_token: Some("community-refresh-token".into()), ..Default::default() },
        proof: "callback-proof".into(),
    };

    let refreshed = block_on(refresh_external_auth(&Community, &tokens)).unwrap();

    assert_eq!(refreshed.provider, "community");
    assert_eq!(refreshed.tokens.tokens.access_token, "refreshed-community-access-token");
    assert_eq!(refreshed.tokens.proof, "refreshed-proof");
}
