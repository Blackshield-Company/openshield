//! Agent plan → execute → verify loop, driven by a mocked provider.

mod common;

use std::collections::HashMap;

use common::{MockProvider, isolate_home, scratch_dir};
use openshield::agent::{Agent, AgentConfig, PlanStep};
use openshield::config::{Config, ModelConfig, ProviderConfig, ProviderKind};
use openshield::memory::MemoryStore;

const MODEL: &str = "mock-model";

fn config_for(mock: &MockProvider, db: std::path::PathBuf) -> Config {
    let mut providers = HashMap::new();
    providers.insert(
        "mock".to_string(),
        ProviderConfig {
            base_url: mock.base_url.clone(),
            api_key: "test-key".to_string(),
            models: vec![ModelConfig {
                name: MODEL.to_string(),
                context_length: 8192,
                cost_per_1k_input: 0.0,
                cost_per_1k_output: 0.0,
                capabilities: vec!["agentic".to_string()],
            }],
            kind: ProviderKind::OpenAiCompatible,
            headers: HashMap::new(),
            env_file: None,
        },
    );
    Config {
        providers,
        memory_db_path: db,
        default_model: MODEL.to_string(),
        ..Config::default()
    }
}

fn agent_config() -> AgentConfig {
    AgentConfig {
        max_iterations: 10,
        require_approval: false,
        default_model: MODEL.to_string(),
    }
}

fn plan_json(steps: &[(&str, &str, &str)]) -> String {
    let steps: Vec<_> = steps
        .iter()
        .map(|(tool, args, expected)| {
            serde_json::json!({
                "tool_name": tool,
                "args": args,
                "expected_result": expected,
                "verification_criteria": "output matches"
            })
        })
        .collect();
    // Wrap in a markdown fence like real models do, to exercise extraction.
    format!(
        "Here is the plan:\n```json\n{}\n```",
        serde_json::json!({"description": "mock plan", "steps": steps})
    )
}

#[tokio::test]
async fn generate_plan_parses_fenced_json_from_provider() {
    isolate_home();
    let dir = scratch_dir("agent-plan");
    let mock = MockProvider::start(vec![plan_json(&[("fs", "list .", "entries")])]).await;
    let agent = Agent::new(agent_config(), &config_for(&mock, dir.join("mem.db"))).unwrap();

    let plan = agent.generate_plan("list files").await.unwrap();
    assert_eq!(plan.description, "mock plan");
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].tool_name, "fs");
    assert_eq!(mock.request_count(), 1);
    let sent = &mock.requests.lock().unwrap()[0];
    assert!(sent.contains("Create a plan for this task: list files"));
}

#[tokio::test]
async fn run_task_executes_tool_verifies_and_records_memory() {
    isolate_home();
    let dir = scratch_dir("agent-run");
    let file = dir.join("hello.txt");
    std::fs::write(&file, "shield wall holds").unwrap();
    let args = format!("read {}", file.display());

    let mock = MockProvider::start(vec![plan_json(&[("fs", &args, "shield")])]).await;
    let db = dir.join("mem.db");
    let agent = Agent::new(agent_config(), &config_for(&mock, db.clone())).unwrap();

    let result = agent.run_task("read the hello file").await.unwrap();
    assert!(result.success, "{}", result.message);
    assert_eq!(result.total_iterations, 1);
    assert_eq!(result.step_results.len(), 1);
    assert!(result.step_results[0].verified);
    assert!(result.step_results[0].output.contains("shield wall holds"));
    // Verified on the first try: only the planning call hits the provider.
    assert_eq!(mock.request_count(), 1);

    // The tool call and agent messages are persisted to SQLite memory.
    let store = MemoryStore::new(&db).unwrap();
    let hits = store.search_messages("Starting agentic task", 10).unwrap();
    assert_eq!(hits.len(), 1);
    let calls = store
        .search_tool_calls_by_session(&hits[0].session_id)
        .unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool_name, "fs");
    assert!(calls[0].success);
}

#[tokio::test]
async fn failed_verification_retries_then_escalates() {
    isolate_home();
    let dir = scratch_dir("agent-escalate");
    let file = dir.join("log.txt");
    // Output containing a negation word fails verification.
    std::fs::write(&file, "build failed with error").unwrap();
    let args = format!("read {}", file.display());

    let mock = MockProvider::start(vec![
        plan_json(&[("fs", &args, "success")]),
        // Recovery plan: nothing more to do.
        r#"{"description": "give up", "steps": []}"#.to_string(),
    ])
    .await;
    let agent = Agent::new(agent_config(), &config_for(&mock, dir.join("mem.db"))).unwrap();

    let result = agent.run_task("check the build").await.unwrap();
    assert!(!result.success);
    assert_eq!(result.step_results.len(), 1);
    assert!(!result.step_results[0].verified);
    assert_eq!(
        result.step_results[0].iterations, 3,
        "retries up to 3 times"
    );
    assert_eq!(result.total_iterations, 3);
    // Plan request + escalation request.
    assert_eq!(mock.request_count(), 2);
    assert!(mock.requests.lock().unwrap()[1].contains("A tool step failed"));
}

#[tokio::test]
async fn max_iterations_stops_the_loop() {
    isolate_home();
    let dir = scratch_dir("agent-maxiter");
    let file = dir.join("ok.txt");
    std::fs::write(&file, "ok").unwrap();
    let args = format!("read {}", file.display());
    let mock = MockProvider::start(vec![plan_json(&[
        ("fs", &args, "ok"),
        ("fs", &args, "ok"),
        ("fs", &args, "ok"),
    ])])
    .await;
    let cfg = AgentConfig {
        max_iterations: 2,
        ..agent_config()
    };
    let agent = Agent::new(cfg, &config_for(&mock, dir.join("mem.db"))).unwrap();
    let result = agent.run_task("read thrice").await.unwrap();
    assert!(!result.success);
    assert_eq!(result.step_results.len(), 2);
    assert!(result.message.contains("maximum iterations"));
}

#[tokio::test]
async fn unknown_tool_in_plan_is_an_error() {
    isolate_home();
    let dir = scratch_dir("agent-unknown");
    let mock = MockProvider::start(vec![plan_json(&[("definitely_not_a_tool", "x", "y")])]).await;
    let agent = Agent::new(agent_config(), &config_for(&mock, dir.join("mem.db"))).unwrap();
    let err = agent.run_task("do something").await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("definitely_not_a_tool"),
        "unexpected error: {msg}"
    );
}

#[tokio::test]
async fn malformed_plan_is_reported() {
    isolate_home();
    let dir = scratch_dir("agent-badplan");
    let mock = MockProvider::start(vec!["I cannot help with that.".to_string()]).await;
    let agent = Agent::new(agent_config(), &config_for(&mock, dir.join("mem.db"))).unwrap();
    let err = agent.generate_plan("anything").await.unwrap_err();
    assert!(format!("{err:#}").contains("Failed to parse plan JSON"));
}

#[tokio::test]
async fn verify_step_rejects_negative_output() {
    isolate_home();
    let dir = scratch_dir("agent-verify");
    let mock = MockProvider::start(vec![]).await;
    let agent = Agent::new(agent_config(), &config_for(&mock, dir.join("mem.db"))).unwrap();
    let step = PlanStep {
        tool_name: "fs".into(),
        args: "read x".into(),
        expected_result: "file contents".into(),
        verification_criteria: String::new(),
    };
    assert!(
        agent
            .verify_step(&step, "file contents here")
            .await
            .unwrap()
    );
    assert!(!agent.verify_step(&step, "Permission denied").await.unwrap());
    assert!(!agent.verify_step(&step, "Error: not found").await.unwrap());
    let empty = PlanStep {
        expected_result: String::new(),
        ..step
    };
    assert!(agent.verify_step(&empty, "anything").await.unwrap());
}
