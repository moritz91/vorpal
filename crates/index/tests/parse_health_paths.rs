//! Native path separators must not hide product damage behind a clean report.
use std::fs;

#[test]
fn parse_health_native_paths_preserve_damage_and_missing_products_are_not_clean() {
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let root = std::env::temp_dir().join(format!(
    "vorpal-health-paths-{}-{nonce}",
    std::process::id()
  ));
  let src = root.join("src");
  fs::create_dir_all(src.join("damaged")).unwrap();
  fs::create_dir_all(src.join("clean")).unwrap();
  fs::write(src.join("damaged/twin.cc"), "void damaged() { let = ; }\n").unwrap();
  fs::write(src.join("clean/twin.cc"), "void clean() {}\n").unwrap();
  let out = root.join("index");
  let build = vorpal_index::build_index(&src, &out).unwrap();
  assert_eq!(build.error_files, 1);
  let report = vorpal_index::parse_health_report(&out).unwrap();
  assert!(
    report.contains("1 of 2 files carry ERROR/MISSING nodes"),
    "{report}"
  );
  assert!(report.contains("damaged"), "{report}");
  assert!(
    !report.contains("clean/twin.cc") && !report.contains("clean\\twin.cc"),
    "{report}"
  );
  // Losing the pack must return unavailable, never report uninspected files as clean.
  let generation = vorpal_kg::resolve_index_dir(&out);
  let products = generation.join("products");
  let pack_dir = if products.is_dir() {
    &products
  } else {
    &generation
  };
  for entry in fs::read_dir(pack_dir).unwrap() {
    let path = entry.unwrap().path();
    if path.extension().is_some_and(|ext| ext == "pack") {
      fs::write(path, []).unwrap();
    }
  }
  assert!(vorpal_index::parse_health_report(&out).is_err());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_only_health_survives_replay_and_strict_policies() {
  use vorpal_index::{CacheMode, ParseHealthMode, ParseHealthPolicy, build_index_full};
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let root = std::env::temp_dir().join(format!(
    "vorpal-missing-health-{}-{nonce}",
    std::process::id()
  ));
  let src = root.join("src");
  fs::create_dir_all(&src).unwrap();
  fs::write(src.join("missing.cc"), "int damaged() { return 1 }\n").unwrap();
  fs::write(src.join("clean.cc"), "void clean() {}\n").unwrap();
  let out = root.join("index");
  for _ in 0..2 {
    let built = build_index_full(
      &src,
      &out,
      CacheMode::Verified,
      ParseHealthPolicy::default(),
      None,
    )
    .unwrap();
    assert_eq!(built.error_files, 1);
    assert!(built.error_nodes > 0);
    assert_eq!(built.error_bytes, 0);
    let health = vorpal_index::parse_health_report(&out).unwrap();
    assert!(
      health.contains("1 of 2 files carry ERROR/MISSING nodes"),
      "{health}"
    );
    assert!(health.contains("0 damaged bytes"), "{health}");
    assert!(
      health.contains("entities in damaged regions: damaged"),
      "{health}"
    );
  }
  let strict = ParseHealthPolicy {
    mode: ParseHealthMode::Fail,
    max_error_ratio: 0.0,
  };
  let error = build_index_full(&src, &out, CacheMode::Verified, strict, None).unwrap_err();
  assert!(error.to_string().contains("missing.cc"), "{error}");
  let exclude = ParseHealthPolicy {
    mode: ParseHealthMode::Exclude,
    max_error_ratio: 0.0,
  };
  let excluded = build_index_full(&src, &out, CacheMode::Verified, exclude, None).unwrap();
  assert_eq!(excluded.excluded_files, 1);
  // Positive thresholds remain byte ratios, without inventing an error-byte width.
  let lenient = ParseHealthPolicy {
    mode: ParseHealthMode::Fail,
    max_error_ratio: 0.1,
  };
  build_index_full(&src, &out, CacheMode::Verified, lenient, None).unwrap();
  fs::remove_dir_all(root).unwrap();
}


#[test]
fn objc_guard_edits_revalidate_default_cpp_trees_and_products() {
  let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
  let root = std::env::temp_dir().join(format!("vorpal-objc-guard-health-{}-{nonce}", std::process::id()));
  let src = root.join("src"); let out = root.join("index");
  fs::create_dir_all(&src).unwrap();
  let file = src.join("guarded.cc");
  let positive = "#ifdef __OBJC__
@protocol ProbeProtocol
-(void) action;
@end
void run(Probe* object) { [object release]; consume(@selector(action)); selector(real()); }
void guarded() { @try { before(); } @catch (Probe* error) { forward([error description]); } }
namespace Sample { template<class T> struct Wrapper { id stored = @\"text\"; Wrapper() : stored([object copy]) { init(); } int count() { id local = [object format:@\"value\", payload()]; return [object range].location + value(); } }; }
#endif
void following() { after(); }
";
  let negative = positive.replace("__OBJC__", "PLATFORM");
  fs::write(&file, positive).unwrap();
  // No macro opt-in: exercise the default C++ incremental tree/walk lanes.
  let clean = vorpal_index::build_index(&src, &out).unwrap();
  assert_eq!(clean.error_nodes, 0);
  assert_eq!(vorpal_index::build_index(&src, &out).unwrap().indexed, 0);
  for _ in 0..2 {
    fs::write(&file, &negative).unwrap();
    let changed = vorpal_index::build_index(&src, &out).unwrap();
    assert_eq!(changed.indexed, 1); assert!(changed.error_nodes > 0);
    let warm = vorpal_index::build_index(&src, &out).unwrap();
    assert_eq!(warm.indexed, 0);
    // Whole-tree reuse reports no newly processed parse counters; verify stored telemetry.
    let health = vorpal_index::parse_health_report(&out).unwrap();
    assert!(health.contains("1 of 1 files carry ERROR/MISSING nodes"), "{health}");
    fs::write(&file, positive).unwrap();
    let restored = vorpal_index::build_index(&src, &out).unwrap();
    assert_eq!(restored.indexed, 1); assert_eq!(restored.error_nodes, 0);
    fs::write(&file, positive.replace("(Probe* error)", "()")).unwrap();
    assert!(vorpal_index::build_index(&src, &out).unwrap().error_nodes > 0);
    assert!(vorpal_index::parse_health_report(&out).unwrap().contains("1 of 1 files carry ERROR/MISSING nodes"));
    fs::write(&file, positive).unwrap();
    assert_eq!(vorpal_index::build_index(&src, &out).unwrap().error_nodes, 0);
    fs::write(&file, positive.replace("@protocol ProbeProtocol", "@protocolProbeProtocol")).unwrap();
    assert!(vorpal_index::build_index(&src, &out).unwrap().error_nodes > 0);
    fs::write(&file, positive).unwrap();
    assert_eq!(vorpal_index::build_index(&src, &out).unwrap().error_nodes, 0);
    fs::write(&file, positive.replace("@selector(action)", "@selectorSuffix(action)")).unwrap();
    assert!(vorpal_index::build_index(&src, &out).unwrap().error_nodes > 0);
    fs::write(&file, positive).unwrap();
    assert_eq!(vorpal_index::build_index(&src, &out).unwrap().error_nodes, 0);
    fs::write(&file, positive.replace("[object range].location", "[object range].")).unwrap();
    assert!(vorpal_index::build_index(&src, &out).unwrap().error_nodes > 0);
    fs::write(&file, positive).unwrap();
    assert_eq!(vorpal_index::build_index(&src, &out).unwrap().error_nodes, 0);
    fs::write(&file, positive.replace("stored([object copy])", "stored([object copy]")).unwrap();
    assert!(vorpal_index::build_index(&src, &out).unwrap().error_nodes > 0);
    fs::write(&file, positive).unwrap();
    assert_eq!(vorpal_index::build_index(&src, &out).unwrap().error_nodes, 0);
    fs::write(&file, positive.replace("namespace Sample {", "namespace Sample { [object release];")).unwrap();
    assert!(vorpal_index::build_index(&src, &out).unwrap().error_nodes > 0);
    fs::write(&file, positive).unwrap();
    assert_eq!(vorpal_index::build_index(&src, &out).unwrap().error_nodes, 0);
    let inverse = "#ifndef __OBJC__\nvoid ordinary() { before(); }\n#else\nvoid guarded() { [object release]; }\n#endif\nvoid following() { after(); }\n";
    fs::write(&file, inverse).unwrap();
    assert_eq!(vorpal_index::build_index(&src, &out).unwrap().error_nodes, 0);
    fs::write(&file, inverse.replace("__OBJC__", "PLATFORM")).unwrap();
    assert!(vorpal_index::build_index(&src, &out).unwrap().error_nodes > 0);
    fs::write(&file, inverse).unwrap();
    assert_eq!(vorpal_index::build_index(&src, &out).unwrap().error_nodes, 0);
    fs::write(&file, positive).unwrap();
    assert_eq!(vorpal_index::build_index(&src, &out).unwrap().error_nodes, 0);
    let scratch = root.join("scratch"); vorpal_index::build_index(&src, &scratch).unwrap();
    assert_eq!(fs::read(out.join("CURRENT")).unwrap(), fs::read(scratch.join("CURRENT")).unwrap());
  }
  fs::remove_dir_all(root).unwrap();
}
