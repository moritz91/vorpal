#[path = "../../ingest/tests/support/cpp_compiler_provider.rs"]
mod provider;
use std::fs;
use vorpal_index::{ExtractionEnv, build_index_env};

#[test]
fn compiler_outages_preserve_committed_artifacts_and_never_return_a_live_graph() {
  let base = std::env::temp_dir().join(format!(
    "vorpal-index-compiler-outage-{}",
    std::process::id()
  ));
  let src = base.join("src");
  let sdk = base.join("sdk");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir_all(&sdk).unwrap();
  let path = src.join("main.cc");
  let source = "#include <unknown-sdk.h>\nvoid run() { CHECK(value())\nafter(); }\nvoid value() {}\nvoid after() {}\n";
  fs::write(&path, source).unwrap();
  // Other workers may have already built products when this capture fails.
  fs::write(src.join("ordinary.cc"), "void independent() {}\n").unwrap();
  fs::write(sdk.join("proof.h"), "#define CHECK(x) { sink(x); }\n").unwrap();
  let mut command = provider::command(&sdk, &path);
  command.timeout_seconds = 1;
  let env = ExtractionEnv {
    cpp_macro_compiler: Some(command),
    ..Default::default()
  };
  let out = base.join("index");
  let build = |out: &std::path::Path, env: &ExtractionEnv| {
    build_index_env(&src, out, Default::default(), Default::default(), env)
  };
  build(&out, &env).unwrap();
  let initial = fs::read(out.join("CURRENT")).unwrap();
  let generation = vorpal_kg::resolve_index_dir(&out);
  let original = provider::artifact_snapshot(&generation);
  for mode in [
    "unavailable",
    "exit",
    "timeout",
    "json",
    "missing-response",
    "missing-provider",
  ] {
    fs::write(sdk.join("mode"), mode).unwrap();
    let mut failed_env = env.clone();
    if mode == "missing-provider" {
      failed_env.cpp_macro_compiler.as_mut().unwrap().program = sdk.join("private-secret-absent");
    }
    for live in [false, true] {
      let error = if live {
        // Empty hints must not enable a no-change bypass or a partial live KG.
        vorpal_index::build_index_live(&src, &out, Some(&Default::default()), &failed_env)
          .err()
          .unwrap()
          .to_string()
      } else {
        build(&out, &failed_env).unwrap_err().to_string()
      };
      assert!(
        error.contains("native C++ compiler recovery failed"),
        "{mode}/{live}: {error}"
      );
      assert!(!error.contains("private-secret"));
      assert!(!error.contains("private-provider-stderr-secret"));
      assert_eq!(fs::read(out.join("CURRENT")).unwrap(), initial);
      assert_eq!(provider::artifact_snapshot(&generation), original);
    }
    let empty = base.join(format!("empty-{mode}"));
    assert!(build(&empty, &failed_env).is_err());
    assert!(!empty.join("CURRENT").exists());
    fs::write(sdk.join("mode"), "").unwrap();
    let live = vorpal_index::build_index_live(&src, &out, None, &env).unwrap();
    assert!(live.kg.is_some());
    live.pending.unwrap().persist().unwrap();
    assert_eq!(fs::read(out.join("CURRENT")).unwrap(), initial);
    assert_eq!(provider::artifact_snapshot(&generation), original);
  }
  // A completed, explicitly declined proof still publishes raw diagnostics.
  fs::write(sdk.join("mode"), "decline").unwrap();
  assert!(build(&out, &env).unwrap().error_nodes > 0);
  fs::write(sdk.join("mode"), "").unwrap();
  build(&out, &env).unwrap();
  assert_eq!(fs::read(out.join("CURRENT")).unwrap(), initial);
  assert_eq!(fs::read_to_string(path).unwrap(), source);
  fs::remove_dir_all(base).unwrap();
}

#[test]
fn compiler_captures_bypass_warm_products_and_match_a_scratch_generation() {
  let base = std::env::temp_dir().join(format!(
    "vorpal-index-compiler-{}-{}",
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
  let path = src.join("main.cc");
  let source = "#include <unknown-sdk.h>\nvoid run() { CHECK(value())\nafter(); }\nvoid value() {}\nvoid after() {}\n";
  fs::write(&path, source).unwrap();
  fs::write(sdk.join("proof.h"), "#define CHECK(x) { sink(x); }\n").unwrap();
  let env = ExtractionEnv {
    cpp_macro_compiler: Some(provider::command(&sdk, &path)),
    ..Default::default()
  };
  let out = base.join("index");
  let build = |out: &std::path::Path| {
    build_index_env(&src, out, Default::default(), Default::default(), &env).unwrap()
  };
  let first = build(&out);
  assert_eq!(first.indexed, 1);
  let initial = fs::read(out.join("CURRENT")).unwrap();
  let warm = build(&out);
  assert_eq!(warm.indexed, 1);
  assert_eq!(warm.skipped, 0);
  assert!(!warm.reused);
  assert_eq!(fs::read(out.join("CURRENT")).unwrap(), initial);
  fs::write(sdk.join("native-only.h"), "changed").unwrap();
  let changed = build(&out);
  assert_eq!(changed.indexed, 1);
  assert_eq!(changed.skipped, 0);
  assert!(
    vorpal_index::parse_health_report(&out)
      .unwrap()
      .contains("carry ERROR/MISSING nodes")
  );
  let scratch = base.join("scratch");
  build(&scratch);
  assert_eq!(
    fs::read(out.join("CURRENT")).unwrap(),
    fs::read(scratch.join("CURRENT")).unwrap()
  );
  fs::remove_file(sdk.join("native-only.h")).unwrap();
  build(&out);
  assert!(
    vorpal_index::parse_health_report(&out)
      .unwrap()
      .contains("parse health: clean")
  );
  assert_eq!(fs::read(out.join("CURRENT")).unwrap(), initial);
  assert_eq!(fs::read_to_string(path).unwrap(), source);
  fs::remove_dir_all(base).unwrap();
}
