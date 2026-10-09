//! Project configuration reaches CLI extraction and every MCP rebuild.
#[path = "../../ingest/tests/support/cpp_compiler_provider.rs"]
mod provider;
use serde_json::{Value, json};
use std::{
  fs,
  io::Write,
  process::{Command, Stdio},
};

#[test]
fn explicit_fresh_compiler_configuration_survives_external_config_mcp_rebuilds() {
  let temp = fixture_dir();
  let src = temp.path().join("src");
  let settings = temp.path().join("settings");
  let sdk = settings.join("driver");
  fs::create_dir(&src).unwrap();
  fs::create_dir_all(&sdk).unwrap();
  let path = src.join("main.cc");
  let source = "#include <unknown-sdk.h>\nvoid run() { CHECK(value())\nafter(); }\nvoid value() {}\nvoid after() {}\n";
  fs::write(&path, source).unwrap();
  fs::write(sdk.join("proof.h"), "#define CHECK(x) { sink(x); }\n").unwrap();
  let mut command = provider::command(&sdk, &path);
  command.directory = "driver".into();
  command.translation_units = vec!["../src/main.cc".into()];
  let config = settings.join("vorpalconfig.yml");
  fs::write(
    &config,
    serde_yaml::to_string(&json!({"ruleDirs":[],"cppMacroCompiler":command})).unwrap(),
  )
  .unwrap();
  let out = temp.path().join("index");
  index(&src, &out, &config);
  assert!(
    vorpal_index::parse_health_report(&out)
      .unwrap()
      .contains("parse health: clean")
  );
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("parse health: clean"));
  fs::write(sdk.join("native-only.h"), "changed").unwrap();
  index(&src, &out, &config);
  assert!(
    vorpal_index::parse_health_report(&out)
      .unwrap()
      .contains("carry ERROR/MISSING nodes")
  );
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("carry ERROR/MISSING nodes"));
  fs::remove_file(sdk.join("native-only.h")).unwrap();
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("parse health: clean"));
  assert_eq!(fs::read_to_string(path).unwrap(), source);
}

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
    .arg("--src")
    .arg(src)
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
  let temp = fixture_dir();
  let src = temp.path().join("src");
  let cfg = temp.path().join("settings");
  let sdk = cfg.join("sdk");
  fs::create_dir(&src).unwrap();
  fs::create_dir_all(&sdk).unwrap();
  let config = cfg.join("vorpalconfig.yml");
  fs::write(&config, "ruleDirs: []\ncppMacroIncludeRoots: [sdk]\n").unwrap();
  let header = sdk.join("proof.h");
  fs::write(&header, "#define UNUSED() __pragma(pop_macro(\"CHECK\"))\n#define WRAPPER() UNUSED()\n#define CHECK(x) { effect(x); }\n#if defined(PLATFORM)\nstruct First {};\n#else\nstruct Second {};\n#endif\n").unwrap();
  fs::write(
    src.join("run.cc"),
    "#include <proof.h>\nvoid run() { CHECK /* invocation */ (value()) after(); }\n",
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
      .contains("carry ERROR/MISSING nodes"),
    "{response}"
  );
  index(&src, &out, &config);
  assert!(
    vorpal_index::parse_health_report(&out)
      .unwrap()
      .contains("carry ERROR/MISSING nodes")
  );
  // A later header edit can alter the replacement's syntax without redefining
  // CHECK itself. Normal MCP rebuilds must decline that expanded body too.
  fs::write(
    &header,
    "#define CHECK(x) { effect(x); }\n#define effect }\n",
  )
  .unwrap();
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("carry ERROR/MISSING nodes"));
  fs::write(&header, "#define CHECK(x) { effect(x); }\n").unwrap();
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("parse health: clean"));
  // A statement replacement is not valid in a return-expression slot, even
  // though the unexpanded C++ call tree has no ERROR or MISSING nodes.
  fs::write(src.join("run.cc"), "#include <proof.h>\nint run() { return CHECK(value()); }\n").unwrap();
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("carry ERROR/MISSING nodes"));
  index(&src, &out, &config);
  assert!(vorpal_index::parse_health_report(&out).unwrap().contains("macro-context diagnostics"));
}

#[test]
fn empty_roots_enable_local_includes_and_absent_config_keeps_default_errors() {
  let temp = fixture_dir();
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
      .contains("carry ERROR/MISSING nodes")
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
  let temp = fixture_dir();
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
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("parse health: clean"));
  fs::write(&header, "#undef CHECK\n").unwrap();
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("carry ERROR/MISSING nodes"));
  fs::write(&header, "#pragma once\n#undef CHECK\n").unwrap();
  assert!(health(mcp_rebuild(&src, &out, &config)).contains("parse health: clean"));
}

fn fixture_dir() -> tempfile::TempDir {
  let path = std::env::temp_dir();
  #[cfg(unix)]
  let path = path.canonicalize().unwrap_or(path);
  tempfile::tempdir_in(path).unwrap()
}

fn health(response: Value) -> String {
  assert_eq!(response["result"]["isError"], false, "{response}");
  response["result"]["content"][0]["text"]
    .as_str()
    .unwrap()
    .to_owned()
}

#[test]
fn normal_mcp_health_reports_missing_only_tokens_on_warm_products() {
  let temp = fixture_dir();
  let src = temp.path().join("src");
  fs::create_dir(&src).unwrap();
  fs::write(src.join("missing.cc"), "int damaged() { return 1 }\n").unwrap();
  let config = temp.path().join("vorpalconfig.yml");
  fs::write(&config, "ruleDirs: []\ncppMacroIncludeRoots: []\n").unwrap();
  let out = temp.path().join("index");
  index(&src, &out, &config);
  for _ in 0..2 {
    let report = health(mcp_rebuild(&src, &out, &config));
    assert!(
      report.contains("1 of 1 files carry ERROR/MISSING nodes"),
      "{report}"
    );
    assert!(report.contains("0 damaged bytes"), "{report}");
    assert!(report.contains("missing.cc"), "{report}");
  }
}
#[test]
fn explicit_mcp_source_keeps_external_index_headers_fresh_without_index_calls() {
  use std::io::{BufRead, BufReader};
  use std::time::Duration;
  struct Running(std::process::Child);
  impl Drop for Running {
    fn drop(&mut self) {
      let _ = self.0.kill();
      let _ = self.0.wait();
    }
  }
  for default_index in [false, true] {
    let temp = fixture_dir();
    let src = temp.path().join("source tree");
    let settings = temp.path().join("settings");
    let headers = settings.join("headers");
    fs::create_dir(&src).unwrap();
    fs::create_dir_all(&headers).unwrap();
    let config = settings.join("vorpalconfig.yml");
    fs::write(&config, "ruleDirs: []\ncppMacroIncludeRoots: [headers]\n").unwrap();
    fs::write(
      src.join("main.cc"),
      "#include <proof.h>\nvoid run() { CHECK(target()) }\nvoid target() {}\n",
    )
    .unwrap();
    let header = headers.join("proof.h");
    let valid = "#define CHECK(x) { consume(x); }\n";
    fs::write(&header, valid).unwrap();
    let out = if default_index {
      src.join(".vorpal/index")
    } else {
      temp.path().join("external-index")
    };
    index(&src, &out, &config);
    let mut command = Command::new(env!("CARGO_BIN_EXE_vorpal"));
    command
      .current_dir(temp.path())
      .args(["mcp", "--src"])
      .arg(&src)
      .args(["--no-watch-rebuild", "--config"])
      .arg(&config);
    if !default_index {
      command.arg("--index").arg(&out);
    }
    let mut child = Running(
      command
        .env("VORPAL_NO_AUTOWARM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(fs::File::create(temp.path().join("mcp.log")).unwrap())
        .spawn()
        .unwrap(),
    );
    let mut input = child.0.stdin.take().unwrap();
    let output = child.0.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
      for line in BufReader::new(output).lines() {
        let Ok(line) = line else {
          break;
        };
        if let Ok(reply) = serde_json::from_str::<Value>(&line) {
          if tx.send(reply).is_err() {
            break;
          }
        }
      }
    });
    for (id, replacement, expected) in [
      (1, valid, "parse health: clean"),
      (
        2,
        "#define CHECK(x) consume(x)\n",
        "carry ERROR/MISSING nodes",
      ),
      (3, valid, "parse health: clean"),
    ] {
      let stamp = fs::metadata(&header).unwrap().modified().unwrap();
      fs::write(&header, replacement).unwrap();
      fs::File::options()
        .write(true)
        .open(&header)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(stamp))
        .unwrap();
      writeln!(input,"{}",json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"health","arguments":{}}})).unwrap();
      input.flush().unwrap();
      let response = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("running MCP health response");
      assert_eq!(response["id"], id, "{response}");
      assert_eq!(response["result"]["isError"], false, "{response}");
      assert!(health(response).contains(expected));
    }
    drop(input);
    assert!(child.0.wait().unwrap().success());
    reader.join().unwrap();
  }
}

#[test]
fn explicit_mcp_source_rejects_missing_roots_and_project_mode_conflicts() {
  let temp = fixture_dir();
  let file = temp.path().join("file");
  fs::write(&file, "source root must be a directory").unwrap();
  for src in [file, temp.path().join("missing")] {
    let output = Command::new(env!("CARGO_BIN_EXE_vorpal"))
      .current_dir(temp.path())
      .args(["mcp", "--src"])
      .arg(src)
      .stdin(Stdio::null())
      .output()
      .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("MCP source"));
  }
  let output = Command::new(env!("CARGO_BIN_EXE_vorpal"))
    .current_dir(temp.path())
    .args(["mcp", "--projects", "--src"])
    .arg(temp.path())
    .stdin(Stdio::null())
    .output()
    .unwrap();
  assert!(!output.status.success());
  assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
}
