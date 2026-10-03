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
    report.contains("1 of 2 files carry ERROR nodes"),
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
