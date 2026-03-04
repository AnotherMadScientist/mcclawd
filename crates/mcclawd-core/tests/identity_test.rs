use mcclawd_core::identity::{IdentityProvider, JwtIdentityProvider};
use mcclawd_core::types::AgentId;

#[tokio::test]
async fn test_issue_and_verify_token() {
    let provider = JwtIdentityProvider::new("test-secret-key");
    let agent = AgentId("coding".to_string());
    let token = provider.issue(&agent).await.unwrap();
    let claims = provider.verify(&token).await.unwrap();
    assert_eq!(claims.agent_id, "coding");
}

#[tokio::test]
async fn test_invalid_token_fails() {
    let provider = JwtIdentityProvider::new("test-secret-key");
    let result = provider.verify("invalid-token").await;
    assert!(result.is_err());
}
