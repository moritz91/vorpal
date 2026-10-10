#![cfg(feature = "builtin-parser")]
use std::path::{Path, PathBuf};
use vorpal_ingest::OutlineExtractor;

fn fixture(newline: &str) -> (tempfile::TempDir, PathBuf, Vec<PathBuf>) {
  let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
  let dir = tempfile::tempdir_in(base).unwrap();
  let root = dir.path().join("root.cc");
  let includes: Vec<_> = ["prefix.cc", "head.cc", "tail.cc"]
    .into_iter()
    .map(|name| dir.path().join(name))
    .collect();
  let write =
    |path: &Path, source: &str| std::fs::write(path, source.replace('\n', newline)).unwrap();
  write(
    &root,
    "namespace Demo {\n#include \"prefix.cc\"\n#include \"head.cc\"\n#include \"tail.cc\"\n}\n",
  );
  write(&includes[0], "// α UTF-8\ntemplate <typename T>\n");
  write(&includes[1], "T read(T value) {\n  before(value);\n");
  write(
    &includes[2],
    "  after(value);\n  return value;\n}\nint next() { return read(1); }\n",
  );
  (dir, root, includes)
}

#[test]
fn definitions_calls_and_following_functions_keep_original_owners_and_all_parts() {
  for newline in ["\n", "\r\n"] {
    let (_dir, root, includes) = fixture(newline);
    let report = OutlineExtractor::new()
      .unwrap()
      .audit_include_projection(&root, &includes)
      .unwrap();
    assert!(report.diagnostics.is_empty());
    let read = report
      .definitions
      .iter()
      .find(|def| def.entry.name == "read")
      .unwrap();
    let provenance = read.entry.source_context.as_ref().unwrap();
    assert_eq!(provenance.parts.len(), 3);
    assert!(Path::new(&provenance.name.path).ends_with("head.cc"));
    let original = std::fs::read_to_string(&provenance.name.path).unwrap();
    assert_eq!(&original[provenance.name.range.byte_offset.clone()], "read");
    assert_eq!(provenance.name.range.start.line, 0);
    assert_eq!(provenance.name.range.start.column, 2);
    let reconstructed: String = provenance
      .parts
      .iter()
      .map(|part| {
        let source = std::fs::read_to_string(&part.path).unwrap();
        source[part.range.byte_offset.clone()].to_owned()
      })
      .collect();
    assert!(reconstructed.starts_with("template <typename T>"));
    assert!(reconstructed.ends_with('}'));
    assert!(reconstructed.contains("after(value)"));
    let template_type = report
      .references
      .iter()
      .find(|reference| {
        reference.reference.name == "T"
          && Path::new(&reference.physical.path).ends_with("prefix.cc")
      })
      .unwrap();
    assert_eq!(template_type.owner_entity_index, read.entity_index);
    assert!(read.entry.range.byte_offset.end <= original.len());
    let next = report
      .definitions
      .iter()
      .find(|def| def.entry.name == "next")
      .unwrap();
    for (name, file, owner) in [
      ("before", "head.cc", read.entity_index),
      ("after", "tail.cc", read.entity_index),
      ("read", "tail.cc", next.entity_index),
    ] {
      let call = report
        .references
        .iter()
        .find(|reference| reference.reference.name == name)
        .unwrap();
      assert_eq!(call.owner_entity_index, owner, "{name}");
      assert!(Path::new(&call.physical.path).ends_with(file));
      let source = std::fs::read_to_string(&call.physical.path).unwrap();
      let evidence = &source[call.physical.range.byte_offset.clone()];
      assert!(evidence.contains(name));
      assert_eq!(
        call.reference.start as usize,
        call.physical.range.byte_offset.start
      );
      assert_eq!(
        call.reference.end as usize,
        call.physical.range.byte_offset.end
      );
    }
  }
}

#[test]
fn physical_errors_remain_and_header_edits_change_the_identity() {
  let (_dir, root, includes) = fixture("\n");
  let extractor = OutlineExtractor::new().unwrap();
  let clean = extractor
    .audit_include_projection(&root, &includes)
    .unwrap();
  let source = std::fs::read_to_string(&includes[2])
    .unwrap()
    .replace("return value;", "return value");
  std::fs::write(&includes[2], source).unwrap();
  let damaged = extractor
    .audit_include_projection(&root, &includes)
    .unwrap();
  assert_ne!(clean.identity, damaged.identity);
  assert!(!damaged.diagnostics.is_empty());
  assert!(
    damaged
      .diagnostics
      .iter()
      .any(|diagnostic| diagnostic.missing)
  );
  for diagnostic in &damaged.diagnostics {
    assert!(!diagnostic.locations.is_empty());
    assert!(
      diagnostic
        .locations
        .iter()
        .all(|span| Path::new(&span.path).ends_with("tail.cc"))
    );
  }
  std::fs::remove_file(&includes[2]).unwrap();
  assert!(
    extractor
      .audit_include_projection(&root, &includes)
      .is_err()
  );
}

#[test]
fn templated_methods_retain_parent_and_physical_name_capture() {
  let dir = tempfile::tempdir().unwrap();
  let dir_path = std::fs::canonicalize(dir.path()).unwrap();
  let root = dir_path.join("root.cc");
  let header = dir_path.join("part.cc");
  std::fs::write(&root, "struct Box {\n#include \"part.cc\"\n};\n").unwrap();
  std::fs::write(
    &header,
    "template<class T> T get(T value) { return work(value); }\n",
  )
  .unwrap();
  let report = OutlineExtractor::new()
    .unwrap()
    .audit_include_projection(&root, &[header])
    .unwrap();
  let parent = report
    .definitions
    .iter()
    .find(|def| def.entry.name == "Box")
    .unwrap();
  let method = report
    .definitions
    .iter()
    .find(|def| def.entry.name == "get")
    .unwrap();
  assert_eq!(method.parent_entity_index, Some(parent.entity_index));
  assert!(method.entity_path.starts_with("Box.get"));
  let work = report
    .references
    .iter()
    .find(|reference| reference.reference.name == "work")
    .unwrap();
  assert_eq!(work.owner_entity_index, method.entity_index);
  assert!(
    OutlineExtractor::new()
      .unwrap()
      .with_cpp_textual_include_contexts(&[vorpal_ingest::CppTextualIncludeContext {
        root: root.clone(),
        includes: vec![dir_path.join("part.cc")]
      }])
      .is_err(),
    "production must decline foreign member parents, not fabricate a class stub"
  );
}

#[test]
fn repeated_original_identity_declines_before_graph_writer_handoff() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path().join("root.cc");
  let part = dir.path().join("part.cc");
  std::fs::write(&root, "#include \"part.cc\"\n").unwrap();
  std::fs::write(
    &part,
    "int duplicate() { return 1; }\nint duplicate() { return 1; }\n",
  )
  .unwrap();
  assert!(
    OutlineExtractor::new()
      .unwrap()
      .with_cpp_textual_include_contexts(&[vorpal_ingest::CppTextualIncludeContext {
        root,
        includes: vec![part]
      }])
      .is_err(),
    "conflicting physical provenance cannot reach the writer as one identity"
  );
}
