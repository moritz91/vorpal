#[path = "../../ingest/tests/support/cpp_compiler_provider.rs"]
mod provider;
use std::fs;
use vorpal_index::{ExtractionEnv, build_index_env};

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
