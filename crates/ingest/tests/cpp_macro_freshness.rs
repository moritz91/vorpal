//! Freshness observations must describe one consistent filesystem epoch.
use std::{fs, sync::Arc};
use vorpal_core::{Language, tree_sitter::LanguageExt};
use vorpal_ingest::{ExtractionEnv, cpp_macro_freshness::MacroFreshness};

#[test]
fn conflicting_observations_require_a_new_epoch_and_canaries_publish_no_inputs() {
  let base = std::env::temp_dir().join(format!(
    "vorpal-macro-observations-{}-{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_nanos()
  ));
  fs::create_dir_all(&base).unwrap();
  let path = base.join("calls.cc");
  let source = "#include \"proof.h\"\nvoid run() { CHECK(target()) }\n";
  fs::write(&path, source).unwrap();
  let header = base.join("proof.h");
  fs::write(&header, "#define CHECK(x) { consume(x); }\n").unwrap();
  let freshness = Arc::new(MacroFreshness::default());
  let env = ExtractionEnv {
    cpp_macro_include_roots: Some(vec![]),
    cpp_macro_freshness: Some(freshness.clone()),
    ..Default::default()
  };
  let extractor = env.extractor().unwrap();
  freshness.begin_refresh();
  vorpal_ingest::verify_env_extraction(
    &extractor,
    &[vorpal_ingest::DynamicCanary {
      lang: "cpp".to_owned(),
      path: base.join("virtual.cpp").to_string_lossy().into_owned(),
      source: "void virtual_canary() {}".to_owned(),
      min_items: 1,
      min_refs: 0,
    }],
  )
  .unwrap();
  freshness.finish_refresh();
  assert!(
    !freshness.has_changed(),
    "manufactured absolute canaries are not filesystem inputs"
  );
  let path = path.to_str().unwrap();
  extractor.extract_product(path, source).unwrap();
  assert!(!freshness.has_changed());
  fs::write(&header, "#define CHECK(x) ((void)(x))\n").unwrap();
  extractor.extract_product(path, source).unwrap();
  freshness.finish_refresh();
  assert!(
    freshness.has_changed(),
    "an epoch that saw two header contents cannot certify freshness"
  );
  freshness.begin_refresh();
  let parsed = vorpal_lang_registry::SgLang::from_path(path).unwrap();
  extractor
    .extract_product_from_root(path, &parsed.grep(source))
    .unwrap();
  freshness.finish_refresh();
  assert!(!freshness.has_changed());
  // Warm replay identity also publishes its exact consulted inputs.
  freshness.begin_refresh();
  extractor.extraction_identity_for_path(path).unwrap();
  freshness.finish_refresh();
  assert!(!freshness.has_changed());
  fs::remove_file(&header).unwrap();
  assert!(freshness.has_changed());
  #[cfg(unix)]
  {
    let alias = base.join("alias.cc");
    std::os::unix::fs::symlink(path, &alias).unwrap();
    freshness.begin_refresh();
    extractor
      .extract_product(alias.to_str().unwrap(), source)
      .unwrap();
    freshness.finish_refresh();
    assert!(
      !freshness.has_changed(),
      "declining redirected source proof is not a perpetual freshness failure"
    );
    fs::remove_file(&alias).unwrap();
    fs::write(&alias, source).unwrap();
    assert!(
      freshness.has_changed(),
      "removing source redirection changes its observation even with equal bytes"
    );
  }
  fs::remove_dir_all(base).unwrap();
}
