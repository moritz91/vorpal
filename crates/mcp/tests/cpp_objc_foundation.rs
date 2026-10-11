//! Genuine Foundation context in the ordinary environment-aware MCP server.
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use vorpal_ingest::{ExtractionEnv, cpp_macro_compiler::CompilerCommand};
use vorpal_mcp::{Profile, Server};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
  name: String,
  probe_path: PathBuf,
  header_path: PathBuf,
  command: CompilerCommand,
}

fn require_macos() {
  #[cfg(not(target_os = "macos"))]
  panic!("Foundation controls require a real macOS host");
}

fn call(server: &mut Server, id: u64, name: &str, arguments: Value) -> String {
  let request = json!({"jsonrpc":"2.0", "id":id, "method":"tools/call", "params":{"name":name, "arguments":arguments}});
  let response: Value =
    serde_json::from_str(&server.handle_line(&request.to_string()).unwrap()).unwrap();
  assert_eq!(response["result"]["isError"], false, "{response}");
  response["result"]["content"][0]["text"]
    .as_str()
    .unwrap()
    .to_owned()
}

#[test]
fn genuine_foundation_mcp_recaptures_external_headers_without_source_hints() {
  let Some(manifest) = std::env::var_os("VORPAL_TEST_OBJC_FOUNDATION_CASES") else {
    eprintln!("not exercised: genuine macOS Foundation manifest is absent");
    return;
  };
  require_macos();
  let cases: Vec<Case> = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
  assert_eq!(cases.len(), 2);
  for case in cases {
    let source = fs::read(&case.probe_path).unwrap();
    let header = fs::read(&case.header_path).unwrap();
    let project = case.probe_path.parent().unwrap();
    let index = case
      .header_path
      .parent()
      .unwrap()
      .join(format!("mcp-index-{}", case.name));
    assert!(!index.exists(), "the isolated test index must be new");
    let env = ExtractionEnv {
      cpp_macro_compiler: Some(case.command),
      ..Default::default()
    };
    let build = vorpal_index::build_index_env(
      project,
      &index,
      Default::default(),
      Default::default(),
      &env,
    )
    .unwrap();
    assert_eq!(build.indexed, 1);
    let generation = fs::read(index.join("CURRENT")).unwrap();
    let mut server = Server::with_profile_env_rebuild_source(
      index.clone(),
      Profile::Full,
      env,
      false,
      Some(project.to_path_buf()),
    );
    for id in [1, 2] {
      let health = call(&mut server, id, "health", json!({}));
      assert!(
        health.contains("parse health: clean"),
        "{}: {health}",
        case.name
      );
    }
    assert_eq!(fs::read(index.join("CURRENT")).unwrap(), generation);
    // Only the external definition changes. No source hint or source/stat edit.
    fs::write(
      &case.header_path,
      b"#import <Foundation/Foundation.h>\n#define CHECK UNPROVEN\n#define HANDLE UNPROVEN\n",
    )
    .unwrap();
    assert!(call(&mut server, 3, "health", json!({})).contains("carry ERROR/MISSING nodes"));
    fs::remove_file(&case.header_path).unwrap();
    assert!(call(&mut server, 4, "health", json!({})).contains("carry ERROR/MISSING nodes"));
    fs::write(&case.header_path, header).unwrap();
    assert!(call(&mut server, 5, "health", json!({})).contains("parse health: clean"));
    assert_eq!(fs::read(index.join("CURRENT")).unwrap(), generation);
    let graph = call(
      &mut server,
      6,
      "graph",
      json!({"relation":"callees", "name":"run", "format":"lean"}),
    );
    assert!(graph.contains("malloc"), "{graph}");
    assert!(!graph.contains("CHECK"), "{graph}");
    assert_eq!(fs::read(&case.probe_path).unwrap(), source);
    eprintln!(
      "{}: normal MCP warm queries, external edit/removal/restoration and graph agree",
      case.name
    );
  }
}
