//! Project configuration reaches CLI extraction and every MCP rebuild.
use serde_json::{Value, json};
use std::{
  fs,
  io::Write,
  process::{Command, Stdio},
};

fn index(src: &std::path::Path, out: &std::path::Path, config: &std::path::Path) {
  let output = Command::new(env!("CARGO_BIN_EXE_vorpal"))
    .args(["index", "--verify", "--config"])
    .arg(config)
    .arg(src)
    .arg("--out")
    .arg(out)
    .output()
    .unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
}

fn mcp_rebuild(src: &std::path::Path, out: &std::path::Path, config: &std::path::Path) -> Value {
  let mut child = Command::new(env!("CARGO_BIN_EXE_vorpal"))
    .current_dir(src)
    .args(["mcp", "--no-watch-rebuild", "--profile", "full", "--config"])
    .arg(config)
    .arg("--index")
    .arg(out)
    .env("VORPAL_NO_AUTOWARM", "1")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  let requests = [
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"config-test","version":"1"}}}),
    json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index","arguments":{"src":src}}}),
    json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"health","arguments":{}}}),
  ];
  let mut stdin = child.stdin.take().unwrap();
  for request in requests {
    writeln!(stdin, "{request}").unwrap();
  }
  drop(stdin);
  let output = child.wait_with_output().unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  let replies: Vec<Value> = String::from_utf8(output.stdout)
    .unwrap()
    .lines()
    .filter_map(|line| serde_json::from_str(line).ok())
    .collect();
  let rebuilt = replies.iter().find(|r| r["id"] == 2).unwrap();
  assert_eq!(rebuilt["result"]["isError"], false, "{rebuilt}");
  replies.into_iter().find(|r| r["id"] == 3).unwrap()
}

#[test]
fn configured_roots_and_external_header_edits_reach_cli_and_mcp() {
  let temp = tempfile::tempdir().unwrap();
  let src = temp.path().join("src");
  let cfg = temp.path().join("settings");
  let sdk = cfg.join("sdk");
  fs::create_dir(&src).unwrap();
  fs::create_dir_all(&sdk).unwrap();
  let config = cfg.join("vorpalconfig.yml");
  fs::write(&config, "ruleDirs: []\ncppMacroIncludeRoots: [sdk]\n").unwrap();
  let header = sdk.join("proof.h");
  fs::write(&header, "#define CHECK(x) { effect(x); }\n#if defined(PLATFORM)\nstruct First {};\n#else\nstruct Second {};\n#endif\n").unwrap();
  fs::write(
    src.join("run.cc"),
    "#include <proof.h>\nvoid run() { CHECK(value()) after(); }\n",
  )
  .unwrap();
  let out = temp.path().join("index");
  index(&src, &out, &config);
  assert!(
    vorpal_index::parse_health_report(&out)
      .unwrap()
      .contains("parse health: clean")
  );
  // The source directory has no config: a rediscovered child environment loses recovery.
  let response = mcp_rebuild(&src, &out, &config);
  assert_eq!(response["result"]["isError"], false, "{response}");
  assert!(
    response["result"]["content"][0]["text"]
      .as_str()
      .unwrap()
      .contains("parse health: clean"),
    "{response}"
  );
  fs::write(
    &header,
    "#define CHECK(x) { effect(x); }\n#if defined(PLATFORM)\n#undef CHECK\n#endif\n",
  )
  .unwrap();
  let response = mcp_rebuild(&src, &out, &config);
  assert!(
    response["result"]["content"][0]["text"]
      .as_str()
      .unwrap()
      .contains("carry ERROR nodes"),
    "{response}"
  );
  index(&src, &out, &config);
  assert!(
    vorpal_index::parse_health_report(&out)
      .unwrap()
      .contains("carry ERROR nodes")
  );
}

#[test]
fn empty_roots_enable_local_includes_and_absent_config_keeps_default_errors() {
  let temp = tempfile::tempdir().unwrap();
  let src = temp.path().join("src");
  fs::create_dir(&src).unwrap();
  fs::write(src.join("proof.h"), "#define CHECK(x) { effect(x); }\n").unwrap();
  fs::write(
    src.join("run.cc"),
    "#include \"proof.h\"\nvoid run() { CHECK(value()) after(); }\n",
  )
  .unwrap();
  let config = temp.path().join("vorpalconfig.yml");
  let out = temp.path().join("index");
  fs::write(&config, "ruleDirs: []\n").unwrap();
  index(&src, &out, &config);
  assert!(
    vorpal_index::parse_health_report(&out)
      .unwrap()
      .contains("carry ERROR nodes")
  );
  fs::write(&config, "ruleDirs: []\ncppMacroIncludeRoots: []\n").unwrap();
  index(&src, &out, &config);
  assert!(
    vorpal_index::parse_health_report(&out)
      .unwrap()
      .contains("parse health: clean")
  );
}

#[test]
fn repeated_once_headers_recover_in_mcp_and_header_edits_restore_errors() {
  let temp = tempfile::tempdir().unwrap();
  let src = temp.path().join("src");
  let sdk = temp.path().join("sdk");
  fs::create_dir(&src).unwrap();
  fs::create_dir(&sdk).unwrap();
  let config = temp.path().join("vorpalconfig.yml");
  fs::write(&config, "ruleDirs: []\ncppMacroIncludeRoots: [sdk]\n").unwrap();
  let header = sdk.join("once.h");
  fs::write(&header, "#pragma once\n#undef CHECK\n").unwrap();
  fs::write(src.join("run.cc"), "#include <once.h>\n#define CHECK(x) { effect(x); }\n#include <once.h>\nvoid run() { CHECK(value()) after(); }\n").unwrap();
  let out = temp.path().join("index");
  index(&src, &out, &config);
  let health = |response: Value| {
    response["result"]["content"][0]["text"]
      .as_str()
      .unwrap()
      .to_owned()
  };
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("parse health: clean"));
  fs::write(&header, "#undef CHECK\n").unwrap();
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("carry ERROR nodes"));
  fs::write(&header, "#pragma once\n#undef CHECK\n").unwrap();
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("parse health: clean"));
}
