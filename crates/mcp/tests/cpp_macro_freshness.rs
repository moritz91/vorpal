//! A running, watched MCP server must revalidate macro proofs without source events.
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use vorpal_ingest::ExtractionEnv;
use vorpal_mcp::{Profile, Server};

fn health(server: &mut Server, id: u64) -> String {
  let response = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"health","arguments":{}}}).to_string()).unwrap();
  let response: Value = serde_json::from_str(&response).unwrap();
  assert_eq!(response["result"]["isError"], false, "{response}");
  response["result"]["content"][0]["text"]
    .as_str()
    .unwrap()
    .to_owned()
}

#[test]
fn running_server_revalidates_external_macro_headers_without_source_events() {
  external_header_freshness(false);
}

#[test]
fn background_enabled_server_revalidates_external_macro_headers() {
  external_header_freshness(true);
}

fn external_header_freshness(watch_rebuild: bool) {
  let base: PathBuf = std::env::temp_dir().join(format!(
    "vorpal-mcp-macro-fresh-{}-{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_nanos()
  ));
  let src = base.join("repo");
  let headers = base.join("headers");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir_all(&headers).unwrap();
  fs::write(
    src.join("calls.cc"),
    "#include \"proof.h\"\nvoid run() { CHECK(target()) }\nvoid target() {}\n",
  )
  .unwrap();
  let header = headers.join("proof.h");
  let valid = "#define UNUSED() __pragma(pop_macro(\"CHECK\"))\n#define WRAPPER() UNUSED()\n#define CHECK(x) { consume(x); }\n";
  fs::write(&header, valid).unwrap();
  let env = ExtractionEnv {
    cpp_macro_include_roots: Some(vec![headers]),
    ..Default::default()
  };
  let index = src.join(".vorpal/index");
  vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
    .unwrap();
  let mut server = Server::with_profile_env_rebuild(index, Profile::Full, env, watch_rebuild);
  assert!(health(&mut server, 1).contains("parse health: clean"));
  // Drain the startup gap before changing only an unwatched external input.
  assert!(health(&mut server, 6).contains("parse health: clean"));
  let before = fs::metadata(&header).unwrap().modified().unwrap();
  let expression = valid.replace("{ consume(x); }", "  consume(x)   ");
  assert_eq!(valid.len(), expression.len());
  fs::write(&header, expression).unwrap();
  fs::File::options()
    .write(true)
    .open(&header)
    .unwrap()
    .set_times(fs::FileTimes::new().set_modified(before))
    .unwrap();
  server.tick();
  assert!(
    health(&mut server, 2).contains("carry ERROR/MISSING nodes"),
    "external header edit must invalidate the running server's proof"
  );
  fs::write(&header, valid).unwrap();
  assert!(health(&mut server, 3).contains("parse health: clean"));
  fs::write(&header, format!("{valid}WRAPPER();\n")).unwrap();
  assert!(health(&mut server, 8).contains("carry ERROR/MISSING nodes"));
  fs::write(&header, valid).unwrap();
  assert!(health(&mut server, 9).contains("parse health: clean"));
  fs::remove_file(&header).unwrap();
  assert!(health(&mut server, 4).contains("carry ERROR/MISSING nodes"));
  fs::write(&header, valid).unwrap();
  assert!(health(&mut server, 5).contains("parse health: clean"));
  fs::write(src.join("calls.cc"), "#include \"proof.h\"\nint run() { return CHECK(target()); }\nvoid target() {}\n").unwrap();
  assert!(health(&mut server, 10).contains("macro-context diagnostics"));
  fs::write(&header, "#undef CHECK\n").unwrap();
  assert!(health(&mut server, 11).contains("parse health: clean"));
  fs::write(&header, valid).unwrap();
  assert!(health(&mut server, 12).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), "#include \"proof.h\"\nvoid run() { CHECK(target()) }\nvoid target() {}\n").unwrap();
  assert!(health(&mut server, 13).contains("parse health: clean"));
  fs::write(src.join("calls.cc"), "#include \"proof.h\"\nnamespace scope { CHECK(target()); }\nvoid target() {}\n").unwrap();
  assert!(health(&mut server, 14).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), "#include \"proof.h\"\nnamespace scope { void run() { CHECK(target()) } }\nvoid target() {}\n").unwrap();
  assert!(health(&mut server, 15).contains("parse health: clean"));
  let declarations = "#define SDK_BEGIN namespace sdk {\n#define SDK_END }\n#if defined(ENABLE)\nSDK_BEGIN\nextern const int variable;\nSDK_END\n#endif\n";
  fs::write(&header, format!("{valid}{declarations}")).unwrap();
  assert!(health(&mut server, 16).contains("parse health: clean"));
  fs::write(&header, format!("{valid}{}", declarations.replace("defined(ENABLE)", "EXPANDING"))).unwrap();
  assert!(health(&mut server, 17).contains("carry ERROR/MISSING nodes"));
  fs::write(&header, format!("{valid}{declarations}")).unwrap();
  assert!(health(&mut server, 18).contains("parse health: clean"));
  let literal_pragmas = "#pragma pack(push, 1)\n#pragma warning(push, 1)\n#pragma warning(disable: 4100 4996)\n#pragma warning(pop)\n#pragma pack(pop)\n";
  fs::write(&header, format!("{literal_pragmas}{valid}{declarations}")).unwrap();
  assert!(health(&mut server, 19).contains("parse health: clean"));
  fs::write(&header, format!("{}{valid}{declarations}", literal_pragmas.replace("warning(push, 1)", "warning(push, LEVEL)"))).unwrap();
  assert!(health(&mut server, 20).contains("carry ERROR/MISSING nodes"));
  fs::write(&header, format!("{literal_pragmas}{valid}{declarations}")).unwrap();
  assert!(health(&mut server, 21).contains("parse health: clean"));
  let literal_conditions = declarations.replace("defined(ENABLE)", "0x10 == 020");
  fs::write(&header, format!("{valid}{literal_conditions}")).unwrap();
  assert!(health(&mut server, 22).contains("parse health: clean"));
  fs::write(&header, format!("{valid}{}", literal_conditions.replace("0x10 == 020", "UNKNOWN == 16"))).unwrap();
  assert!(health(&mut server, 23).contains("carry ERROR/MISSING nodes"));
  fs::write(&header, format!("{valid}{}", literal_conditions.replace("extern const int variable;", "#undef CHECK"))).unwrap();
  assert!(health(&mut server, 24).contains("carry ERROR/MISSING nodes"));
  fs::write(&header, format!("{valid}{literal_conditions}")).unwrap();
  assert!(health(&mut server, 25).contains("parse health: clean"));
  let objc_guard = "#ifdef __OBJC__
void run(Probe* object) { [object release]; }
void guarded() { @try { before(); } @catch (Probe* error) { forward([error description]); } }
namespace Sample { template<class T> struct Wrapper { id stored = [object description]; int count() { id local = [object description]; return [object count] + value(); } }; }
#endif
void following() { target(); }
void target() {}
";
  fs::write(src.join("calls.cc"), objc_guard).unwrap();
  assert!(health(&mut server, 26).contains("parse health: clean"));
  fs::write(src.join("calls.cc"), objc_guard.replace("__OBJC__", "PLATFORM")).unwrap();
  assert!(health(&mut server, 27).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), objc_guard).unwrap();
  assert!(health(&mut server, 28).contains("parse health: clean"));
  fs::write(src.join("calls.cc"), objc_guard.replace("(Probe* error)", "()")).unwrap();
  assert!(health(&mut server, 29).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), objc_guard).unwrap();
  assert!(health(&mut server, 30).contains("parse health: clean"));

  // Quiet queries retain the served generation rather than rebuilding forever.
  assert!(health(&mut server, 7).contains("parse health: clean"));
  drop(server);
  fs::remove_dir_all(base).unwrap();
}

#[test]
fn ignored_local_shadow_creation_and_removal_revalidates_running_proofs() {
  let base = std::env::temp_dir().join(format!(
    "vorpal-mcp-macro-shadow-{}-{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_nanos()
  ));
  let src = base.join("repo");
  let headers = base.join("headers");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir_all(headers.join(".private")).unwrap();
  fs::write(src.join(".gitignore"), ".private/\n").unwrap();
  fs::write(
    src.join("calls.cc"),
    "#include \".private/proof.h\"\nvoid run() { CHECK(target()) }\nvoid target() {}\n",
  )
  .unwrap();
  let valid = "#define CHECK(x) { consume(x); }\n";
  fs::write(headers.join(".private/proof.h"), valid).unwrap();
  let env = ExtractionEnv {
    cpp_macro_include_roots: Some(vec![headers]),
    ..Default::default()
  };
  let index = src.join(".vorpal/index");
  vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
    .unwrap();
  let mut server = Server::with_profile_env_rebuild(index, Profile::Full, env, false);
  assert!(health(&mut server, 1).contains("parse health: clean"));
  assert!(health(&mut server, 2).contains("parse health: clean"));
  fs::create_dir_all(src.join(".private")).unwrap();
  fs::write(
    src.join(".private/proof.h"),
    "#define CHECK(x) ((void)(x))\n",
  )
  .unwrap();
  assert!(health(&mut server, 3).contains("carry ERROR/MISSING nodes"));
  fs::remove_file(src.join(".private/proof.h")).unwrap();
  assert!(health(&mut server, 4).contains("parse health: clean"));
  #[cfg(unix)]
  {
    std::os::unix::fs::symlink(
      base.join("headers/.private/proof.h"),
      src.join(".private/proof.h"),
    )
    .unwrap();
    // Same bytes through a file redirect cannot preserve a compiler-consistent proof.
    assert!(health(&mut server, 5).contains("carry ERROR/MISSING nodes"));
    fs::remove_file(src.join(".private/proof.h")).unwrap();
    assert!(health(&mut server, 6).contains("parse health: clean"));
  }
  drop(server);
  fs::remove_dir_all(base).unwrap();
}
