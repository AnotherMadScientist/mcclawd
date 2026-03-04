// =============================================================================
// Bollard Docker API Reference — bollard 0.20.1
// =============================================================================
// Comprehensive code snippets for all common Docker operations via bollard.
// Cargo.toml: bollard = "0.20"  (add features as needed, see Quick Start below)
//
// Feature flags:
//   Default:  "http", "pipe" (Unix sockets + TCP)
//   Optional: "ssl" (HTTPS), "ssh" (SSH tunnel), "buildkit" + "chrono" (BuildKit)
// =============================================================================

use bollard::container::LogOutput;
use bollard::models::{
    ContainerCreateBody, EndpointSettings, HostConfig, Mount, MountTypeEnum, NetworkingConfig,
};
use bollard::query_parameters::{
    BuildImageOptionsBuilder, CreateContainerOptionsBuilder, InspectContainerOptionsBuilder,
    LogsOptionsBuilder, RemoveContainerOptionsBuilder, StopContainerOptionsBuilder,
    WaitContainerOptionsBuilder,
};
use bollard::Docker;
use futures_util::stream::{StreamExt, TryStreamExt};
use std::collections::HashMap;
use std::default::Default;

// =============================================================================
// 1. CONNECTION
// =============================================================================

async fn connect_examples() -> Result<(), bollard::errors::Error> {
    // Unix socket (default: /var/run/docker.sock)
    let docker = Docker::connect_with_socket_defaults()?;

    // Custom socket path with timeout
    let docker = Docker::connect_with_socket(
        "/var/run/docker.sock",
        120, // timeout in seconds
        bollard::API_DEFAULT_VERSION,
    )?;

    // TCP connection (requires "http" feature — enabled by default)
    let docker = Docker::connect_with_http_defaults()?;

    // Platform-aware default (Unix socket on Linux/macOS, named pipe on Windows)
    let docker = Docker::connect_with_local_defaults()?;

    // Verify connection
    let version = docker.version().await?;
    println!("Docker version: {:?}", version.version);

    let info = docker.info().await?;
    println!("Containers: {:?}", info.containers);

    Ok(())
}

// =============================================================================
// 2. CREATE CONTAINER
// =============================================================================

async fn create_container_example(docker: &Docker) -> Result<String, bollard::errors::Error> {
    // Query parameters (container name, platform)
    let options = CreateContainerOptionsBuilder::default()
        .name("my-sandbox-container")
        .build();

    // Container body — 26 fields, all Option<T>, Default-able
    let config = ContainerCreateBody {
        image: Some("ubuntu:22.04".to_string()),
        cmd: Some(vec![
            "/bin/bash".to_string(),
            "-c".to_string(),
            "echo hello && sleep 30".to_string(),
        ]),
        env: Some(vec![
            "FOO=bar".to_string(),
            "PATH=/usr/local/bin:/usr/bin:/bin".to_string(),
        ]),
        working_dir: Some("/workspace".to_string()),
        user: Some("1000:1000".to_string()),
        tty: Some(false),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        labels: Some(HashMap::from([
            ("app".to_string(), "mcclawd".to_string()),
            ("task.id".to_string(), "abc-123".to_string()),
        ])),
        // HostConfig controls resource limits, mounts, networking
        host_config: Some(HostConfig {
            memory: Some(512 * 1024 * 1024),           // 512 MB
            memory_swap: Some(512 * 1024 * 1024),      // same = no swap
            nano_cpus: Some(1_000_000_000),             // 1.0 CPU (in nanocpus)
            pids_limit: Some(256),
            network_mode: Some("bridge".to_string()),   // or "none", "host", custom
            auto_remove: Some(false),                    // we manage cleanup
            // See section 4 below for mounts
            ..Default::default()
        }),
        // NetworkingConfig for connecting to specific networks at creation time
        networking_config: Some(NetworkingConfig {
            endpoints_config: Some(HashMap::from([(
                "my-network".to_string(),
                EndpointSettings {
                    aliases: Some(vec!["sandbox".to_string()]),
                    ..Default::default()
                },
            )])),
        }),
        ..Default::default()
    };

    let response = docker.create_container(Some(options), config).await?;
    println!("Container ID: {}", response.id);
    // response.warnings: Option<Vec<String>>
    Ok(response.id)
}

// =============================================================================
// 3. START / STOP / REMOVE CONTAINERS
// =============================================================================

async fn lifecycle_examples(docker: &Docker, name: &str) -> Result<(), bollard::errors::Error> {
    // --- Start (no options needed usually) ---
    docker.start_container(name, None::<bollard::query_parameters::StartContainerOptions>).await?;

    // --- Stop (with timeout) ---
    let stop_opts = StopContainerOptionsBuilder::default()
        .t(10) // seconds to wait before killing
        .build();
    docker.stop_container(name, Some(stop_opts)).await?;

    // --- Kill (immediate, specific signal) ---
    docker.kill_container(
        name,
        Some(bollard::query_parameters::KillContainerOptionsBuilder::default()
            .signal("SIGKILL")
            .build()),
    ).await?;

    // --- Remove (with force + volumes cleanup) ---
    let rm_opts = RemoveContainerOptionsBuilder::default()
        .force(true)   // remove even if running
        .v(true)       // remove anonymous volumes
        .build();
    docker.remove_container(name, Some(rm_opts)).await?;

    // --- Rename ---
    docker.rename_container(
        name,
        bollard::query_parameters::RenameContainerOptionsBuilder::default()
            .name("new-name")
            .build(),
    ).await?;

    // --- Restart ---
    docker.restart_container(name, None::<bollard::query_parameters::RestartContainerOptions>).await?;

    // --- Pause / Unpause ---
    docker.pause_container(name).await?;
    docker.unpause_container(name).await?;

    Ok(())
}

// =============================================================================
// 4. BIND MOUNTS AND TMPFS MOUNTS
// =============================================================================

fn mount_examples() -> HostConfig {
    HostConfig {
        // --- Option A: "binds" field (simple string format) ---
        // Format: "host_path:container_path[:options]"
        // Options: ro, rw, z, Z (SELinux labels)
        binds: Some(vec![
            "/host/workspace:/workspace:rw".to_string(),
            "/host/config:/etc/app/config:ro".to_string(),
        ]),

        // --- Option B: "mounts" field (structured, preferred for new code) ---
        mounts: Some(vec![
            // Bind mount
            Mount {
                target: Some("/workspace".to_string()),
                source: Some("/host/workspace".to_string()),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(false),
                ..Default::default()
            },
            // tmpfs mount (in-memory, never written to disk — ideal for secrets)
            Mount {
                target: Some("/run/secrets".to_string()),
                typ: Some(MountTypeEnum::TMPFS),
                read_only: Some(false),
                // tmpfs_options for size limit:
                tmpfs_options: Some(bollard::models::MountTmpfsOptions {
                    size_bytes: Some(64 * 1024 * 1024), // 64 MB
                    mode: Some(0o700),
                }),
                ..Default::default()
            },
            // Named volume mount
            Mount {
                target: Some("/data".to_string()),
                source: Some("my-named-volume".to_string()),
                typ: Some(MountTypeEnum::VOLUME),
                read_only: Some(false),
                ..Default::default()
            },
        ]),

        // --- Legacy tmpfs field (simpler but less control) ---
        // Key = container path, Value = mount options
        tmpfs: Some(HashMap::from([
            ("/tmp".to_string(), "rw,noexec,nosuid,size=100m".to_string()),
        ])),

        ..Default::default()
    }
}

// =============================================================================
// 5. NETWORK OPERATIONS
// =============================================================================

async fn network_examples(docker: &Docker) -> Result<(), bollard::errors::Error> {
    // --- Create a network ---
    let create_opts = bollard::models::CreateNetworkBody {
        name: Some("mcclawd-sandbox".to_string()),
        driver: Some("bridge".to_string()),
        internal: Some(false),
        enable_ipv6: Some(false),
        labels: Some(HashMap::from([
            ("app".to_string(), "mcclawd".to_string()),
        ])),
        ..Default::default()
    };
    let network = docker.create_network(create_opts).await?;
    println!("Network ID: {:?}", network.id);

    // --- Connect a running container to a network ---
    let connect_opts = bollard::models::ConnectNetworkBody {
        container: Some("my-container".to_string()),
        endpoint_config: Some(EndpointSettings {
            aliases: Some(vec!["sandbox-alias".to_string()]),
            ..Default::default()
        }),
    };
    docker.connect_network("mcclawd-sandbox", connect_opts).await?;

    // --- Disconnect container from network ---
    let disconnect_opts = bollard::models::DisconnectNetworkBody {
        container: Some("my-container".to_string()),
        force: Some(true),
    };
    docker.disconnect_network("mcclawd-sandbox", disconnect_opts).await?;

    // --- List networks ---
    let networks = docker.list_networks(None::<bollard::query_parameters::ListNetworksOptions>).await?;
    for net in networks {
        println!("Network: {} ({})", net.name.unwrap_or_default(), net.id.unwrap_or_default());
    }

    // --- Remove network ---
    docker.remove_network("mcclawd-sandbox").await?;

    Ok(())
}

// =============================================================================
// 6. STREAM CONTAINER LOGS
// =============================================================================

async fn logs_example(docker: &Docker, name: &str) -> Result<(), bollard::errors::Error> {
    let options = LogsOptionsBuilder::default()
        .stdout(true)
        .stderr(true)
        .follow(true)       // stream continuously (like `docker logs -f`)
        .tail("100")        // last N lines, or "all"
        .timestamps(true)   // prepend timestamps
        .build();

    // docker.logs() returns impl Stream<Item = Result<LogOutput, Error>>
    let mut log_stream = docker.logs(name, Some(options));

    while let Some(log_result) = log_stream.next().await {
        match log_result? {
            LogOutput::StdOut { message } => {
                print!("[stdout] {}", String::from_utf8_lossy(&message));
            }
            LogOutput::StdErr { message } => {
                eprint!("[stderr] {}", String::from_utf8_lossy(&message));
            }
            LogOutput::StdIn { message } => {
                // Rarely used in log streaming
                print!("[stdin] {}", String::from_utf8_lossy(&message));
            }
            LogOutput::Console { message } => {
                print!("[console] {}", String::from_utf8_lossy(&message));
            }
        }
    }

    Ok(())
}

// Collect all logs at once (non-streaming)
async fn collect_logs(docker: &Docker, name: &str) -> Result<String, bollard::errors::Error> {
    let options = LogsOptionsBuilder::default()
        .stdout(true)
        .stderr(true)
        .follow(false) // don't follow — collect and return
        .build();

    let logs: Vec<LogOutput> = docker
        .logs(name, Some(options))
        .try_collect()
        .await?;

    let output = logs
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("");

    Ok(output)
}

// =============================================================================
// 7. BUILD IMAGES FROM DOCKERFILE
// =============================================================================

async fn build_image_example(docker: &Docker) -> Result<(), bollard::errors::Error> {
    // --- Option A: Build from remote Dockerfile URL ---
    let build_opts = BuildImageOptionsBuilder::default()
        .dockerfile("Dockerfile")
        .t("mcclawd/sandbox:latest")   // tag
        .remote("https://example.com/context.tar.gz")
        .rm(true)                       // remove intermediate containers
        .forcerm(true)                  // remove on failure too
        .nocache(false)
        .pull("true")                   // always pull base image
        .memory(512 * 1024 * 1024)      // build memory limit
        .cpuquota(100000)               // CPU quota
        .networkmode("host")
        .platform("linux/amd64")
        .build();

    let mut stream = docker.build_image(build_opts, None, None);
    while let Some(msg) = stream.next().await {
        let info = msg?;
        // BuildInfo has: stream, error, error_detail, status, progress, id
        if let Some(stream_msg) = info.stream {
            print!("{}", stream_msg);
        }
        if let Some(err) = info.error {
            eprintln!("Build error: {}", err);
        }
    }

    // --- Option B: Build from local tar archive ---
    // The body must be a tar archive containing the Dockerfile and context
    use bytes::Bytes;
    use std::io::Read;

    let mut tar_file = std::fs::File::open("/path/to/context.tar.gz")?;
    let mut contents = Vec::new();
    tar_file.read_to_end(&mut contents)?;

    let build_opts = BuildImageOptionsBuilder::default()
        .dockerfile("Dockerfile")
        .t("mcclawd/sandbox:latest")
        .rm(true)
        .build();

    // Pass body as Option<Body> — use bollard::body_full for Bytes
    let body = bollard::body_full(Bytes::from(contents));
    let mut stream = docker.build_image(build_opts, None, Some(body));
    while let Some(msg) = stream.next().await {
        let info = msg?;
        if let Some(s) = info.stream {
            print!("{}", s);
        }
    }

    // --- Option C: Build tar archive programmatically ---
    let mut header = tar::Header::new_gnu();
    let dockerfile = b"FROM ubuntu:22.04\nRUN apt-get update\n";
    header.set_path("Dockerfile").unwrap();
    header.set_size(dockerfile.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();

    let mut tar_builder = tar::Builder::new(Vec::new());
    tar_builder.append(&header, &dockerfile[..]).unwrap();
    let tar_bytes = tar_builder.into_inner().unwrap();

    let build_opts = BuildImageOptionsBuilder::default()
        .dockerfile("Dockerfile")
        .t("mcclawd/sandbox:built")
        .rm(true)
        .build();

    let body = bollard::body_full(Bytes::from(tar_bytes));
    let mut stream = docker.build_image(build_opts, None, Some(body));
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(info) => {
                if let Some(s) = info.stream { print!("{}", s); }
            }
            Err(e) => eprintln!("Build error: {}", e),
        }
    }

    Ok(())
}

// Build with BuildKit (requires features = ["buildkit", "chrono"])
// See bollard/examples/build_buildkit.rs for full example

// =============================================================================
// 8. CONTAINER STATUS, INSPECT, AND WAIT
// =============================================================================

async fn status_examples(docker: &Docker, name: &str) -> Result<(), bollard::errors::Error> {
    // --- Inspect container (full details) ---
    let opts = InspectContainerOptionsBuilder::default()
        .size(true) // include size info
        .build();
    let info = docker.inspect_container(name, Some(opts)).await?;

    // Key fields from ContainerInspectResponse:
    println!("ID: {:?}", info.id);
    println!("Name: {:?}", info.name);
    if let Some(state) = &info.state {
        println!("Status: {:?}", state.status);     // e.g., "running", "exited"
        println!("Running: {:?}", state.running);
        println!("ExitCode: {:?}", state.exit_code);
        println!("Pid: {:?}", state.pid);
        println!("OOMKilled: {:?}", state.oom_killed);
        println!("StartedAt: {:?}", state.started_at);
        println!("FinishedAt: {:?}", state.finished_at);
    }

    // --- List containers (like `docker ps`) ---
    let containers = docker.list_containers(
        Some(bollard::query_parameters::ListContainersOptionsBuilder::default()
            .all(true) // include stopped
            .build())
    ).await?;
    for c in containers {
        println!("{}: {} ({})",
            c.id.unwrap_or_default(),
            c.image.unwrap_or_default(),
            c.state.unwrap_or_default(),
        );
    }

    // --- Wait for container to finish (non-blocking stream) ---
    let wait_opts = WaitContainerOptionsBuilder::default()
        .condition("not-running")  // wait until not-running
        .build();

    // wait_container returns Stream<Item = Result<ContainerWaitResponse, Error>>
    let mut wait_stream = docker.wait_container(name, Some(wait_opts));

    while let Some(result) = wait_stream.next().await {
        let wait_response = result?;
        // ContainerWaitResponse fields:
        //   status_code: i64  — process exit code
        //   error: Option<ContainerWaitExitError>
        println!("Container exited with code: {}", wait_response.status_code);
        if let Some(err) = wait_response.error {
            eprintln!("Wait error: {:?}", err.message);
        }
    }

    Ok(())
}

// --- Streaming stats (CPU, memory, network I/O) ---
async fn stats_example(docker: &Docker, name: &str) -> Result<(), bollard::errors::Error> {
    use bollard::query_parameters::StatsOptionsBuilder;

    let options = StatsOptionsBuilder::default()
        .stream(true)  // continuous stream
        .one_shot(false)
        .build();

    let mut stats_stream = docker.stats(name, Some(options));

    while let Some(stat) = stats_stream.next().await {
        let stat = stat?;
        if let Some(mem) = &stat.memory_stats {
            println!("Memory usage: {:?} / {:?}",
                mem.usage, mem.limit);
        }
    }

    Ok(())
}

// =============================================================================
// 9. EXEC — RUN COMMANDS IN RUNNING CONTAINERS
// =============================================================================

async fn exec_example(docker: &Docker, name: &str) -> Result<(), bollard::errors::Error> {
    use bollard::exec::StartExecResults;
    use bollard::models::CreateExecBody;

    // Create exec instance
    let exec = docker
        .create_exec(
            name,
            CreateExecBody {
                cmd: Some(vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    "echo hello from exec".to_string(),
                ]),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                working_dir: Some("/workspace".to_string()),
                env: Some(vec!["MY_VAR=value".to_string()]),
                user: Some("1000".to_string()),
                ..Default::default()
            },
        )
        .await?;

    // Start exec and stream output
    let start_result = docker
        .start_exec(&exec.id, None::<bollard::query_parameters::StartExecOptions>)
        .await?;

    match start_result {
        StartExecResults::Attached { mut output, .. } => {
            while let Some(msg) = output.next().await {
                match msg? {
                    LogOutput::StdOut { message } => {
                        print!("{}", String::from_utf8_lossy(&message));
                    }
                    LogOutput::StdErr { message } => {
                        eprint!("{}", String::from_utf8_lossy(&message));
                    }
                    _ => {}
                }
            }
        }
        StartExecResults::Detached => {
            println!("Exec started in detached mode");
        }
    }

    // Check exec exit code
    let exec_info = docker.inspect_exec(&exec.id).await?;
    println!("Exec exit code: {:?}", exec_info.exit_code);

    Ok(())
}

// =============================================================================
// 10. ERROR HANDLING PATTERNS
// =============================================================================

async fn error_handling_example(docker: &Docker) -> Result<(), Box<dyn std::error::Error>> {
    use bollard::errors::Error;

    match docker.inspect_container("nonexistent", None).await {
        Ok(info) => println!("Found: {:?}", info.name),
        Err(Error::DockerResponseServerError {
            status_code,
            message,
        }) => {
            // Docker daemon returned an error
            match status_code {
                404 => println!("Container not found: {}", message),
                409 => println!("Conflict (container already exists): {}", message),
                500 => println!("Docker server error: {}", message),
                _ => println!("Docker error {}: {}", status_code, message),
            }
        }
        Err(Error::RequestTimeoutError) => {
            println!("Request timed out");
        }
        Err(Error::IOError { err }) => {
            println!("I/O error (Docker not running?): {}", err);
        }
        Err(Error::HttpClientError { err }) => {
            println!("HTTP client error: {}", err);
        }
        Err(Error::HyperResponseError { err }) => {
            println!("Hyper response error: {}", err);
        }
        Err(Error::JsonSerdeError { err }) => {
            println!("JSON serialization error: {}", err);
        }
        Err(e) => {
            println!("Other error: {}", e);
        }
    }

    // --- Pattern: ensure cleanup on failure ---
    let container_name = "temp-container";
    let result = async {
        // ... do work ...
        Ok::<(), Error>(())
    }
    .await;

    // Always try to clean up, ignore errors
    let _ = docker
        .remove_container(
            container_name,
            Some(
                RemoveContainerOptionsBuilder::default()
                    .force(true)
                    .v(true)
                    .build(),
            ),
        )
        .await;

    result?;
    Ok(())
}

// =============================================================================
// 11. COMPLETE LIFECYCLE EXAMPLE — McClawd Sandbox Pattern
// =============================================================================

async fn full_sandbox_lifecycle(
    docker: &Docker,
    task_id: &str,
    image: &str,
    cmd: Vec<String>,
    workspace_path: &str,
    secrets: HashMap<String, String>,
) -> Result<i64, bollard::errors::Error> {
    let container_name = format!("mcclawd-sandbox-{}", task_id);

    // 1. Create with mounts + resource limits
    let config = ContainerCreateBody {
        image: Some(image.to_string()),
        cmd: Some(cmd),
        env: Some(vec!["TERM=xterm-256color".to_string()]),
        working_dir: Some("/workspace".to_string()),
        labels: Some(HashMap::from([
            ("mcclawd.task".to_string(), task_id.to_string()),
        ])),
        host_config: Some(HostConfig {
            memory: Some(512 * 1024 * 1024),
            nano_cpus: Some(1_000_000_000),
            pids_limit: Some(256),
            network_mode: Some("none".to_string()), // isolated
            mounts: Some(vec![
                // Workspace bind mount
                Mount {
                    target: Some("/workspace".to_string()),
                    source: Some(workspace_path.to_string()),
                    typ: Some(MountTypeEnum::BIND),
                    read_only: Some(false),
                    ..Default::default()
                },
                // Secrets via tmpfs (never touches disk)
                Mount {
                    target: Some("/run/secrets".to_string()),
                    typ: Some(MountTypeEnum::TMPFS),
                    tmpfs_options: Some(bollard::models::MountTmpfsOptions {
                        size_bytes: Some(1024 * 1024), // 1 MB
                        mode: Some(0o700),
                    }),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };

    let create_opts = CreateContainerOptionsBuilder::default()
        .name(&container_name)
        .build();

    docker.create_container(Some(create_opts), config).await?;

    // 2. Write secrets to tmpfs via exec (after start)
    //    Alternative: use `docker cp` or init script
    docker.start_container(&container_name, None::<bollard::query_parameters::StartContainerOptions>).await?;

    // 3. Stream logs in background (spawn a task)
    let docker_clone = docker.clone();
    let name_clone = container_name.clone();
    let log_handle = tokio::spawn(async move {
        let opts = LogsOptionsBuilder::default()
            .stdout(true)
            .stderr(true)
            .follow(true)
            .build();

        let mut stream = docker_clone.logs(&name_clone, Some(opts));
        while let Some(Ok(log)) = stream.next().await {
            match log {
                LogOutput::StdOut { message } => {
                    print!("[sandbox] {}", String::from_utf8_lossy(&message));
                }
                LogOutput::StdErr { message } => {
                    eprint!("[sandbox] {}", String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }
    });

    // 4. Wait for completion
    let wait_opts = WaitContainerOptionsBuilder::default()
        .condition("not-running")
        .build();

    let mut wait_stream = docker.wait_container(&container_name, Some(wait_opts));
    let exit_code = if let Some(result) = wait_stream.next().await {
        let resp = result?;
        resp.status_code
    } else {
        -1
    };

    // 5. Cancel log streaming
    log_handle.abort();

    // 6. Cleanup
    docker
        .remove_container(
            &container_name,
            Some(
                RemoveContainerOptionsBuilder::default()
                    .force(true)
                    .v(true)
                    .build(),
            ),
        )
        .await?;

    Ok(exit_code)
}

// =============================================================================
// QUICK REFERENCE — Method Signatures
// =============================================================================
//
// Container lifecycle:
//   create_container(Option<CreateContainerOptions>, ContainerCreateBody) -> ContainerCreateResponse
//   start_container(&str, Option<StartContainerOptions>) -> ()
//   stop_container(&str, Option<StopContainerOptions>) -> ()
//   kill_container(&str, Option<KillContainerOptions>) -> ()
//   restart_container(&str, Option<RestartContainerOptions>) -> ()
//   remove_container(&str, Option<RemoveContainerOptions>) -> ()
//   rename_container(&str, RenameContainerOptions) -> ()
//   pause_container(&str) -> ()
//   unpause_container(&str) -> ()
//
// Inspection:
//   inspect_container(&str, Option<InspectContainerOptions>) -> ContainerInspectResponse
//   list_containers(Option<ListContainersOptions>) -> Vec<ContainerSummary>
//   container_changes(&str) -> Option<Vec<FilesystemChange>>
//
// Streaming:
//   logs(&str, Option<LogsOptions>) -> Stream<LogOutput>
//   stats(&str, Option<StatsOptions>) -> Stream<Stats>
//   wait_container(&str, Option<WaitContainerOptions>) -> Stream<ContainerWaitResponse>
//
// Exec:
//   create_exec(&str, CreateExecBody) -> CreateExecResults
//   start_exec(&str, Option<StartExecOptions>) -> StartExecResults
//   inspect_exec(&str) -> ExecInspectResponse
//
// Images:
//   build_image(BuildImageOptions, Option<DockerCredentials>, Option<Body>) -> Stream<BuildInfo>
//   list_images(Option<ListImagesOptions>) -> Vec<ImageSummary>
//   inspect_image(&str) -> ImageInspect
//   pull_image(Option<CreateImageOptions>, Option<Body>, Option<DockerCredentials>) -> Stream<CreateImageInfo>
//   remove_image(&str, Option<RemoveImageOptions>, Option<DockerCredentials>) -> Vec<ImageDeleteResponseItem>
//
// Networks:
//   create_network(CreateNetworkBody) -> NetworkCreateResponse
//   connect_network(&str, ConnectNetworkBody) -> ()
//   disconnect_network(&str, DisconnectNetworkBody) -> ()
//   list_networks(Option<ListNetworksOptions>) -> Vec<Network>
//   inspect_network(&str, Option<InspectNetworkOptions>) -> Network
//   remove_network(&str) -> ()
//
// Error variants (bollard::errors::Error):
//   DockerResponseServerError { status_code: u16, message: String }
//   DockerResponseBadParameterError { message: String }
//   DockerResponseNotFoundError { message: String }
//   DockerResponseNotModifiedError { message: String }
//   DockerResponseConflictError { message: String }
//   RequestTimeoutError
//   IOError { err: io::Error }
//   HttpClientError { err: ... }
//   HyperResponseError { err: hyper::Error }
//   JsonSerdeError { err: serde_json::Error }
//   StrParseError { err: ... }
//   NoHomePathError
//   CertPathError { err: ... }
//   CertMultipleError { err: ... }
