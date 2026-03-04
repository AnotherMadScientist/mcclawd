use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::types::AgentId;

pub mod jwt;
pub use jwt::JwtIdentityProvider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentClaims {
    pub agent_id: String,
    pub iat: i64,
    pub exp: i64,
}

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    async fn issue(&self, agent: &AgentId) -> crate::Result<String>;
    async fn verify(&self, token: &str) -> crate::Result<AgentClaims>;
}
