#[path = "../../ingest/tests/support/cpp_compiler_provider.rs"]
mod provider;
use serde_json::{Value, json};
use std::fs;
use vorpal_ingest::ExtractionEnv;
use vorpal_mcp::{Profile, Server};

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
