//! Normal watched MCP serves original include pieces and recaptures quiet input edits.
use serde_json::{Value, json};
use std::fs;
use vorpal_ingest::{CppTextualIncludeContext, ExtractionEnv};
use vorpal_mcp::{Profile, Server};

fn call(server: &mut Server, name: &str, arguments: Value) -> Value {
  let reply = server.handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}).to_string()).unwrap();
  serde_json::from_str(&reply).unwrap()
}
fn text(reply: &Value) -> &str {
  reply["result"]["content"][0]["text"].as_str().unwrap()
}

#[test]
fn watched_mcp_recaptures_common_parse_and_preserves_physical_call_sites() {
  for background in [false, true] {
    let base = std::env::temp_dir().join(format!(
      "vorpal-mcp-context-{}-{background}",
      std::process::id()
    ));
    let src = base.join("src");
    fs::create_dir_all(&src).unwrap();
    let root = src.join("root.cc");
    let head = src.join("head.cc");
    let tail = src.join("tail.cc");
    fs::write(&root, "static int left(int x) { return x; }\nstatic int next(int x) { return x; }\nnamespace Real {\n#include \"head.cc\"\n#include \"tail.cc\"\n}\n").unwrap();
    fs::write(&head, "int run(int x) {\n").unwrap();
    fs::write(&tail, "  return left(x);\n}\n").unwrap();
    let env = ExtractionEnv {
      cpp_textual_include_contexts: vec![CppTextualIncludeContext {
        root: root.clone(),
        includes: vec![head, tail.clone()],
      }],
      ..Default::default()
    };
    let index = src.join(".vorpal/index");
    vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
      .unwrap();
    let initial = fs::read(index.join("CURRENT")).unwrap();
    let mut server =
      Server::with_profile_env_rebuild(index.clone(), Profile::Full, env, background);
    let health = call(&mut server, "health", json!({}));
    assert_eq!(health["result"]["isError"], false, "{health}");
    assert!(text(&health).contains("parse health: clean"));
    let graph = call(
      &mut server,
      "graph",
      json!({"relation":"callees","name":"run","format":"lean"}),
    );
    assert_eq!(graph["result"]["isError"], false, "{graph}");
    assert!(text(&graph).contains("left"), "{graph}");
    let generation = vorpal_kg::resolve_index_dir(&index);
    let kg = vorpal_kg::Kg::load(&generation).unwrap();
    let run = (0..kg.node_count())
      .map(|id| vorpal_kg::NodeId::new(id as u64))
      .find(|&id| kg.node(id).unwrap().name == "run")
      .unwrap();
    let fetched = call(
      &mut server,
      "fetch_span",
      json!({"id":run.raw(),"max_bytes":4096}),
    );
    assert_eq!(fetched["result"]["isError"], false, "{fetched}");
    assert!(
      text(&fetched).contains("head.cc")
        && text(&fetched).contains("tail.cc")
        && text(&fetched).contains("return left(x)"),
      "{fetched}"
    );
    // Preserve both size and mtime: freshness must observe exact input bytes.
    let stamp = fs::metadata(&tail).unwrap().modified().unwrap();
    fs::write(&tail, "  return next(x);\n}\n").unwrap();
    fs::File::options()
      .write(true)
      .open(&tail)
      .unwrap()
      .set_times(fs::FileTimes::new().set_modified(stamp))
      .unwrap();
    assert!(matches!(
      vorpal_index::records::definition_context_parts(
        kg.node(run).unwrap().source_context.unwrap(),
        1
      ),
      Err(vorpal_index::records::SnippetError::Stale(_))
    ));
    let graph = call(
      &mut server,
      "graph",
      json!({"relation":"callees","name":"run","format":"lean"}),
    );
    assert_eq!(graph["result"]["isError"], false, "{graph}");
    let records = graph["result"]["structuredContent"]["records"]
      .as_array()
      .unwrap();
    assert!(records.iter().any(|row| row["name"] == "next"), "{graph}");
    assert!(!records.iter().any(|row| row["name"] == "left"), "{graph}");
    assert_ne!(fs::read(index.join("CURRENT")).unwrap(), initial);
    fs::remove_file(&tail).unwrap();
    let failed = call(
      &mut server,
      "graph",
      json!({"relation":"callees","name":"run","format":"lean"}),
    );
    assert_eq!(
      failed["result"]["isError"], true,
      "missing input cannot serve old graph: {failed}"
    );
    drop(server);
    fs::remove_dir_all(base).unwrap();
  }
}
