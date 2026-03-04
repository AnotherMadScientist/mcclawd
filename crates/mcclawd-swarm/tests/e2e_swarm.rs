//! Task 27: End-to-end swarm test — planner tools + coordinator.
//!
//! Uses CreateSubtaskTool, AddDependencyTool, FinalizePlanTool to build a DAG
//! programmatically, then passes it to SwarmCoordinator for execution.

use std::sync::Arc;
use tokio::sync::Mutex;

use rig::tool::Tool;

use mcclawd_swarm::{
    AddDependencyTool, CreateSubtaskTool, FinalizePlanTool, MergeStrategy, SubtaskStatus,
    SwarmConfig, SwarmCoordinator, TaskDag,
};
use mcclawd_swarm::planner::{AddDependencyArgs, CreateSubtaskArgs, FinalizePlanArgs};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_planner_tools_build_dag_then_execute() {
    // Shared planner state (the DAG being built)
    let state = Arc::new(Mutex::new(TaskDag::new()));

    let create_tool = CreateSubtaskTool::new(state.clone());
    let dep_tool = AddDependencyTool::new(state.clone());
    let finalize_tool = FinalizePlanTool::new(state.clone());

    // Step 1: Create subtasks via the tool
    let research_id = create_tool
        .call(CreateSubtaskArgs {
            agent_role: "researcher".into(),
            prompt: "Research Rust async patterns".into(),
            input_keys: vec![],
            output_key: "research_out".into(),
        })
        .await
        .expect("create research subtask");

    let code_id = create_tool
        .call(CreateSubtaskArgs {
            agent_role: "coder".into(),
            prompt: "Implement async pipeline".into(),
            input_keys: vec!["research_out".into()],
            output_key: "code_out".into(),
        })
        .await
        .expect("create code subtask");

    let review_id = create_tool
        .call(CreateSubtaskArgs {
            agent_role: "reviewer".into(),
            prompt: "Review the implementation".into(),
            input_keys: vec!["code_out".into()],
            output_key: "review_out".into(),
        })
        .await
        .expect("create review subtask");

    // Step 2: Add dependencies via the tool
    dep_tool
        .call(AddDependencyArgs {
            from_subtask_id: research_id.clone(),
            to_subtask_id: code_id.clone(),
        })
        .await
        .expect("add research -> code dependency");

    dep_tool
        .call(AddDependencyArgs {
            from_subtask_id: code_id.clone(),
            to_subtask_id: review_id.clone(),
        })
        .await
        .expect("add code -> review dependency");

    // Step 3: Finalize the plan (validates DAG)
    let plan_json = finalize_tool
        .call(FinalizePlanArgs {})
        .await
        .expect("finalize plan");

    // The finalize tool returns a JSON string with wave info
    assert!(
        !plan_json.is_empty(),
        "finalize should return non-empty plan"
    );

    // Step 4: Drop tools so Arc refcount reaches 1, then extract the DAG
    drop(create_tool);
    drop(dep_tool);
    drop(finalize_tool);

    let dag = match Arc::try_unwrap(state) {
        Ok(mutex) => mutex.into_inner(),
        Err(_) => panic!("should be sole owner of state"),
    };

    let config = SwarmConfig {
        merge_strategy: MergeStrategy::Concatenate,
        ..Default::default()
    };
    let coordinator = SwarmCoordinator::new(config);

    let result = coordinator
        .execute("Build async pipeline", &dag)
        .await
        .expect("coordinator execution should succeed");

    // Verify all subtasks completed
    assert_eq!(result.subtask_results.len(), 3);
    for (id, sr) in &result.subtask_results {
        assert!(
            matches!(sr.status, SubtaskStatus::Completed),
            "subtask {id} should be Completed, got {:?}",
            sr.status
        );
    }

    // Verify shared memory populated
    let mem = coordinator.shared_memory();
    assert!(mem.contains("research_out"));
    assert!(mem.contains("code_out"));
    assert!(mem.contains("review_out"));

    // Verify final output is non-empty
    assert!(
        !result.final_output.is_empty(),
        "final_output should not be empty"
    );
    // Duration may be 0 on fast machines; just verify it is present
    let _ = result.total_duration_ms;
}

#[tokio::test]
async fn e2e_planner_tools_fan_in_dag() {
    let state = Arc::new(Mutex::new(TaskDag::new()));

    let create_tool = CreateSubtaskTool::new(state.clone());
    let dep_tool = AddDependencyTool::new(state.clone());
    let finalize_tool = FinalizePlanTool::new(state.clone());

    // Create 3 independent analysis tasks
    let id_a = create_tool
        .call(CreateSubtaskArgs {
            agent_role: "analyst_a".into(),
            prompt: "Analyze performance".into(),
            input_keys: vec![],
            output_key: "perf_out".into(),
        })
        .await
        .unwrap();

    let id_b = create_tool
        .call(CreateSubtaskArgs {
            agent_role: "analyst_b".into(),
            prompt: "Analyze security".into(),
            input_keys: vec![],
            output_key: "sec_out".into(),
        })
        .await
        .unwrap();

    let id_c = create_tool
        .call(CreateSubtaskArgs {
            agent_role: "analyst_c".into(),
            prompt: "Analyze usability".into(),
            input_keys: vec![],
            output_key: "ux_out".into(),
        })
        .await
        .unwrap();

    // Create merge node
    let id_merge = create_tool
        .call(CreateSubtaskArgs {
            agent_role: "synthesizer".into(),
            prompt: "Synthesize all analyses".into(),
            input_keys: vec!["perf_out".into(), "sec_out".into(), "ux_out".into()],
            output_key: "synthesis_out".into(),
        })
        .await
        .unwrap();

    // Fan-in dependencies
    for source_id in [&id_a, &id_b, &id_c] {
        dep_tool
            .call(AddDependencyArgs {
                from_subtask_id: source_id.clone(),
                to_subtask_id: id_merge.clone(),
            })
            .await
            .unwrap();
    }

    // Finalize
    finalize_tool.call(FinalizePlanArgs {}).await.unwrap();

    // Drop tools so Arc refcount reaches 1
    drop(create_tool);
    drop(dep_tool);
    drop(finalize_tool);

    // Execute
    let dag = match Arc::try_unwrap(state) {
        Ok(mutex) => mutex.into_inner(),
        Err(_) => panic!("should be sole owner of state"),
    };
    let coordinator = SwarmCoordinator::new(SwarmConfig::default());

    let result = coordinator
        .execute("Full analysis", &dag)
        .await
        .expect("fan-in e2e should succeed");

    assert_eq!(result.subtask_results.len(), 4);

    // The merge node should have access to all 3 upstream outputs
    let mem = coordinator.shared_memory();
    assert!(mem.contains("perf_out"));
    assert!(mem.contains("sec_out"));
    assert!(mem.contains("ux_out"));
    assert!(mem.contains("synthesis_out"));
}

#[tokio::test]
async fn e2e_finalize_rejects_cycle() {
    let state = Arc::new(Mutex::new(TaskDag::new()));

    let create_tool = CreateSubtaskTool::new(state.clone());
    let dep_tool = AddDependencyTool::new(state.clone());
    let finalize_tool = FinalizePlanTool::new(state.clone());

    let id_a = create_tool
        .call(CreateSubtaskArgs {
            agent_role: "a".into(),
            prompt: "Task A".into(),
            input_keys: vec![],
            output_key: "a_out".into(),
        })
        .await
        .unwrap();

    let id_b = create_tool
        .call(CreateSubtaskArgs {
            agent_role: "b".into(),
            prompt: "Task B".into(),
            input_keys: vec![],
            output_key: "b_out".into(),
        })
        .await
        .unwrap();

    // Create a cycle: A -> B -> A
    dep_tool
        .call(AddDependencyArgs {
            from_subtask_id: id_a.clone(),
            to_subtask_id: id_b.clone(),
        })
        .await
        .unwrap();

    dep_tool
        .call(AddDependencyArgs {
            from_subtask_id: id_b.clone(),
            to_subtask_id: id_a.clone(),
        })
        .await
        .unwrap();

    // Finalize should detect the cycle and return an error
    let result = finalize_tool.call(FinalizePlanArgs {}).await;
    assert!(
        result.is_err(),
        "finalize should reject a DAG with cycles"
    );
}
