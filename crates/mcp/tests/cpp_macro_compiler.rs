#[path = "../../ingest/tests/support/cpp_compiler_provider.rs"]
mod provider;
use serde_json::{Value, json};
use std::fs;
use vorpal_ingest::ExtractionEnv;
use vorpal_mcp::{Profile, Server};

fn call(server: &mut Server, name: &str, arguments: Value) -> Value {
  serde_json::from_str(
    &server
      .handle_line(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
    "params":{"name":name,"arguments":arguments}})
        .to_string(),
      )
      .unwrap(),
  )
  .unwrap()
}

#[test]
fn normal_mcp_reports_capture_outages_without_serving_or_publishing_a_fallback_graph() {
  for background in [false, true] {
    let base = std::env::temp_dir().join(format!(
      "vorpal-mcp-compiler-outage-{}-{background}",
      std::process::id()
    ));
    let src = base.join("src");
    let sdk = base.join("sdk");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&sdk).unwrap();
    let path = src.join("main.cc");
    let source = "#include <unknown-sdk.h>\nvoid run() { CHECK(value())\nafter(); }\nvoid value() {}\nvoid after() {}\n";
    fs::write(&path, source).unwrap();
    fs::write(sdk.join("proof.h"), "#define CHECK(x) { sink(x); }\n").unwrap();
    let mut command = provider::command(&sdk, &path);
    command.timeout_seconds = 1;
    let env = ExtractionEnv {
      cpp_macro_compiler: Some(command),
      ..Default::default()
    };
    let index = src.join(".vorpal/index");
    vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
      .unwrap();
    let mut server =
      Server::with_profile_env_rebuild(index.clone(), Profile::Full, env, background);
    assert!(health(&mut server, 1).contains("parse health: clean"));
    let args = json!({"relation":"callees","name":"run","format":"lean"});
    let graph = call(&mut server, "graph", args.clone());
    assert_eq!(graph["result"]["isError"], false, "{graph}");
    let id = graph["result"]["structuredContent"]["records"][0]["id"].clone();
    let span = call(&mut server, "fetch_span", json!({"id":id}));
    assert_eq!(span["result"]["isError"], false, "{span}");
    let initial = fs::read(index.join("CURRENT")).unwrap();
    let directory = vorpal_kg::resolve_index_dir(&index);
    let bytes = provider::artifact_snapshot(&directory);
    for mode in ["unavailable", "exit", "timeout", "json", "missing-response"] {
      fs::write(sdk.join("mode"), mode).unwrap();
      for (tool, arguments) in [
        ("health", json!({})),
        ("graph", args.clone()),
        ("fetch_span", json!({"id":id})),
        ("index", json!({"src":src,"verify":true})),
      ] {
        let reply = call(&mut server, tool, arguments);
        assert_eq!(reply["result"]["isError"], true, "{mode}/{tool}: {reply}");
        assert_eq!(
          reply["result"]["structuredContent"]["code"],
          if tool == "index" { "tool-error" } else { "index-unavailable" },
          "{reply}"
        );
        let message = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
          message.contains("native C++ compiler recovery failed"),
          "{message}"
        );
        assert!(!message.contains("private-provider-stderr-secret"));
        assert!(!message.contains(sdk.to_str().unwrap()));
        // No old graph records may ride alongside an operational error.
        assert!(
          reply["result"]["structuredContent"]
            .get("records")
            .is_none()
        );
        assert_eq!(fs::read(index.join("CURRENT")).unwrap(), initial);
        assert_eq!(provider::artifact_snapshot(&directory), bytes);
      }
      fs::write(sdk.join("mode"), "").unwrap();
      assert!(health(&mut server, 2).contains("parse health: clean"));
      let restored = call(&mut server, "graph", args.clone());
      assert_eq!(
        restored["result"]["structuredContent"],
        graph["result"]["structuredContent"]
      );
      let restored_span = call(&mut server, "fetch_span", json!({"id":id}));
      assert_eq!(restored_span["result"], span["result"]);
      assert_eq!(fs::read(index.join("CURRENT")).unwrap(), initial);
    }
    fs::write(sdk.join("mode"), "decline").unwrap();
    assert!(health(&mut server, 3).contains("carry ERROR/MISSING nodes"));
    fs::write(sdk.join("mode"), "").unwrap();
    assert!(health(&mut server, 4).contains("parse health: clean"));
    assert_eq!(fs::read(index.join("CURRENT")).unwrap(), initial);
    drop(server);
    assert_eq!(fs::read_to_string(path).unwrap(), source);
    fs::remove_dir_all(base).unwrap();
  }
}

fn health(server: &mut Server, id: u64) -> String {
  let reply = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"health","arguments":{}}}).to_string()).unwrap();
  let reply: Value = serde_json::from_str(&reply).unwrap();
  assert_eq!(reply["result"]["isError"], false, "{reply}");
  reply["result"]["content"][0]["text"]
    .as_str()
    .unwrap()
    .to_owned()
}

#[test]
fn quiet_normal_mcp_queries_recapture_native_only_inputs_instead_of_replaying() {
  for background in [false, true] {
    let base = std::env::temp_dir().join(format!(
      "vorpal-mcp-compiler-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    let src = base.join("src");
    let sdk = base.join("sdk");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&sdk).unwrap();
    let source = "#include <unknown-sdk.h>\nvoid run() { CHECK(value())\nafter(); }\n#undef CHECK\nvoid CHECK(int);\nvoid ordinary() { CHECK(other()); }\nvoid value() {}\nvoid after() {}\nvoid other() {}\n";
    let path = src.join("main.cc");
    fs::write(&path, source).unwrap();
    fs::write(sdk.join("proof.h"), "#define CHECK(x) { sink(x); }\n").unwrap();
    let env = ExtractionEnv {
      cpp_macro_compiler: Some(provider::command(&sdk, &path)),
      ..Default::default()
    };
    let index = src.join(".vorpal/index");
    vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
      .unwrap();
    let mut server =
      Server::with_profile_env_rebuild(index.clone(), Profile::Full, env, background);
    let first = health(&mut server, 1);
    assert!(first.contains("parse health: clean"), "{first}");
    let initial = fs::read(index.join("CURRENT")).unwrap();
    let before = fs::read_to_string(sdk.join("runs"))
      .unwrap()
      .lines()
      .count();
    assert!(health(&mut server, 2).contains("parse health: clean"));
    assert!(
      fs::read_to_string(sdk.join("runs"))
        .unwrap()
        .lines()
        .count()
        > before
    );
    assert_eq!(fs::read(index.join("CURRENT")).unwrap(), initial);
    // The observer has no dependency on this native-only header. The source and
    // observed header are unchanged; quiet queries must still run native PP.
    let hidden = sdk.join("native-only.h");
    fs::write(&hidden, "stable").unwrap();
    let mtime = fs::metadata(&hidden).unwrap().modified().unwrap();
    fs::write(&hidden, "changed").unwrap();
    fs::File::options()
      .write(true)
      .open(&hidden)
      .unwrap()
      .set_times(fs::FileTimes::new().set_modified(mtime))
      .unwrap();
    assert!(health(&mut server, 3).contains("carry ERROR/MISSING nodes"));
    fs::remove_file(&hidden).unwrap();
    assert!(health(&mut server, 4).contains("parse health: clean"));
    assert_eq!(fs::read(index.join("CURRENT")).unwrap(), initial);
    let response = server.handle_line(&json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"graph","arguments":{"relation":"callees","name":"run","format":"lean"}}}).to_string()).unwrap();
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["result"]["isError"], false, "{response}");
    let records = response["result"]["structuredContent"]["records"]
      .as_array()
      .unwrap();
    assert_eq!(records.len(), 2, "{response}");
    for name in ["value", "after"] {
      assert!(records.iter().any(|r| r["name"] == name), "{response}");
    }
    drop(server);
    assert_eq!(fs::read_to_string(path).unwrap(), source);
    fs::remove_dir_all(base).unwrap();
  }
}

#[test]
fn frozen_external_index_cannot_claim_native_compiler_freshness() {
  let dir = std::env::temp_dir().join(format!("vorpal-frozen-compiler-{}", std::process::id()));
  fs::create_dir_all(&dir).unwrap();
  let path = dir.join("main.cc");
  let env = ExtractionEnv {
    cpp_macro_compiler: Some(provider::command(&dir, &path)),
    ..Default::default()
  };
  // No authorized source root exists for this external index.
  let mut server =
    Server::with_profile_env_rebuild(dir.join("external-index"), Profile::Full, env, false);
  let reply = server.handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"health","arguments":{}}}).to_string()).unwrap();
  let reply: Value = serde_json::from_str(&reply).unwrap();
  assert_eq!(reply["result"]["isError"], true, "{reply}");
  assert!(
    reply["result"]["content"][0]["text"]
      .as_str()
      .unwrap()
      .contains("authorized source root")
  );
  drop(server);
  fs::remove_dir_all(dir).unwrap();
}
