//! Protocol fixture, not a native compiler equivalence test.
use serde_json::{Value, json};
use std::{
  fs,
  path::Path,
  process,
  time::{Duration, Instant},
};

pub fn command(
  directory: &Path,
  source: &Path,
) -> vorpal_ingest::cpp_macro_compiler::CompilerCommand {
  vorpal_ingest::cpp_macro_compiler::CompilerCommand {
    program: std::env::current_exe().unwrap(),
    arguments: vec![
      "--exact".into(),
      "provider::compiler_provider_process".into(),
      "--nocapture".into(),
    ],
    directory: directory.to_owned(),
    translation_units: vec![source.to_owned()],
    timeout_seconds: 3,
  }
}

#[test]
fn compiler_provider_process() {
  let Some(request) = std::env::var_os("VORPAL_CPP_COMPILER_REQUEST") else {
    return;
  };
  let ready = std::env::var_os("VORPAL_CPP_COMPILER_READY").unwrap();
  let deadline = Instant::now() + Duration::from_secs(2);
  while !Path::new(&ready).exists() {
    assert!(Instant::now() < deadline);
    std::thread::sleep(Duration::from_millis(1));
  }
  let directory = std::env::current_dir().unwrap();
  let mode = fs::read_to_string(directory.join("mode")).unwrap_or_default();
  if std::env::var_os("VORPAL_COMPILER_FIXTURE_CHILD").is_some() {
    fs::write(directory.join("child.pid"), process::id().to_string()).unwrap();
    std::thread::sleep(Duration::from_secs(30));
    return;
  }
  if mode == "descendant" {
    let mut child = process::Command::new(std::env::current_exe().unwrap())
      .args([
        "--exact",
        "provider::compiler_provider_process",
        "--nocapture",
      ])
      .env("VORPAL_COMPILER_FIXTURE_CHILD", "1")
      .spawn()
      .unwrap();
    child.wait().unwrap();
  }
  if mode == "timeout" {
    std::thread::sleep(Duration::from_secs(30));
  }
  if mode == "volume" {
    fs::File::create(
      Path::new(&request)
        .parent()
        .unwrap()
        .join("native-before.i"),
    )
    .unwrap()
    .set_len(129 * 1024 * 1024)
    .unwrap();
    std::thread::sleep(Duration::from_secs(30));
  }
  if mode == "exit" {
    process::exit(9);
  }
  let response = std::env::var_os("VORPAL_CPP_COMPILER_RESPONSE").unwrap();
  if mode == "json" {
    fs::write(response, b"{broken").unwrap();
    return;
  }
  let request: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
  let source = request["source"].as_str().unwrap();
  let path = Path::new(request["path"].as_str().unwrap());
  assert_eq!(fs::read_to_string(path).unwrap(), source);
  let header_path = directory.join("proof.h");
  let Ok(header) = fs::read_to_string(&header_path) else {
    process::exit(8);
  };
  let invocation = if mode.starts_with("x-generator") { "CHECK(DISPATCH)" } else if mode == "native-operators" { "CHECK(value)" } else { "CHECK(value())" };
  let start = source.find(invocation).unwrap();
  let native = if fs::read_to_string(directory.join("native-only.h")).is_ok_and(|s| s == "changed")
  {
    vec!["native", "changed"]
  } else {
    vec!["native", "stable"]
  };
  let mut packet = json!({
    "version":1,"requestId":request["requestId"],"path":path,"source":source,
    "complete":true,"volatileInputs":false,
    "nativeContextBefore":"native-v1","nativeContextAfter":"native-v1","observedContext":"observer-v1",
    "nativeBefore":native,"nativeAfter":native,"observedTokens":["native","stable"],"nativeDirectives":[],
    "definitions":[{"path":header_path,"source":header}],
    "expansions":[{"name":"CHECK","start":start,"end":start+invocation.len(),
      "definition":{"buffer":0,"nameOffset":header.find("CHECK").unwrap(),"end":header.trim_end().len(),"parameters":1}}],
    "expandedNames":["CHECK"],"calleeSites":[{"name":"CHECK","start":start}]
  });
  if mode.starts_with("x-generator") {
    let tokens = vec!["if", "(", "ready", ")", "{", "first", "(", ")", ";", "}", "if", "(", "ready", ")", "{", "second", "(", ")", ";", "}"];
    packet["nativeBefore"] = json!(tokens);
    packet["nativeAfter"] = json!(tokens);
    packet["observedTokens"] = json!(tokens);
    packet["observedTokenSites"] = json!(tokens.iter().map(|_| json!({"offset":start,"fromMacro":true})).collect::<Vec<_>>());
    packet["expandedNames"] = json!(["CHECK", "DISPATCH"]);
    packet["expansions"][0]["definition"]["end"] = json!(header.lines().next().unwrap().trim_end().len());
    if mode == "x-generator-offset" { packet["observedTokenSites"][0]["offset"] = json!(source.len()); }
    if mode == "x-generator-origin" { packet["observedTokenSites"][0]["fromMacro"] = json!(false); }
    if mode == "x-generator-length" { packet["observedTokenSites"].as_array_mut().unwrap().pop(); }
    if mode == "x-generator-unclosed" {
      for key in ["nativeBefore", "nativeAfter", "observedTokens", "observedTokenSites"] { packet[key].as_array_mut().unwrap().pop(); }
    }
  }
  if mode == "definitions" {
    let start = source.find("DECL()").unwrap();
    packet["expansions"]
      .as_array_mut()
      .unwrap()
      .push(json!({"name":"DECL","start":start,"end":start+6,"definition":null}));
    packet["expandedNames"]
      .as_array_mut()
      .unwrap()
      .push(json!("DECL"));
    packet["calleeSites"]
      .as_array_mut()
      .unwrap()
      .push(json!({"name":"DECL","start":start}));
  }
  if mode == "type-definitions" {
    let start = source.find("struct TYPE").unwrap() + "struct ".len();
    packet["expansions"]
      .as_array_mut()
      .unwrap()
      .push(json!({"name":"TYPE","start":start,"end":start+4,"definition":null}));
    packet["expandedNames"]
      .as_array_mut()
      .unwrap()
      .push(json!("TYPE"));
    packet["calleeSites"]
      .as_array_mut()
      .unwrap()
      .push(json!({"name":"TYPE","start":start}));
  }
  match mode.as_str() {
    "stale" => packet["requestId"] = json!("previous-request"),
    "source" => packet["source"] = json!("stale source"),
    "context" => packet["nativeContextAfter"] = json!("different"),
    "after" => packet["nativeAfter"] = json!(["changed"]),
    "incomplete" => packet["complete"] = json!(false),
    "volatile" => packet["volatileInputs"] = json!(true),
    "directive" => packet["nativeDirectives"] = json!(["#pragma unknown"]),
    "anchor" => packet["expansions"][0]["definition"]["end"] = json!(1),
    "callee" => packet["calleeSites"][0]["start"] = json!(start + 1),
    "header" => {
      packet["definitions"][0]["source"] = json!("#define CHECK(x) { false_proof(x); }\n")
    }
    _ => (),
  }
  let mut runs = fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(directory.join("runs"))
    .unwrap();
  use std::io::Write;
  writeln!(runs, "capture").unwrap();
  fs::write(response, serde_json::to_vec(&packet).unwrap()).unwrap();
}
