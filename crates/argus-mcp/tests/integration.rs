use std::path::PathBuf;

use argus_mcp::tools::{
    AnalyzeDiffParams, ArgusServer, GetHistoryParams, GetHotspotsParams, GetRepoMapParams,
};
use rmcp::{handler::server::wrapper::Parameters, model::*, ServerHandler};

fn test_server() -> ArgusServer {
    // Tests run from the workspace root; use CARGO_MANIFEST_DIR to get a reliable path
    // that's inside the git repo.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    ArgusServer::new(PathBuf::from(manifest_dir).join("../.."))
}

fn extract_text(result: &CallToolResult) -> &str {
    match &result.content[0].raw {
        RawContent::Text(t) => &t.text,
        _ => panic!("expected text content"),
    }
}

#[test]
fn server_info_is_correct() {
    let server = test_server();
    let info = server.get_info();

    assert_eq!(info.server_info.name, "argus");
    assert_eq!(info.server_info.version, "0.2.0");
    assert!(info.instructions.is_some());
    let instructions = info.instructions.unwrap();
    assert!(instructions.contains("analyze_diff"));
    assert!(instructions.contains("search_codebase"));
    assert!(instructions.contains("get_repo_map"));
    assert!(instructions.contains("get_hotspots"));
    assert!(instructions.contains("get_history"));
}

#[test]
fn analyze_diff_empty_input() {
    let server = test_server();
    let params = Parameters(AnalyzeDiffParams {
        diff: String::new(),
        focus: None,
    });
    let result = server.analyze_diff(params).unwrap();
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["riskScore"]["overall"], 0.0);
}

#[test]
fn analyze_diff_valid_diff() {
    let server = test_server();
    let diff = "\
diff --git a/hello.rs b/hello.rs
--- a/hello.rs
+++ b/hello.rs
@@ -1,2 +1,3 @@
 fn main() {
+    println!(\"hello\");
 }
";
    let params = Parameters(AnalyzeDiffParams {
        diff: diff.to_string(),
        focus: None,
    });
    let result = server.analyze_diff(params).unwrap();
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(parsed.get("riskScore").is_some());
    assert!(parsed.get("files").is_some());
    assert!(parsed.get("summary").is_some());
    assert!(parsed["files"].as_array().unwrap().len() == 1);
}

#[test]
fn get_repo_map_current_dir() {
    let server = test_server();
    let params = Parameters(GetRepoMapParams {
        path: None,
        max_tokens: Some(500),
    });
    let result = server.get_repo_map(params).unwrap();
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(parsed.get("map").is_some());
    assert!(parsed.get("stats").is_some());
    let stats = &parsed["stats"];
    assert!(stats["totalFiles"].as_u64().unwrap() > 0);
    assert!(stats["totalSymbols"].as_u64().unwrap() > 0);
}

#[test]
fn get_hotspots_current_repo() {
    let server = test_server();
    let params = Parameters(GetHotspotsParams {
        path: None,
        since_days: Some(30),
        limit: Some(5),
    });
    let result = server.get_hotspots(params).unwrap();
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(parsed.get("hotspots").is_some());
    assert!(parsed.get("summary").is_some());
}

#[test]
fn get_history_all() {
    let server = test_server();
    let params = Parameters(GetHistoryParams {
        path: None,
        analysis: Some("all".to_string()),
        since_days: Some(30),
        min_coupling: None,
    });
    let result = server.get_history(params).unwrap();
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(parsed.get("summary").is_some());
    assert!(parsed.get("coupling").is_some());
    assert!(parsed.get("ownership").is_some());
}

#[test]
fn get_history_coupling_only() {
    let server = test_server();
    let params = Parameters(GetHistoryParams {
        path: None,
        analysis: Some("coupling".to_string()),
        since_days: Some(30),
        min_coupling: Some(0.5),
    });
    let result = server.get_history(params).unwrap();
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(parsed.get("coupling").is_some());
    assert!(parsed.get("ownership").is_none());
}

#[test]
fn get_history_invalid_analysis() {
    let server = test_server();
    let params = Parameters(GetHistoryParams {
        path: None,
        analysis: Some("invalid".to_string()),
        since_days: None,
        min_coupling: None,
    });
    let result = server.get_history(params);
    assert!(result.is_err());
}
