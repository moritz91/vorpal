//! Opt-in proof dependencies invalidate full-tree reuse and per-file replay.
use std::{fs, path::Path};
use vorpal_index::{CacheMode, ExtractionEnv, ParseHealthPolicy, build_index_env};

#[test]
fn external_header_changes_and_local_shadowing_rebuild_dependent_products() {
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let root = std::env::temp_dir().join(format!("vorpal-cpp-replay-{}-{nonce}", std::process::id()));
  let src = root.join("src");
  let sdk = root.join("sdk");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir(&sdk).unwrap();
  let path = src.join("run.cc");
  fs::write(
    &path,
    "#include \"proof.h\"\nvoid run() { CHECK(value()) after(); }\n",
  )
  .unwrap();
  let header = sdk.join("proof.h");
  fs::write(&header, "#define CHECK(x) { function(x); }\n#ifdef PLATFORM\nstruct First {};\n#else\nstruct Second {};\n#endif\n").unwrap();
  let env = ExtractionEnv {
    cpp_macro_include_roots: Some(vec![sdk]),
    ..Default::default()
  };
  assert!(!env.is_default());
  let out = root.join("index");
  let build = |out: &Path| {
    build_index_env(
      &src,
      out,
      CacheMode::default(),
      ParseHealthPolicy::default(),
      &env,
    )
    .unwrap()
  };
  let initial = build(&out);
  assert_eq!(initial.indexed, 1);
  assert_eq!(
    initial.error_nodes, 0,
    "unchanged conditional branches preserve recovery"
  );
  assert_eq!(
    build(&out).indexed,
    0,
    "unchanged dependencies replay products"
  );
  let extractor = env.extractor().unwrap();
  let key = path.to_str().unwrap();
  let first = extractor.extraction_identity_for_path(key).unwrap();
  fs::write(
    &header,
    "#define CHECK(x) { function(x); }\n#ifdef PLATFORM\n#undef CHECK\n#endif\n",
  )
  .unwrap();
  assert_ne!(first, extractor.extraction_identity_for_path(key).unwrap());
  let edited = build(&out);
  assert!(
    edited.error_nodes > 0,
    "a possible undef restores real parse errors"
  );
  assert!(!edited.reused && !edited.graph_reused);
  assert_eq!(
    edited.indexed, 1,
    "external header edit must reparse untouched source"
  );
  let shadow = src.join("proof.h");
  fs::write(&shadow, "#define CHECK(x) { local(x); }\n").unwrap();
  assert!(
    build(&out).indexed >= 1,
    "new earlier candidate invalidates dependency"
  );
  fs::remove_file(&shadow).unwrap();
  assert!(build(&out).indexed >= 1);
  fs::remove_file(&header).unwrap();
  assert_eq!(build(&out).indexed, 1);
  fs::write(&header, "#define CHECK(x) { function(x); }\n").unwrap();
  let no_hints = std::collections::HashSet::new();
  let live = vorpal_index::build_index_live(&src, &out, Some(&no_hints), &env).unwrap();
  assert_eq!(
    live.report.indexed, 1,
    "live build must see external dependency changes even with no source hints"
  );
  assert!(live.kg.is_some());
  live.pending.unwrap().persist().unwrap();
  let scratch = root.join("scratch");
  build(&scratch);
  assert_eq!(
    fs::read(out.join("CURRENT")).unwrap(),
    fs::read(scratch.join("CURRENT")).unwrap()
  );
  // Temporary index artifacts remain under this test's unique temp directory.
}

#[test]
fn pragma_once_header_changes_invalidate_warm_products() {
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let root =
    std::env::temp_dir().join(format!("vorpal-once-replay-{}-{nonce}", std::process::id()));
  let src = root.join("src");
  let sdk = root.join("sdk");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir(&sdk).unwrap();
  let header = sdk.join("once.h");
  fs::write(&header, "#pragma once\n#undef CHECK\n").unwrap();
  fs::write(src.join("run.cc"), "#include <once.h>\n#define CHECK(x) { effect(x); }\n#include <once.h>\nvoid run() { CHECK(value()) after(); }\n").unwrap();
  let env = ExtractionEnv {
    cpp_macro_include_roots: Some(vec![sdk]),
    ..Default::default()
  };
  let out = root.join("index");
  let build = |out: &Path| {
    build_index_env(
      &src,
      out,
      CacheMode::default(),
      ParseHealthPolicy::default(),
      &env,
    )
    .unwrap()
  };
  assert_eq!(build(&out).error_nodes, 0);
  assert_eq!(build(&out).indexed, 0);
  fs::write(&header, "#undef CHECK\n").unwrap();
  let changed = build(&out);
  assert_eq!(changed.indexed, 1);
  assert!(changed.error_nodes > 0);
  assert!(!changed.reused && !changed.graph_reused);
  fs::write(&header, "#pragma once\n#undef CHECK\n").unwrap();
  let restored = build(&out);
  assert_eq!(restored.indexed, 1);
  assert_eq!(restored.error_nodes, 0);
  let scratch = root.join("scratch");
  build(&scratch);
  assert_eq!(
    fs::read(out.join("CURRENT")).unwrap(),
    fs::read(scratch.join("CURRENT")).unwrap()
  );
}

#[test]
fn header_pragma_operator_invalidates_recovered_products() {
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let root = std::env::temp_dir().join(format!(
    "vorpal-pragma-effect-replay-{}-{nonce}",
    std::process::id()
  ));
  let src = root.join("src");
  let sdk = root.join("sdk");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir(&sdk).unwrap();
  let header = sdk.join("proof.h");
  let safe = "#define CHECK(x) { effect(x); }\n";
  fs::write(&header, safe).unwrap();
  fs::write(
    src.join("run.cc"),
    "#include <proof.h>\nvoid run() { CHECK(value()) after(); }\n",
  )
  .unwrap();
  let env = ExtractionEnv {
    cpp_macro_include_roots: Some(vec![sdk]),
    ..Default::default()
  };
  let out = root.join("index");
  let build = |out: &Path| {
    build_index_env(
      &src,
      out,
      CacheMode::default(),
      ParseHealthPolicy::default(),
      &env,
    )
    .unwrap()
  };
  assert_eq!(build(&out).error_nodes, 0);
  assert_eq!(build(&out).indexed, 0);
  fs::write(
    &header,
    format!("{safe}#define RESTORE() __pragma(pop_macro(\"CHECK\"))\n"),
  )
  .unwrap();
  let changed = build(&out);
  assert_eq!(changed.indexed, 1);
  assert!(
    changed.error_nodes > 0,
    "opaque pragma effects must disable recovery"
  );
  assert!(!changed.reused && !changed.graph_reused);
  fs::write(&header, safe).unwrap();
  let restored = build(&out);
  assert_eq!(restored.indexed, 1);
  assert_eq!(restored.error_nodes, 0);
  let scratch = root.join("scratch");
  build(&scratch);
  assert_eq!(
    fs::read(out.join("CURRENT")).unwrap(),
    fs::read(scratch.join("CURRENT")).unwrap()
  );
}
