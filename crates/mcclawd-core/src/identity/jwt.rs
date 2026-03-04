use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};

use crate::identity::{AgentClaims, IdentityProvider};
use crate::types::AgentId;
use crate::{McclawdError, Result};

pub struct JwtIdentityProvider {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtIdentityProvider {
    pub fn new(secret: &str) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
        }
    }
}

#[async_trait::async_trait]
impl IdentityProvider for JwtIdentityProvider {
    async fn issue(&self, agent: &AgentId) -> Result<String> {
        let now = chrono::Utc::now().timestamp();
        let claims = AgentClaims {
            agent_id: agent.0.clone(),
            iat: now,
            exp: now + 3600, // 1 hour
        };
        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| McclawdError::Identity(format!("JWT encode failed: {e}")))
    }

    async fn verify(&self, token: &str) -> Result<AgentClaims> {
        let data = decode::<AgentClaims>(token, &self.decoding_key, &Validation::default())
            .map_err(|e| McclawdError::Identity(format!("JWT verify failed: {e}")))?;
        Ok(data.claims)
    }
}
