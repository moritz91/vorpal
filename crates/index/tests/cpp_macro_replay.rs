//! Opt-in proof dependencies invalidate full-tree reuse and per-file replay.
use std::{fs, path::Path};
use vorpal_index::{CacheMode, ExtractionEnv, ParseHealthPolicy, build_index_env};

#[test]
fn external_header_changes_and_local_shadowing_rebuild_dependent_products() {
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let root = physical_temp_dir().join(format!("vorpal-cpp-replay-{}-{nonce}", std::process::id()));
  let src = root.join("src");
  let sdk = root.join("sdk");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir(&sdk).unwrap();
  let path = src.join("run.cc");
  fs::write(
    &path,
    "#include \"proof.h\"\nvoid run() { CHECK /* invocation */ \x0b(value()) after(); }\n",
  )
  .unwrap();
  let header = sdk.join("proof.h");
  fs::write(&header, "#define CHECK(x) { function(x); }\n#if defined(PLATFORM)\nstruct First {};\n#else\nstruct Second {};\n#endif\n").unwrap();
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
    "#define CHECK(x) { function(x); }\n#if defined(PLATFORM)\n#undef CHECK\n#endif\n",
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
  let root = physical_temp_dir().join(format!("vorpal-once-replay-{}-{nonce}", std::process::id()));
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
  let root = physical_temp_dir().join(format!(
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
    format!("{safe}#define RESTORE() __pragma(pop_macro(\"CHECK\"))\nRESTORE();\n"),
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

#[cfg(any(unix, windows))]
#[test]
fn directory_redirect_creation_and_removal_invalidates_macro_products() {
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let root = physical_temp_dir().join(format!(
    "vorpal-directory-replay-{}-{nonce}",
    std::process::id()
  ));
  let src = root.join("src");
  let sdk = root.join("sdk");
  let real = root.join("real");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir(&sdk).unwrap();
  fs::create_dir(&real).unwrap();
  let safe = "#define CHECK(x) { effect(x); }\n";
  fs::write(sdk.join("proof.h"), safe).unwrap();
  fs::write(real.join("proof.h"), safe).unwrap();
  fs::write(
    src.join("run.cc"),
    "#include <proof.h>\nvoid run() { CHECK(value()) after(); }\n",
  )
  .unwrap();
  let env = ExtractionEnv {
    cpp_macro_include_roots: Some(vec![sdk.clone()]),
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
  fs::remove_file(sdk.join("proof.h")).unwrap();
  fs::remove_dir(&sdk).unwrap();
  #[cfg(unix)]
  std::os::unix::fs::symlink(&real, &sdk).unwrap();
  #[cfg(windows)]
  {
    let result = std::process::Command::new("cmd")
      .args(["/d", "/c", "mklink", "/J"])
      .arg(&sdk)
      .arg(&real)
      .output()
      .unwrap();
    assert!(
      result.status.success(),
      "{}",
      String::from_utf8_lossy(&result.stderr)
    );
  }
  // Both full and live entry points must reject source aliases before their
  // shared canonicalizer can silently choose different quoted headers.
  let alias_env = ExtractionEnv {
    cpp_macro_include_roots: Some(vec![]),
    ..Default::default()
  };
  let rejected = build_index_env(
    &sdk,
    &root.join("aliased-index"),
    CacheMode::default(),
    ParseHealthPolicy::default(),
    &alias_env,
  );
  assert!(
    rejected
      .unwrap_err()
      .to_string()
      .contains("unredirected source root")
  );
  let rejected = vorpal_index::build_index_live(&sdk, &root.join("aliased-live"), None, &alias_env);
  match rejected {
    Err(error) => assert!(error.to_string().contains("unredirected source root")),
    Ok(_) => panic!("live builds must reject redirected source roots"),
  }
  assert!(
    build_index_env(
      &sdk,
      &root.join("aliased-default"),
      CacheMode::default(),
      ParseHealthPolicy::default(),
      &ExtractionEnv::default()
    )
    .is_ok(),
    "default indexing keeps its existing canonical-root behavior"
  );
  let redirected = build(&out);
  assert_eq!(
    redirected.indexed, 1,
    "an unchanged header behind a redirect must not replay its old proof"
  );
  assert!(redirected.error_nodes > 0);
  assert!(!redirected.reused && !redirected.graph_reused);
  #[cfg(windows)]
  fs::remove_dir(&sdk).unwrap();
  #[cfg(unix)]
  fs::remove_file(&sdk).unwrap();
  fs::create_dir(&sdk).unwrap();
  fs::write(sdk.join("proof.h"), safe).unwrap();
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

// Some platforms spell their temp directory through a system symlink. Ordinary
// fixtures use the physical path; alias tests create their own explicit redirects.
fn physical_temp_dir() -> std::path::PathBuf {
  let path = std::env::temp_dir();
  #[cfg(unix)]
  {
    path.canonicalize().unwrap_or(path)
  }
  #[cfg(not(unix))]
  {
    path
  }
}

#[test]
fn argument_macro_header_edits_invalidate_recovered_products() {
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let root = physical_temp_dir().join(format!(
    "vorpal-argument-replay-{}-{nonce}",
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
    "#include <proof.h>\nvoid run() { CHECK(END) after(); }\n",
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
  fs::write(&header, format!("{safe}#define END }}\n")).unwrap();
  let changed = build(&out);
  assert_eq!(changed.indexed, 1);
  assert!(
    changed.error_nodes > 0,
    "an expanding argument must not replay recovered products"
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

#[test]
fn legacy_invocation_proofs_cannot_replay_a_false_clean_product() {
  use vorpal_ingest::{Manifest, cache_file_name, save_product};
  for (version, source) in [
    (1, "void before() { CHECK\x0b(value()) }\n#define CHECK(x) { effect(x); }\nvoid run() { CHECK(value()) }\n"),
    (2, "#define DECLARE(name) { int name; }\nvoid run() { DECLARE(1 + 2) }\n"),
    (3, "#define DECLARE(\\u03B1) { int α; }\nvoid run() { DECLARE(1 + 2) }\n"),
    (4, "#define NESTED(x) { void local() { sink(x); } }\nvoid run() { NESTED(1) }\n"),
    (5, "#define CHECK(x) { sink(x); }\n#undef 123invalid\nvoid run() { CHECK(value()) }\n"),
    (7, "#define CHECK(x) { sink(x); }\nint run() { return CHECK(value()); }\n"),
    (9, "#define DECLARE(x) { int x; }\nvoid run() { DECLARE(1 + 2); }\n"),
    (10, "#define CHECK(x) { sink(x); }\nnamespace scope { CHECK(value()) }\n"),
  ] {
    let nonce = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = physical_temp_dir().join(format!("vorpal-proof-migration-v{version}-{}-{nonce}", std::process::id()));
    let src = root.join("src");
    let out = root.join("index");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(out.join("products")).unwrap();
    fs::write(src.join("run.cc"), source).unwrap();
    let env = ExtractionEnv { cpp_macro_include_roots: Some(vec![]), ..Default::default() };
    let extractor = env.extractor().unwrap();
    let manifest = Manifest::scan(&src, |_| true).unwrap();
    let stat = &manifest.entries()[0];
    let mut legacy = extractor.extract_product(&stat.path, source).unwrap();
    assert!(legacy.error_nodes > 0, "the invocation lacks a valid replacement proof");
    // Frozen old identities model whitespace and replacement-context proof bugs.
    // Source, config and dependencies are unchanged.
    let base = vorpal_ingest::extraction_identity_for_path(&stat.path, extractor.rules_digest()).unwrap();
    let evidence = vorpal_ingest::cpp_macro_evidence::audit_with_roots(Path::new(&stat.path), source, &[]);
    let mut hash = xxhash_rust::xxh3::Xxh3::new();
    hash.update(format!("vorpal-cpp-macro-product-v{version}\0").as_bytes());
    hash.update(&base.to_le_bytes());
    hash.update(&evidence.dependency_identity().to_le_bytes());
    legacy.grammar_digest = hash.digest();
    legacy.source_mtime_ns = stat.mtime_ns;
    legacy.source_size = stat.size;
    legacy.source_xxh3 = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
    legacy.error_nodes = 0;
    legacy.error_bytes = 0;
    legacy.error_spans.clear();
    if version == 7 {
      // The old expression slot also invented a runtime callee for the macro.
      let mut fake = legacy.refs.iter().find(|r| r.name == "value").unwrap().clone();
      fake.name = "CHECK".to_owned();
      fake.start = source.rfind("CHECK(").unwrap() as u32;
      fake.end = fake.start + "CHECK(value())".len() as u32;
      fake.call_shape = 5; // one argument, plain callee, no tree error
      legacy.refs.push(fake);
    }
    save_product(&out.join("products").join(cache_file_name(&stat.path)), &legacy).unwrap();
    let build = |out: &Path| build_index_env(&src, out, CacheMode::default(), ParseHealthPolicy::default(), &env).unwrap();
    let migrated = build(&out);
    assert_eq!(migrated.indexed, 1, "old invocation proof must be reparsed");
    assert!(migrated.error_nodes > 0, "old telemetry must not conceal the missing semicolon");
    let warm = build(&out);
    assert_eq!(warm.indexed, 0, "new packed products replay normally");
    assert_eq!(warm.error_nodes, migrated.error_nodes);
    let scratch = root.join("scratch");
    build(&scratch);
    assert_eq!(fs::read(out.join("CURRENT")).unwrap(), fs::read(scratch.join("CURRENT")).unwrap());
  }
}

#[test]
fn macro_stack_target_edits_invalidate_external_header_replay() {
  let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
  let root = physical_temp_dir().join(format!("vorpal-stack-replay-{}-{nonce}", std::process::id()));
  let src = root.join("src");
  let sdk = root.join("sdk");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir(&sdk).unwrap();
  fs::write(src.join("run.cc"), "#include \"proof.h\"\nvoid run() { CHECK(value()) after(); }\n").unwrap();
  let header = sdk.join("proof.h");
  let safe = "#pragma push_macro(\"OTHER\")\n#pragma pop_macro(\"OTHER\")\n#define CHECK(x) { sink(x); }\n";
  fs::write(&header, safe).unwrap();
  let env = ExtractionEnv { cpp_macro_include_roots: Some(vec![sdk]), ..Default::default() };
  let out = root.join("index");
  let build = |out: &Path| build_index_env(&src, out, CacheMode::default(), ParseHealthPolicy::default(), &env).unwrap();
  let first = build(&out);
  assert_eq!(first.indexed, 1);
  assert_eq!(first.error_nodes, 0);
  assert_eq!(build(&out).indexed, 0);
  fs::write(&header, safe.replace("OTHER", "CHECK")).unwrap();
  let changed = build(&out);
  assert_eq!(changed.indexed, 1);
  assert!(changed.error_nodes > 0, "the target's definition must remain unknown after stack metadata");
  assert!(!changed.reused && !changed.graph_reused);
  fs::write(&header, safe).unwrap();
  let restored = build(&out);
  assert_eq!(restored.indexed, 1);
  assert_eq!(restored.error_nodes, 0);
  let scratch = root.join("scratch");
  build(&scratch);
  assert_eq!(fs::read(out.join("CURRENT")).unwrap(), fs::read(scratch.join("CURRENT")).unwrap());
}


#[test]
fn unused_pragma_header_proofs_migrate_and_invocations_invalidate_replay() {
  use vorpal_ingest::{Manifest, cache_file_name, save_product};
  let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
  let root = physical_temp_dir().join(format!("vorpal-unused-pragma-replay-{}-{nonce}", std::process::id()));
  let src = root.join("src");
  let sdk = root.join("sdk");
  let out = root.join("index");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir(&sdk).unwrap();
  fs::create_dir_all(out.join("products")).unwrap();
  let source = "#include \"proof.h\"\nvoid run() { CHECK(value()) after(); }\n";
  fs::write(src.join("run.cc"), source).unwrap();
  let header = sdk.join("proof.h");
  let safe = "#define DIRECT() __pragma(pop_macro(\"CHECK\"))\n#define WRAPPER() DIRECT()\n#define CHECK(x) { sink(x); }\n";
  fs::write(&header, safe).unwrap();
  let env = ExtractionEnv { cpp_macro_include_roots: Some(vec![sdk]), ..Default::default() };
  let extractor = env.extractor().unwrap();
  let manifest = Manifest::scan(&src, |_| true).unwrap();
  let stat = &manifest.entries()[0];
  let mut legacy = extractor.extract_product(&stat.path, source).unwrap();
  assert_eq!(legacy.error_nodes, 0);
  let base = vorpal_ingest::extraction_identity_for_path(&stat.path, extractor.rules_digest()).unwrap();
  let evidence = vorpal_ingest::cpp_macro_evidence::audit_with_roots(Path::new(&stat.path), source, env.cpp_macro_include_roots.as_ref().unwrap());
  let mut hash = xxhash_rust::xxh3::Xxh3::new();
  hash.update(b"vorpal-cpp-macro-product-v6\0");
  hash.update(&base.to_le_bytes());
  hash.update(&evidence.dependency_identity().to_le_bytes());
  legacy.grammar_digest = hash.digest();
  legacy.source_mtime_ns = stat.mtime_ns;
  legacy.source_size = stat.size;
  legacy.source_xxh3 = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
  // Prior policy declined all recovery merely on seeing an unused operator.
  legacy.error_nodes = 1;
  legacy.error_bytes = 1;
  save_product(&out.join("products").join(cache_file_name(&stat.path)), &legacy).unwrap();
  let build = |out: &Path| build_index_env(&src, out, CacheMode::default(), ParseHealthPolicy::default(), &env).unwrap();
  let migrated = build(&out);
  assert_eq!(migrated.indexed, 1, "old conservative telemetry must be reparsed");
  assert_eq!(migrated.error_nodes, 0);
  assert_eq!(build(&out).indexed, 0);
  fs::write(&header, format!("{safe}WRAPPER();\n")).unwrap();
  let changed = build(&out);
  assert_eq!(changed.indexed, 1);
  assert!(changed.error_nodes > 0, "invoked pragma wrappers must still disable proof");
  assert!(!changed.reused && !changed.graph_reused);
  fs::write(&header, safe).unwrap();
  let restored = build(&out);
  assert_eq!(restored.indexed, 1);
  assert_eq!(restored.error_nodes, 0);
  let scratch = root.join("scratch");
  build(&scratch);
  assert_eq!(fs::read(out.join("CURRENT")).unwrap(), fs::read(scratch.join("CURRENT")).unwrap());
}


#[test]
fn proof_context_errors_survive_replay_and_strict_health_policies() {
  use vorpal_index::ParseHealthMode;
  let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
  let root = physical_temp_dir().join(format!("vorpal-context-policy-{}-{nonce}", std::process::id()));
  let src = root.join("src");
  fs::create_dir_all(&src).unwrap();
  fs::write(src.join("run.cc"), "#define CHECK(x) { sink(x); }\nint run() { return CHECK(value()); }\n").unwrap();
  let env = ExtractionEnv { cpp_macro_include_roots: Some(vec![]), ..Default::default() };
  let out = root.join("index");
  let build = |policy| build_index_env(&src, &out, CacheMode::Verified, policy, &env);
  let first = build(ParseHealthPolicy::default()).unwrap();
  assert_eq!(first.error_nodes, 1, "one original-span context diagnostic, despite a clean unexpanded tree");
  assert_eq!(first.error_files, 1);
  let warm = build(ParseHealthPolicy::default()).unwrap();
  assert_eq!(warm.indexed, 0);
  assert_eq!(warm.error_nodes, 1);
  assert!(vorpal_index::parse_health_report(&out).unwrap().contains("macro-context diagnostics"));
  let strict = ParseHealthPolicy { mode: ParseHealthMode::Fail, max_error_ratio: 0.0 };
  assert!(build(strict).unwrap_err().to_string().contains("run.cc"));
  let exclude = ParseHealthPolicy { mode: ParseHealthMode::Exclude, max_error_ratio: 0.0 };
  assert_eq!(build(exclude).unwrap().excluded_files, 1);
}

#[test]
fn context_policy_migrations_remove_false_diagnostics() {
  use vorpal_ingest::{Manifest, cache_file_name, save_product};
  for (version, source) in [
    (8, "#define CHECK(x) { sink(x); }\n#define IDENTITY(x) x\nvoid run() { IDENTITY(CHECK(value())); }\n"),
    (9, "#define CHECK(x) { sink(x); }\nvoid run() { CHECK() }\n"),
  ] {
  let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
  let root = physical_temp_dir().join(format!("vorpal-context-v8-{}-{nonce}", std::process::id()));
  let src = root.join("src");
  let out = root.join("index");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir_all(out.join("products")).unwrap();
  fs::write(src.join("run.cc"), source).unwrap();
  let env = ExtractionEnv { cpp_macro_include_roots: Some(vec![]), ..Default::default() };
  let extractor = env.extractor().unwrap();
  let manifest = Manifest::scan(&src, |_| true).unwrap();
  let stat = &manifest.entries()[0];
  let mut legacy = extractor.extract_product(&stat.path, source).unwrap();
  assert_eq!(legacy.error_nodes, 0);
  let base = vorpal_ingest::extraction_identity_for_path(&stat.path, extractor.rules_digest()).unwrap();
  let evidence = vorpal_ingest::cpp_macro_evidence::audit_with_roots(Path::new(&stat.path), source, &[]);
  let mut hash = xxhash_rust::xxh3::Xxh3::new();
  hash.update(format!("vorpal-cpp-macro-product-v{version}\0").as_bytes());
  hash.update(&base.to_le_bytes());
  hash.update(&evidence.dependency_identity().to_le_bytes());
  legacy.grammar_digest = hash.digest();
  legacy.source_mtime_ns = stat.mtime_ns;
  legacy.source_size = stat.size;
  legacy.source_xxh3 = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
  legacy.error_nodes = 1;
  legacy.error_bytes = "CHECK(value())".len() as u64;
  save_product(&out.join("products").join(cache_file_name(&stat.path)), &legacy).unwrap();
  let build = |out: &Path| build_index_env(&src, out, CacheMode::default(), ParseHealthPolicy::default(), &env).unwrap();
  let migrated = build(&out);
  assert_eq!(migrated.indexed, 1);
  assert_eq!(migrated.error_nodes, 0, "valid enclosing expansion must lose stale context errors");
  assert_eq!(build(&out).indexed, 0);
  let scratch = root.join("scratch");
  build(&scratch);
  assert_eq!(fs::read(out.join("CURRENT")).unwrap(), fs::read(scratch.join("CURRENT")).unwrap());
  }
}

#[test]
fn intact_guard_metadata_migrates_and_header_effects_invalidate_replay() {
  use vorpal_ingest::{Manifest, cache_file_name, save_product};
  let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
  let root = physical_temp_dir().join(format!("vorpal-guard-metadata-{}-{nonce}", std::process::id()));
  let src = root.join("src"); let sdk = root.join("sdk"); let out = root.join("index");
  fs::create_dir_all(&src).unwrap(); fs::create_dir(&sdk).unwrap(); fs::create_dir_all(out.join("products")).unwrap();
  let source = "#define CHECK(x) { sink(x); }\n#include <proof.h>\nvoid run() { CHECK(value()) }\n";
  fs::write(src.join("run.cc"), source).unwrap();
  let header = sdk.join("proof.h");
  let safe = "#define BEGIN namespace sdk {\n#define END }\n#if defined(ENABLE)\nBEGIN\nextern const int variable;\nEND\n#endif\n";
  fs::write(&header, safe).unwrap();
  let env = ExtractionEnv { cpp_macro_include_roots: Some(vec![sdk]), ..Default::default() };
  let extractor = env.extractor().unwrap();
  let manifest = Manifest::scan(&src, |_| true).unwrap(); let stat = &manifest.entries()[0];
  let mut legacy = extractor.extract_product(&stat.path, source).unwrap(); assert_eq!(legacy.error_nodes, 0);
  let base = vorpal_ingest::extraction_identity_for_path(&stat.path, extractor.rules_digest()).unwrap();
  let evidence = vorpal_ingest::cpp_macro_evidence::audit_with_roots(Path::new(&stat.path), source, env.cpp_macro_include_roots.as_ref().unwrap());
  let mut dependency = xxhash_rust::xxh3::Xxh3::new(); dependency.update(b"vorpal-cpp-macro-evidence-v12\0");
  dependency.update(&(evidence.include_roots.len() as u64).to_le_bytes());
  for path in &evidence.include_roots { let bytes = path.as_os_str().as_encoded_bytes(); dependency.update(&(bytes.len() as u64).to_le_bytes()); dependency.update(bytes); }
  dependency.update(&(evidence.dependencies.len() as u64).to_le_bytes());
  for item in &evidence.dependencies { let bytes = item.path.as_os_str().as_encoded_bytes(); dependency.update(&(bytes.len() as u64).to_le_bytes()); dependency.update(bytes); dependency.update(&[u8::from(item.digest.is_some())]); dependency.update(&item.digest.unwrap_or_default().to_le_bytes()); }
  let mut hash = xxhash_rust::xxh3::Xxh3::new(); hash.update(b"vorpal-cpp-macro-product-v11\0"); hash.update(&base.to_le_bytes()); hash.update(&dependency.digest().to_le_bytes());
  legacy.grammar_digest = hash.digest(); legacy.source_mtime_ns = stat.mtime_ns; legacy.source_size = stat.size; legacy.source_xxh3 = xxhash_rust::xxh3::xxh3_64(source.as_bytes()); legacy.error_nodes = 1; legacy.error_bytes = 0;
  save_product(&out.join("products").join(cache_file_name(&stat.path)), &legacy).unwrap();
  let build = |out: &Path| build_index_env(&src, out, CacheMode::default(), ParseHealthPolicy::default(), &env).unwrap();
  let migrated = build(&out); assert_eq!(migrated.indexed, 1); assert_eq!(migrated.error_nodes, 0); assert_eq!(build(&out).indexed, 0);
  for unsafe_header in [safe.replace("defined(ENABLE)", "EXPANDING"), safe.replace("END\n#endif", "#undef CHECK\nEND\n#endif")] {
    fs::write(&header, unsafe_header).unwrap(); let changed = build(&out); assert_eq!(changed.indexed, 1); assert!(changed.error_nodes > 0); assert!(!changed.reused && !changed.graph_reused);
    fs::write(&header, safe).unwrap(); let restored = build(&out); assert_eq!(restored.indexed, 1); assert_eq!(restored.error_nodes, 0);
  }
  let scratch = root.join("scratch"); build(&scratch); assert_eq!(fs::read(out.join("CURRENT")).unwrap(), fs::read(scratch.join("CURRENT")).unwrap());
}
