use std::path::Path;
use vorpal_ingest::cpp_include_context::{IncludeContext, audit_context};

fn physical_temp() -> tempfile::TempDir {
  // Some systems redirect their default temp directory. Test the physical
  // location, while separately checking that source redirects decline.
  let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
  tempfile::tempdir_in(base).unwrap()
}

fn verify_mapping(context: &IncludeContext) {
  let mut cursor = 0;
  for piece in &context.pieces {
    assert_eq!(piece.composed.start, cursor);
    let original = &context.files[piece.original.file].source;
    assert_eq!(
      &context.source[piece.composed.clone()],
      &original[piece.original.bytes.clone()]
    );
    cursor = piece.composed.end;
  }
  assert_eq!(cursor, context.source.len());
  let spans = context.physical_spans(0..cursor).unwrap();
  let reconstructed: String = spans
    .iter()
    .map(|span| &context.files[span.file].source[span.bytes.clone()])
    .collect();
  assert_eq!(reconstructed, context.source);
  assert!(context.physical_spans(cursor..cursor).is_none());
  assert!(context.physical_spans(0..cursor + 1).is_none());
}

#[test]
fn template_and_function_cross_real_include_boundaries_without_synthetic_bytes() {
  for newline in ["\n", "\r\n"] {
    let dir = physical_temp();
    let root = dir.path().join("root.cc");
    let one = dir.path().join("one.cc");
    let two = dir.path().join("two.cc");
    let three = dir.path().join("three.cc");
    let write =
      |path: &Path, text: &str| std::fs::write(path, text.replace('\n', newline)).unwrap();
    write(
      &root,
      "#include <unavailable_sdk.h>\nnamespace Demo {\n#include \"one.cc\"\n#include \"two.cc\"\n#include \"three.cc\"\n}\n",
    );
    write(&one, "// α: original UTF-8 bytes\ntemplate <typename T>\n");
    write(&two, "T read(T value) {\n  before(value);\n");
    write(&three, "  after(value);\n  return value;\n}\n");
    let context = audit_context(&root, &[three.clone(), one.clone(), two.clone()]).unwrap();
    verify_mapping(&context);
    assert_eq!(context.files.len(), 4);
    assert_eq!(context.includes.len(), 3);
    assert!(context.source.contains("#include <unavailable_sdk.h>"));
    assert!(!context.source.contains("#include \"one.cc\""));
    assert_eq!(context.files[1].path, std::fs::canonicalize(&one).unwrap());
    let start = context.source.find("template <typename T>").unwrap();
    let end =
      context.source.find("return value;").unwrap() + "return value;".len() + newline.len() + 1;
    let spans = context.physical_spans(start..end).unwrap();
    assert_eq!(
      spans.iter().map(|span| span.file).collect::<Vec<_>>(),
      [1, 2, 3]
    );
    assert_eq!(
      spans[0].bytes.start,
      std::fs::read_to_string(&one)
        .unwrap()
        .find("template")
        .unwrap()
    );
    assert_eq!(spans[1].bytes.start, 0);
    assert_eq!(spans[2].bytes.start, 0);
    #[cfg(feature = "builtin-parser")]
    {
      use vorpal_core::{Language, tree_sitter::LanguageExt};
      let parsed = vorpal_language::with_cpp_statement_macros(&[], || {
        vorpal_lang_registry::SgLang::from_path("context.cc")
          .unwrap()
          .grep(&context.source)
      });
      assert!(!parsed.root().has_error());
      let template = parsed
        .root()
        .dfs()
        .find(|node| node.kind() == "template_declaration")
        .unwrap();
      assert_eq!(
        context
          .physical_spans(template.range())
          .unwrap()
          .iter()
          .map(|span| span.file)
          .collect::<Vec<_>>(),
        [1, 2, 3]
      );
      for name in ["before", "after"] {
        let call = parsed
          .root()
          .dfs()
          .find(|node| node.kind() == "call_expression" && node.text().starts_with(name))
          .unwrap();
        let physical = context.physical_spans(call.range()).unwrap();
        assert_eq!(physical.len(), 1);
        assert_eq!(physical[0].file, if name == "before" { 2 } else { 3 });
        assert_eq!(
          &context.files[physical[0].file].source[physical[0].bytes.clone()],
          format!("{name}(value)")
        );
      }
    }
  }
}

#[test]
fn selected_include_order_and_fresh_inputs_shape_identity() {
  let dir = physical_temp();
  let root = dir.path().join("root.cc");
  let child = dir.path().join("child.cc");
  std::fs::write(&root, "#include \"child.cc\"\n").unwrap();
  std::fs::write(&child, "void first() {}\n").unwrap();
  let before = audit_context(&root, std::slice::from_ref(&child)).unwrap();
  assert!(before.is_current());
  std::fs::write(&child, "void second() {}\n").unwrap();
  assert!(!before.is_current());
  let after = audit_context(&root, std::slice::from_ref(&child)).unwrap();
  assert_ne!(before.identity(), after.identity());
  std::fs::remove_file(&child).unwrap();
  assert!(!after.is_current());
  assert!(audit_context(&root, &[child]).is_err());
}

#[test]
fn uncertain_conditional_repeated_and_unused_inputs_decline() {
  let dir = physical_temp();
  let root = dir.path().join("root.cc");
  let child = dir.path().join("child.cc");
  std::fs::write(&child, "value();\n").unwrap();
  for text in [
    "#if FLAG\n#include \"child.cc\"\n#endif\n",
    "#include \"child.cc\"\n#include \"child.cc\"\n",
    "#include HEADER\n",
    "#include \"child.cc\"\n/* unterminated",
    "#include <child.cc>\n",
  ] {
    std::fs::write(&root, text).unwrap();
    assert!(
      audit_context(&root, std::slice::from_ref(&child)).is_err(),
      "{text:?}"
    );
  }
  std::fs::write(&root, "#include \"child.cc\"\n").unwrap();
  assert!(audit_context(&root, &[child.clone(), child.clone()]).is_err());
  std::fs::write(&child, "#include \"root.cc\"\n").unwrap();
  assert!(audit_context(&root, &[child, root.clone()]).is_err());
}

#[test]
fn no_newline_or_delimiter_is_invented_at_include_edges() {
  let dir = physical_temp();
  let root = dir.path().join("root.cc");
  let child = dir.path().join("child.cc");
  std::fs::write(&root, "#include \"child.cc\"\nbody").unwrap();
  std::fs::write(&child, "prefix").unwrap();
  assert!(audit_context(&root, &[child]).is_err());
}

#[test]
fn include_depth_limit_declines_before_unbounded_recursion() {
  let dir = physical_temp();
  let paths: Vec<_> = (0..19)
    .map(|i| dir.path().join(format!("piece{i}.cc")))
    .collect();
  for (i, path) in paths.iter().enumerate() {
    let source = if i + 1 < paths.len() {
      format!("#include \"piece{}.cc\"\n", i + 1)
    } else {
      "void last() {}\n".into()
    };
    std::fs::write(path, source).unwrap();
  }
  assert!(
    audit_context(&paths[0], &paths[1..])
      .unwrap_err()
      .0
      .contains("limit")
  );
}

#[test]
fn oversized_inputs_and_selections_decline_with_bounded_reads() {
  let dir = physical_temp();
  let root = dir.path().join("root.cc");
  let file = std::fs::File::create(&root).unwrap();
  file.set_len(16 * 1024 * 1024 + 1).unwrap();
  drop(file);
  assert!(
    audit_context(&root, &[])
      .unwrap_err()
      .0
      .contains("byte limit")
  );
  std::fs::write(&root, "void root() {}\n").unwrap();
  assert!(
    audit_context(&root, &vec![root.clone(); 128])
      .unwrap_err()
      .0
      .contains("file limit")
  );
}

#[test]
fn literal_strings_and_comments_cannot_create_include_edges() {
  let dir = physical_temp();
  let root = dir.path().join("root.cc");
  let child = dir.path().join("child.cc");
  std::fs::write(
    &root,
    "/*\n#include \"child.cc\"\n*/\nconst char* text = R\"tag(\n#include \"child.cc\"\n)tag\";\n",
  )
  .unwrap();
  std::fs::write(&child, "void hidden() {}\n").unwrap();
  assert!(audit_context(&root, &[child]).is_err());
  let context = audit_context(&root, &[]).unwrap();
  assert_eq!(context.source, std::fs::read_to_string(&root).unwrap());
  assert!(context.includes.is_empty());
}

#[cfg(feature = "builtin-parser")]
#[test]
fn genuine_missing_semicolon_keeps_its_original_fragment_span() {
  use vorpal_core::{Language, tree_sitter::LanguageExt};
  let dir = physical_temp();
  let root = dir.path().join("root.cc");
  let child = dir.path().join("child.cc");
  std::fs::write(&root, "void run() {\n#include \"child.cc\"\n}\n").unwrap();
  std::fs::write(&child, "first() second();\n").unwrap();
  let context = audit_context(&root, &[child]).unwrap();
  let parsed = vorpal_language::with_cpp_statement_macros(&[], || {
    vorpal_lang_registry::SgLang::from_path("context.cc")
      .unwrap()
      .grep(&context.source)
  });
  assert!(parsed.root().has_error());
  let error = parsed
    .root()
    .dfs()
    .find(|node| node.kind() == "ERROR")
    .unwrap();
  let spans = context.physical_spans(error.range()).unwrap();
  assert_eq!(spans.len(), 1);
  assert_eq!(spans[0].file, 1);
  assert_eq!(
    &context.files[1].source[spans[0].bytes.clone()],
    error.text()
  );
}

#[cfg(unix)]
#[test]
fn symlink_include_and_source_decline() {
  let dir = physical_temp();
  let root = dir.path().join("root.cc");
  let child = dir.path().join("child.cc");
  let link = dir.path().join("link.cc");
  std::fs::write(&root, "#include \"link.cc\"\n").unwrap();
  std::fs::write(&child, "void real() {}\n").unwrap();
  std::os::unix::fs::symlink(&child, &link).unwrap();
  assert!(audit_context(&root, std::slice::from_ref(&link)).is_err());
  assert!(audit_context(&link, &[]).is_err());
  // A redirect through an unselected spelling must not match its real target.
  assert!(audit_context(&root, &[child]).is_err());
}
