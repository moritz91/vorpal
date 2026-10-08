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

fn assert_else_callees(server: &mut Server, id: u64) {
  let response = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"graph","arguments":{"relation":"callees","name":"run","format":"lean"}}}).to_string()).unwrap();
  let response: Value = serde_json::from_str(&response).unwrap();
  assert_eq!(response["result"]["isError"], false, "{response}");
  let records = response["result"]["structuredContent"]["records"].as_array().unwrap();
  assert_eq!(records.len(), 2, "{response}");
  for name in ["target", "after"] {
    let call = records.iter().find(|r| r["name"] == name).unwrap();
    assert_eq!(call["site"], "void run() { CHECK(target()) else after(); }");
    assert_eq!(call["site_line"], 2);
  }
}

#[test]
fn running_server_revalidates_external_macro_headers_without_source_events() {
  external_header_freshness(false, false);
}

#[test]
fn background_enabled_server_revalidates_external_macro_headers() {
  external_header_freshness(true, false);
}

#[test]
fn external_index_revalidates_external_macro_headers_with_explicit_source() {
  for watch_rebuild in [false, true] {
    external_header_freshness(watch_rebuild, true);
  }
}

fn external_header_freshness(watch_rebuild: bool, external_index: bool) {
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
  let index = if external_index {
    base.join("external-index")
  } else {
    src.join(".vorpal/index")
  };
  vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
    .unwrap();
  let mut server = Server::with_profile_env_rebuild_source(
    index,
    Profile::Full,
    env,
    watch_rebuild,
    external_index.then(|| src.clone()),
  );
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
@protocol ProbeProtocol
-(void) action;
@end
void run(Probe* object) { [object release]; consume(@selector(action)); selector(real()); }
void guarded() { @try { before(); } @catch (Probe* error) { forward([error description]); } }
namespace Sample { template<class T> struct Wrapper { id stored = @\"text\"; Wrapper() : stored([object copy]) { init(); } int count() { id local = [object format:@\"value\", payload()]; return [object range].location + value(); } }; }
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
  fs::write(src.join("calls.cc"), objc_guard.replace("@protocol ProbeProtocol", "@protocolProbeProtocol")).unwrap();
  assert!(health(&mut server, 31).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), objc_guard).unwrap();
  assert!(health(&mut server, 32).contains("parse health: clean"));
  fs::write(src.join("calls.cc"), objc_guard.replace("@selector(action)", "@selectorSuffix(action)")).unwrap();
  assert!(health(&mut server, 33).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), objc_guard).unwrap();
  assert!(health(&mut server, 34).contains("parse health: clean"));
  fs::write(src.join("calls.cc"), objc_guard.replace("[object range].location", "[object range].")).unwrap();
  assert!(health(&mut server, 35).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), objc_guard).unwrap();
  assert!(health(&mut server, 36).contains("parse health: clean"));
  fs::write(src.join("calls.cc"), objc_guard.replace("stored([object copy])", "stored([object copy]")).unwrap();
  assert!(health(&mut server, 37).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), objc_guard).unwrap();
  assert!(health(&mut server, 38).contains("parse health: clean"));
  fs::write(src.join("calls.cc"), objc_guard.replace("namespace Sample {", "namespace Sample { [object release];")).unwrap();
  assert!(health(&mut server, 39).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), objc_guard).unwrap();
  assert!(health(&mut server, 40).contains("parse health: clean"));

  let inverse = "#ifndef __OBJC__\nvoid ordinary() { before(); }\n#else\nvoid guarded() { [object release]; }\n#endif\nvoid following() { after(); }\n";
  fs::write(src.join("calls.cc"), inverse).unwrap();
  assert!(health(&mut server, 41).contains("parse health: clean"));
  fs::write(src.join("calls.cc"), inverse.replace("__OBJC__", "PLATFORM")).unwrap();
  assert!(health(&mut server, 42).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), inverse).unwrap();
  assert!(health(&mut server, 43).contains("parse health: clean"));
  fs::write(src.join("calls.cc"), inverse.replace("#else", "#else junk")).unwrap();
  assert!(health(&mut server, 44).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), objc_guard).unwrap();
  assert!(health(&mut server, 45).contains("parse health: clean"));


  let dangling = "#include <proof.h>\nvoid run() { CHECK(target()) else after(); }\nvoid after() {}\nvoid target() {}\n";
  let open = "#define CHECK(x) if (x) { consume(x); }\n";
  let closed = "#define CHECK(x)        { consume(x); }\n";
  assert_eq!(open.len(), closed.len());
  fs::write(&header, open).unwrap(); fs::write(src.join("calls.cc"), dangling).unwrap();
  assert!(health(&mut server, 46).contains("parse health: clean"));
  assert_else_callees(&mut server, 146);
  let modified = fs::metadata(&header).unwrap().modified().unwrap();
  fs::write(&header, closed).unwrap();
  fs::File::options().write(true).open(&header).unwrap().set_times(fs::FileTimes::new().set_modified(modified)).unwrap();
  assert!(health(&mut server, 47).contains("macro-context diagnostics"));
  fs::write(&header, open).unwrap();
  assert!(health(&mut server, 48).contains("parse health: clean"));
  assert_else_callees(&mut server, 148);
  fs::write(src.join("calls.cc"), dangling.replace("CHECK(target()) else", "CHECK(target()); else")).unwrap();
  assert!(health(&mut server, 49).contains("carry ERROR/MISSING nodes"));
  fs::write(src.join("calls.cc"), dangling).unwrap();
  assert!(health(&mut server, 50).contains("parse health: clean"));
  assert_else_callees(&mut server, 150);

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

#[test]
fn conditional_function_macro_callees_keep_both_original_sites() {
  for watch_rebuild in [false, true] {
    for crlf in [false, true] {
      let base = std::env::temp_dir().join(format!(
        "vorpal-mcp-conditional-sites-{}-{}",
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
      let header = headers.join("proof.h");
      let proof = "#define CHECK(v) { (v); }\n";
      fs::write(&header, proof).unwrap();
      let source = "#include \"proof.h\"\nint shared(int v) { return v; }\nint value() { return 1; }\n#if defined(WIDE)\nint wide(int argc) {\n#else\nint narrow(int argc) {\n#endif\n CHECK(shared(value()))\n return argc;\n}\n";
      fs::write(
        src.join("main.cc"),
        if crlf {
          source.replace('\n', "\r\n")
        } else {
          source.to_owned()
        },
      )
      .unwrap();
      let env = ExtractionEnv {
        cpp_macro_include_roots: Some(vec![headers]),
        ..Default::default()
      };
      let index = src.join(".vorpal/index");
      vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
        .unwrap();
      let mut server = Server::with_profile_env_rebuild(index, Profile::Full, env, watch_rebuild);
      let mut id = 1;
      for restored in [false, true] {
        assert!(health(&mut server, id).contains("parse health: clean"));
        id += 1;
        for (relation, name, expected) in [
          ("callees", "wide", ["shared", "value"]),
          ("callees", "narrow", ["shared", "value"]),
          ("callers", "shared", ["wide", "narrow"]),
        ] {
          let response = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"graph","arguments":{"relation":relation,"name":name,"format":"lean"}}}).to_string()).unwrap();
          let response: Value = serde_json::from_str(&response).unwrap();
          assert_eq!(response["result"]["isError"], false, "{response}");
          let rows = response["result"]["structuredContent"]["records"]
            .as_array()
            .unwrap();
          assert_eq!(rows.len(), 2, "{response}");
          assert_ne!(rows[0]["id"], rows[1]["id"], "{response}");
          for target in expected {
            let row = rows.iter().find(|r| r["name"] == target).unwrap();
            assert_eq!(row["site_line"], 9, "{response}");
            assert_eq!(row["site"], "CHECK(shared(value()))", "{response}");
          }
          id += 1;
        }
        if !restored {
          // Same-length external-header mutation cannot replay the prior proof,
          // even when its timestamp is restored and no source event arrives.
          let modified = fs::metadata(&header).unwrap().modified().unwrap();
          let expression = proof.replace("{ (v); }", "  (v)   ");
          assert_eq!(proof.len(), expression.len());
          fs::write(&header, expression).unwrap();
          fs::File::options()
            .write(true)
            .open(&header)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
          assert!(health(&mut server, id).contains("carry ERROR/MISSING nodes"));
          id += 1;
          fs::write(&header, proof).unwrap();
        }
      }
      drop(server);
      fs::remove_dir_all(base).unwrap();
    }
  }
}

#[test]
fn friend_macro_callees_are_owned_by_free_functions_after_header_revalidation() {
  for watch_rebuild in [false, true] {
    for crlf in [false, true] {
      let base = std::env::temp_dir().join(format!(
        "vorpal-mcp-friend-sites-{}-{}",
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
      let header = headers.join("proof.h");
      let proof = "#define CHECK(v) { (v); }\n";
      fs::write(&header, proof).unwrap();
      let source = "#include \"proof.h\"\nint target() { return 1; }\nstruct Column {\n inline friend void run() { CHECK(target()) }\n friend void inspect(Column&);\n int size() const { return 1; }\n};\n";
      fs::write(
        src.join("main.cc"),
        if crlf {
          source.replace('\n', "\r\n")
        } else {
          source.to_owned()
        },
      )
      .unwrap();
      let env = ExtractionEnv {
        cpp_macro_include_roots: Some(vec![headers]),
        ..Default::default()
      };
      let index = src.join(".vorpal/index");
      vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
        .unwrap();
      let mut server = Server::with_profile_env_rebuild(index, Profile::Full, env, watch_rebuild);
      let mut id = 1;
      for restored in [false, true] {
        assert!(health(&mut server, id).contains("parse health: clean"));
        id += 1;
        for (relation, name, expected) in
          [("callees", "run", "target"), ("callers", "target", "run")]
        {
          let response = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"graph","arguments":{"relation":relation,"name":name,"format":"lean"}}}).to_string()).unwrap();
          id += 1;
          let response: Value = serde_json::from_str(&response).unwrap();
          assert_eq!(response["result"]["isError"], false, "{response}");
          let records = response["result"]["structuredContent"]["records"]
            .as_array()
            .unwrap();
          assert_eq!(records.len(), 1, "{response}");
          assert_eq!(records[0]["name"], expected, "{response}");
          assert_eq!(
            records[0]["site"],
            "inline friend void run() { CHECK(target()) }"
          );
          assert_eq!(records[0]["site_line"], 4);
        }
        if !restored {
          let modified = fs::metadata(&header).unwrap().modified().unwrap();
          let expression = "#define CHECK(v)   (v)   \n";
          assert_eq!(expression.len(), proof.len());
          fs::write(&header, expression).unwrap();
          fs::File::options()
            .write(true)
            .open(&header)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
          assert!(health(&mut server, id).contains("carry ERROR/MISSING nodes"));
          id += 1;
          fs::write(&header, proof).unwrap();
        }
      }
      drop(server);
      fs::remove_dir_all(base).unwrap();
    }
  }
}

#[test]
fn logical_return_macro_callees_keep_guarded_sites_after_header_revalidation() {
  for watch_rebuild in [false, true] {
    for crlf in [false, true] {
      let base = std::env::temp_dir().join(format!(
        "vorpal-mcp-logical-return-sites-{}-{}",
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
      let header = headers.join("proof.h");
      let proof = "#define CHECK(v) { (v); }\n";
      fs::write(&header, proof).unwrap();
      let source = "#include \"proof.h\"\nbool base() { return true; }\nbool extra() { return false; }\nint value() { return 1; }\nbool run() {\n CHECK(value())\n return base()\n#ifdef ON\n || extra()\n#endif\n ;\n}\n";
      fs::write(
        src.join("main.cc"),
        if crlf {
          source.replace('\n', "\r\n")
        } else {
          source.to_owned()
        },
      )
      .unwrap();
      let env = ExtractionEnv {
        cpp_macro_include_roots: Some(vec![headers]),
        ..Default::default()
      };
      let index = src.join(".vorpal/index");
      vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
        .unwrap();
      let mut server = Server::with_profile_env_rebuild(index, Profile::Full, env, watch_rebuild);
      let mut id = 1;
      for restored in [false, true] {
        assert!(health(&mut server, id).contains("parse health: clean"));
        id += 1;
        let response = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"graph","arguments":{"relation":"callees","name":"run","format":"lean"}}}).to_string()).unwrap();
        id += 1;
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"]["isError"], false, "{response}");
        let records = response["result"]["structuredContent"]["records"]
          .as_array()
          .unwrap();
        assert_eq!(records.len(), 3, "{response}");
        for (name, line, site) in [
          ("value", 6, "CHECK(value())"),
          ("base", 7, "return base()"),
          ("extra", 9, "|| extra()"),
        ] {
          let row = records.iter().find(|r| r["name"] == name).unwrap();
          assert_eq!(row["site_line"], line, "{response}");
          assert_eq!(row["site"], site, "{response}");
        }
        if !restored {
          let modified = fs::metadata(&header).unwrap().modified().unwrap();
          let expression = "#define CHECK(v)   (v)   \n";
          assert_eq!(expression.len(), proof.len());
          fs::write(&header, expression).unwrap();
          fs::File::options()
            .write(true)
            .open(&header)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
          assert!(health(&mut server, id).contains("carry ERROR/MISSING nodes"));
          id += 1;
          fs::write(&header, proof).unwrap();
        }
      }
      drop(server);
      fs::remove_dir_all(base).unwrap();
    }
  }
}

#[test]
fn ambiguous_macro_receiver_heads_do_not_leak_through_watched_mcp() {
  for watch_rebuild in [false, true] {
    for crlf in [false, true] {
      let base = std::env::temp_dir().join(format!(
        "vorpal-mcp-ambiguous-head-{}-{}",
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
      let header = headers.join("proof.h");
      let proof = "#define CHECK(v, msg) { (v); }\n";
      fs::write(&header, proof).unwrap();
      let source = "#include \"proof.h\"\nstruct Probe { void method(int) {} };\nint value() { return 1; }\nvoid after() {}\nvoid run() { Probe obj;\n CHECK(value(), \"message\")\n obj.method(value());\n after();\n}\n";
      fs::write(
        src.join("main.cc"),
        if crlf {
          source.replace('\n', "\r\n")
        } else {
          source.to_owned()
        },
      )
      .unwrap();
      let env = ExtractionEnv {
        cpp_macro_include_roots: Some(vec![headers]),
        ..Default::default()
      };
      let index = src.join(".vorpal/index");
      vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
        .unwrap();
      let mut server = Server::with_profile_env_rebuild(index, Profile::Full, env, watch_rebuild);
      let mut id = 1;
      for damaged in [false, true, false] {
        let report = health(&mut server, id);
        id += 1;
        assert!(
          report.contains(if damaged {
            "carry ERROR/MISSING nodes"
          } else {
            "parse health: clean"
          }),
          "{report}"
        );
        let response = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"graph","arguments":{"relation":"callees","name":"run","format":"lean"}}}).to_string()).unwrap();
        id += 1;
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"]["isError"], false, "{response}");
        let rows = response["result"]["structuredContent"]["records"]
          .as_array()
          .unwrap();
        assert_eq!(rows.len(), if damaged { 2 } else { 3 }, "{response}");
        assert!(rows.iter().all(|r| r["name"] != "CHECK"), "{response}");
        for (name, line, site) in [
          ("value", 6, "CHECK(value(), \"message\")"),
          ("after", 8, "after();"),
        ] {
          let row = rows.iter().find(|r| r["name"] == name).unwrap();
          assert_eq!(row["site_line"], line, "{response}");
          assert_eq!(row["site"], site, "{response}");
        }
        if !damaged {
          let row = rows.iter().find(|r| r["name"] == "method").unwrap();
          assert_eq!(row["site_line"], 7, "{response}");
          assert_eq!(row["site"], "obj.method(value());", "{response}");
        }
        let modified = fs::metadata(&header).unwrap().modified().unwrap();
        let expression = "#define CHECK(v, msg)   (v)   \n";
        assert_eq!(proof.len(), expression.len());
        fs::write(&header, if damaged { proof } else { expression }).unwrap();
        fs::File::options()
          .write(true)
          .open(&header)
          .unwrap()
          .set_times(fs::FileTimes::new().set_modified(modified))
          .unwrap();
      }
      drop(server);
      fs::remove_dir_all(base).unwrap();
    }
  }
}

#[test]
fn do_while_macro_terminators_revalidate_through_normal_mcp() {
  for watch_rebuild in [false, true] {
    for crlf in [false, true] {
      let base = std::env::temp_dir().join(format!(
        "vorpal-mcp-do-terminators-{}-{}",
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
      let header = headers.join("proof.h");
      let proof = "#define CHECK(v) do { (v); } while (false) \n";
      let double_terminated = "#define CHECK(v) do { (v); } while (false);\n";
      assert_eq!(proof.len(), double_terminated.len());
      fs::write(&header, proof).unwrap();
      let source = "#include \"proof.h\"\nint value() { return 1; }\nvoid after() {}\nvoid run(bool flag) {\n if (flag) CHECK(value()) /* end */ ; else after();\n}\n";
      fs::write(
        src.join("main.cc"),
        if crlf {
          source.replace('\n', "\r\n")
        } else {
          source.to_owned()
        },
      )
      .unwrap();
      let env = ExtractionEnv {
        cpp_macro_include_roots: Some(vec![headers]),
        ..Default::default()
      };
      let index = src.join(".vorpal/index");
      vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
        .unwrap();
      let mut server = Server::with_profile_env_rebuild(index, Profile::Full, env, watch_rebuild);
      let mut id = 1;
      for damaged in [false, true, false] {
        let report = health(&mut server, id);
        id += 1;
        assert!(
          report.contains(if damaged {
            "carry ERROR/MISSING nodes"
          } else {
            "parse health: clean"
          }),
          "{report}"
        );
        let response = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"graph","arguments":{"relation":"callees","name":"run","format":"lean"}}}).to_string()).unwrap();
        id += 1;
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"]["isError"], false, "{response}");
        let rows = response["result"]["structuredContent"]["records"]
          .as_array()
          .unwrap();
        assert_eq!(rows.len(), 2, "{response}");
        for name in ["value", "after"] {
          let row = rows.iter().find(|r| r["name"] == name).unwrap();
          assert_eq!(row["site_line"], 5);
          assert_eq!(
            row["site"],
            "if (flag) CHECK(value()) /* end */ ; else after();"
          );
        }
        let modified = fs::metadata(&header).unwrap().modified().unwrap();
        fs::write(&header, if damaged { proof } else { double_terminated }).unwrap();
        fs::File::options()
          .write(true)
          .open(&header)
          .unwrap()
          .set_times(fs::FileTimes::new().set_modified(modified))
          .unwrap();
      }
      drop(server);
      fs::remove_dir_all(base).unwrap();
    }
  }
}

#[test]
fn complete_control_macros_revalidate_external_headers_in_normal_mcp() {
  for watch_rebuild in [false, true] {
    for crlf in [false, true] {
      for replacement in [
        "while (v) { (v); }",
        "for (; v;) { (v); }",
        "switch (v) { default: (v); }",
      ] {
        let base = std::env::temp_dir().join(format!(
          "vorpal-mcp-controls-{}-{}",
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
        let header = headers.join("proof.h");
        let newline = if crlf { "\r\n" } else { "\n" };
        let proof = format!("#define CHECK(v) {replacement}{newline}");
        let prefix = replacement.split('{').next().unwrap();
        let partial = format!(
          "#define CHECK(v) {prefix}{}{newline}",
          " ".repeat(replacement.len() - prefix.len())
        );
        assert_eq!(proof.len(), partial.len());
        fs::write(&header, &proof).unwrap();
        let source = "#include \"proof.h\"\nint value() { return 1; }\nvoid after() {}\nvoid run() {\n CHECK(value())\n after();\n}\n".replace('\n', newline);
        fs::write(src.join("main.cc"), &source).unwrap();
        let env = ExtractionEnv {
          cpp_macro_include_roots: Some(vec![headers]),
          ..Default::default()
        };
        let index = src.join(".vorpal/index");
        vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
          .unwrap();
        let mut server = Server::with_profile_env_rebuild(index, Profile::Full, env, watch_rebuild);
        let mut id = 1;
        for damaged in [false, true, false] {
          let report = health(&mut server, id);
          id += 1;
          assert!(
            report.contains(if damaged {
              "carry ERROR/MISSING nodes"
            } else {
              "parse health: clean"
            }),
            "{replacement}: {report}"
          );
          if !damaged {
            let response = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"graph","arguments":{"relation":"callees","name":"run","format":"lean"}}}).to_string()).unwrap();
            id += 1;
            let response: Value = serde_json::from_str(&response).unwrap();
            let rows = response["result"]["structuredContent"]["records"]
              .as_array()
              .unwrap();
            assert_eq!(rows.len(), 2, "{response}");
            for (name, line, site) in [("value", 5, "CHECK(value())"), ("after", 6, "after();")] {
              let row = rows.iter().find(|r| r["name"] == name).unwrap();
              assert_eq!(row["site_line"], line, "{response}");
              assert_eq!(row["site"], site, "{response}");
            }
          }
          let modified = fs::metadata(&header).unwrap().modified().unwrap();
          fs::write(&header, if damaged { &proof } else { &partial }).unwrap();
          fs::File::options()
            .write(true)
            .open(&header)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        }
        drop(server);
        fs::remove_dir_all(base).unwrap();
      }
    }
  }
}

#[test]
fn normal_mcp_rebuilds_keep_generated_heads_anonymous_and_real_functions_named() {
  anonymous_body_freshness(false);
}

#[test]
fn external_index_rebuilds_keep_original_source_sites_with_explicit_source() {
  anonymous_body_freshness(true);
}

fn anonymous_body_freshness(external_index: bool) {
  for watch_rebuild in [false, true] {
    for crlf in [false, true] {
      for recovery in [false, true] {
        let base = std::env::temp_dir().join(format!(
          "vorpal-mcp-anonymous-{}-{}",
          std::process::id(),
          std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
        ));
        let src = base.join("repo");
        fs::create_dir_all(&src).unwrap();
        let path = src.join("main.cc");
        let newline = if crlf { "\r\n" } else { "\n" };
        let generated = "#define GENERATE(name) void test_##name()\nvoid sink() {}\nGENERATE(one) { sink(); }\nvoid following() { sink(); }\n".replace('\n', newline);
        let ordinary = "#define GENERATE(name) void test_##name()\n#undef GENERATE\nvoid sink() {}\nvoid GENERATE(int one) { sink(); }\nvoid following() { sink(); }\n".replace('\n', newline);
        fs::write(&path, &generated).unwrap();
        let env = ExtractionEnv {
          cpp_macro_include_roots: recovery.then(Vec::new),
          ..Default::default()
        };
        let index = if external_index {
          base.join("external-index")
        } else {
          src.join(".vorpal/index")
        };
        let build = |out: &std::path::Path| {
          vorpal_index::build_index_env(&src, out, Default::default(), Default::default(), &env)
            .unwrap()
        };
        assert_eq!(build(&index).indexed, 1);
        assert_eq!(build(&index).indexed, 0);
        let mut server = Server::with_profile_env_rebuild_source(
          index.clone(),
          Profile::Full,
          env.clone(),
          watch_rebuild,
          external_index.then(|| src.clone()),
        );
        let mut id = 1;
        for named in [false, true, false] {
          fs::write(&path, if named { &ordinary } else { &generated }).unwrap();
          assert!(health(&mut server, id).contains("parse health: clean"));
          id += 1;
          // Source watch events are asynchronous in the default live lane.
          let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
          let response: Value = loop {
            let response = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"graph","arguments":{"relation":"callers","name":"sink","format":"lean"}}}).to_string()).unwrap();
            id += 1;
            let response: Value = serde_json::from_str(&response).unwrap();
            assert_eq!(response["result"]["isError"], false, "{response}");
            let rows = response["result"]["structuredContent"]["records"]
              .as_array()
              .unwrap();
            if rows.len() == 2 && rows.iter().any(|row| row["name"] == "GENERATE") == named {
              break response;
            }
            assert!(
              std::time::Instant::now() < deadline,
              "stale callers: {response}"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
          };
          let rows = response["result"]["structuredContent"]["records"]
            .as_array()
            .unwrap();
          assert!(
            rows.iter().any(|row| row["name"] == "following"),
            "{response}"
          );
          assert!(
            !rows.iter().any(|row| row["name"] == "test_one"),
            "no generated owner is guessed"
          );
          if !named {
            assert!(rows.iter().any(|row| row["kind"] == "File"), "{response}");
          }
          let response = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"graph","arguments":{"relation":"callees","name":"following","format":"lean"}}}).to_string()).unwrap();
          id += 1;
          let response: Value = serde_json::from_str(&response).unwrap();
          let rows = response["result"]["structuredContent"]["records"]
            .as_array()
            .unwrap();
          assert_eq!(rows.len(), 1, "{response}");
          assert_eq!(rows[0]["name"], "sink");
          assert_eq!(rows[0]["site_line"], if named { 5 } else { 4 });
          assert_eq!(rows[0]["site"], "void following() { sink(); }");
        }
        drop(server);
        build(&index);
        assert_eq!(build(&index).indexed, 0);
        let scratch = base.join("scratch");
        build(&scratch);
        assert_eq!(
          fs::read(index.join("CURRENT")).unwrap(),
          fs::read(scratch.join("CURRENT")).unwrap()
        );
        fs::remove_dir_all(base).unwrap();
      }
    }
  }
}

#[test]
fn enrolled_external_index_uses_registry_source_for_macro_freshness() {
  let base = std::env::temp_dir().join(format!(
    "vorpal-enrolled-external-{}-{}",
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
    "#include <proof.h>\nvoid run() { CHECK(target()) }\nvoid target() {}\n",
  )
  .unwrap();
  let header = headers.join("proof.h");
  let valid = "#define CHECK(x) { consume(x); }\n";
  fs::write(&header, valid).unwrap();
  let index = base.join("external-index");
  let env = ExtractionEnv {
    cpp_macro_include_roots: Some(vec![headers]),
    ..Default::default()
  };
  vorpal_index::build_index_env(&src, &index, Default::default(), Default::default(), &env)
    .unwrap();
  let projects = std::collections::BTreeMap::from([(
    "fixture".to_owned(),
    vorpal_mcp::registry::ProjectEntry {
      src: src.canonicalize().unwrap(),
      index,
    },
  )]);
  let mut server = vorpal_mcp::MultiServerForTest::with_envs(
    projects,
    Profile::Full,
    std::collections::BTreeMap::from([("fixture".to_owned(), env)]),
  );
  let mut query = |id| {
    let reply = server.handle_line(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"health","arguments":{"project":"fixture"}}}).to_string()).unwrap();
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    reply["result"]["content"][0]["text"]
      .as_str()
      .unwrap()
      .to_owned()
  };
  assert!(query(1).contains("parse health: clean"));
  fs::write(&header, "#define CHECK(x) consume(x)\n").unwrap();
  assert!(query(2).contains("carry ERROR/MISSING nodes"));
  fs::write(&header, valid).unwrap();
  assert!(query(3).contains("parse health: clean"));
  drop(server);
  fs::remove_dir_all(base).unwrap();
}
