//! Integration tests for sandbox orchestrator.
//!
//! Requires Docker to be running.
//! Run: `cargo test -p mcclawd-api --test sandbox_integration -- --ignored --nocapture`

// Note: These tests use the real Docker daemon.
// They create/remove containers and are #[ignore]d by default.

use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions, WaitContainerOptions};
use bollard::Docker;
use futures::StreamExt;

#[tokio::test]
#[ignore]
async fn sandbox_health_check() {
    // This test just verifies Docker connectivity
    let docker = Docker::connect_with_local_defaults().expect("Docker should be available");
    let ping = docker.ping().await;
    assert!(ping.is_ok(), "Docker daemon should respond to ping");
}

#[tokio::test]
#[ignore]
async fn sandbox_create_and_cleanup_container() {
    let docker = Docker::connect_with_local_defaults().unwrap();
    let container_name = "mcclawd-test-sandbox";

    // Cleanup any leftover test container
    let _ = docker
        .remove_container(
            container_name,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;

    // Create container using alpine (small, fast)
    let config: Config<String> = Config {
        image: Some("alpine:latest".to_string()),
        cmd: Some(vec!["echo".to_string(), "hello mcclawd".to_string()]),
        ..Default::default()
    };

    let opts = CreateContainerOptions {
        name: container_name.to_string(),
        platform: None,
    };

    let response = docker.create_container(Some(opts), config).await;
    assert!(response.is_ok(), "should create container: {:?}", response);
    let container_id = response.unwrap().id;

    // Start
    let start = docker.start_container::<String>(&container_id, None).await;
    assert!(start.is_ok(), "should start container");

    // Wait for completion
    let opts = WaitContainerOptions {
        condition: "not-running".to_string(),
    };
    let mut stream = docker.wait_container(&container_id, Some(opts));
    if let Some(result) = stream.next().await {
        let response = result.unwrap();
        assert_eq!(response.status_code, 0, "container should exit 0");
    }

    // Cleanup
    let remove = docker
        .remove_container(
            &container_id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;
    assert!(remove.is_ok(), "should remove container");
}

#[tokio::test]
#[ignore]
async fn sandbox_orchestrator_lifecycle() {
    // Test the SandboxOrchestrator wrapper itself
    use mcclawd_api::sandbox::SandboxOrchestrator;

    let orch = SandboxOrchestrator::new().expect("should connect to Docker");
    let healthy = orch.health_check().await;
    assert!(healthy, "orchestrator health_check should succeed");
}
