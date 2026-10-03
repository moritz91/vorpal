use std::path::Path;
use vorpal_ingest::cpp_macro_evidence::{audit, audit_with_roots};

#[test]
fn statement_definitions_have_original_spans_and_ordered_lifetimes() {
  let source = "before();\n#define CHECK(x) if (!(x)) { throw failure(); }\nCHECK(argument())\n#undef CHECK\nCHECK(later())\n";
  let evidence = audit(Path::new("proof.cc"), source);
  assert!(evidence.at("CHECK", 0).is_none());
  let definition = evidence
    .at("CHECK", source.find("CHECK(argument").unwrap())
    .unwrap();
  assert_eq!(definition.parameters, 1);
  assert_eq!(
    &source[definition.definition_span.clone()],
    "#define CHECK(x) if (!(x)) { throw failure(); }\n"
  );
  assert!(
    evidence
      .at("CHECK", source.find("CHECK(later").unwrap())
      .is_none()
  );
}

#[test]
fn only_complete_statement_replacements_are_evidence() {
  let source = r#"
#define EXPR(x) function(x)
#define DEFINE(x) void function_##x()
#define LOOP(x) do { function(x); } while (false)
#define BROKEN(x) if (x) { function(x)
#define COMPLETE(x) if (x) { function(x); }
#define SAFE(x) try { function(x); } catch (...) { failure(); }
#define VARIADIC(...) { function(__VA_ARGS__); }
#define DUPLICATE(x, x) { function(x); }
void run() {}
"#;
  let evidence = audit(Path::new("proof.cc"), source);
  assert_eq!(
    evidence
      .bindings
      .iter()
      .map(|b| b.definition.name.as_str())
      .collect::<Vec<_>>(),
    vec!["COMPLETE", "SAFE"]
  );
}

#[test]
fn uncertain_directives_and_redefinitions_close_bindings() {
  for directive in [
    "#define CHECK(x) function(x)",
    "#if MAYBE\n#define CHECK(x) function(x)\n#endif",
    "#pragma pop_macro(\"CHECK\")",
    "#include <unknown.h>",
  ] {
    let source = format!(
      "#define CHECK(x) {{ function(x); }}\nCHECK(first())\n{directive}\nCHECK(second())\n"
    );
    let evidence = audit(Path::new("proof.cc"), &source);
    assert!(
      evidence
        .at("CHECK", source.find("CHECK(first").unwrap())
        .is_some(),
      "{directive}"
    );
    assert!(
      evidence
        .at("CHECK", source.find("CHECK(second").unwrap())
        .is_none(),
      "{directive}"
    );
  }
}

#[test]
fn comments_and_continuations_keep_original_definition_bytes() {
  let lf = "#define CHECK(x) if (x) { /* note */ \\\n  function(x); }\nCHECK(argument())\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let evidence = audit(Path::new("proof.cc"), &source);
    let definition = evidence
      .at("CHECK", source.find("CHECK(argument").unwrap())
      .unwrap();
    assert!(source[definition.definition_span.clone()].contains("/* note */"));
  }
}

struct Fixture(std::path::PathBuf);
impl Fixture {
  fn new() -> Self {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
      "vorpal-macro-evidence-{}-{}",
      std::process::id(),
      NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    Self(path)
  }
}
impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

#[test]
fn transitive_includes_track_content_changes_and_missing_headers() {
  let fixture = Fixture::new();
  let source = "#include \"outer.h\"\nCHECK(argument())\n";
  std::fs::write(
    fixture.0.join("outer.h"),
    "#pragma once\n#include \"inner.h\"\n",
  )
  .unwrap();
  let header = fixture.0.join("inner.h");
  std::fs::write(&header, "#define CHECK(x) if (x) { function(x); }\n").unwrap();
  let before = audit(&fixture.0.join("proof.cc"), source);
  assert_eq!(before.dependencies.len(), 2);
  assert_eq!(
    before
      .at("CHECK", source.find("CHECK(argument").unwrap())
      .unwrap()
      .definition_path,
    header.canonicalize().unwrap()
  );
  std::fs::write(&header, "#define CHECK(x) function(x)\n").unwrap();
  let after = audit(&fixture.0.join("proof.cc"), source);
  assert_ne!(before.dependencies, after.dependencies);
  assert!(
    after
      .at("CHECK", source.find("CHECK(argument").unwrap())
      .is_none()
  );
  std::fs::remove_file(&header).unwrap();
  let missing = audit(&fixture.0.join("proof.cc"), source);
  assert!(missing.bindings.is_empty());
  assert!(
    missing
      .dependencies
      .iter()
      .any(|d| d.path == fixture.0.canonicalize().unwrap().join("inner.h") && d.digest.is_none())
  );
}

#[test]
fn include_cycles_and_unknown_branches_do_not_supply_macro_evidence() {
  let fixture = Fixture::new();
  std::fs::write(
    fixture.0.join("cycle.h"),
    "#define CHECK(x) { function(x); }\n#include \"cycle.h\"\n",
  )
  .unwrap();
  let evidence = audit(
    &fixture.0.join("proof.cc"),
    "#include \"cycle.h\"\nCHECK(argument())\n",
  );
  assert!(evidence.bindings.is_empty());
  let source = "#if MAYBE\n#define CHECK(x) { function(x); }\n#endif\nCHECK(argument())\n";
  assert!(audit(Path::new("proof.cc"), source).bindings.is_empty());
}

#[test]
fn ordinary_uppercase_functions_and_oversized_includes_are_not_evidence() {
  let source = "void ASSERT(int); void run() { ASSERT(1) }";
  assert!(audit(Path::new("proof.cc"), source).bindings.is_empty());
  let fixture = Fixture::new();
  std::fs::write(fixture.0.join("large.h"), vec![b' '; 4 * 1024 * 1024 + 1]).unwrap();
  let source = "#define CHECK(x) { function(x); }\n#include \"large.h\"\nCHECK(argument())\n";
  let evidence = audit(&fixture.0.join("proof.cc"), source);
  assert!(
    evidence
      .at("CHECK", source.find("CHECK(argument").unwrap())
      .is_none()
  );
  assert_eq!(evidence.dependencies.len(), 1);
  assert!(evidence.dependencies[0].digest.is_none());
}

#[test]
fn ordered_roots_and_new_local_headers_change_dependency_identity() {
  let fixture = Fixture::new();
  let first = fixture.0.join("first");
  let second = fixture.0.join("second");
  std::fs::create_dir(&first).unwrap();
  std::fs::create_dir(&second).unwrap();
  std::fs::write(first.join("sdk.h"), "#define CHECK(x) { first(x); }\n").unwrap();
  std::fs::write(second.join("sdk.h"), "#define CHECK(x) { second(x); }\n").unwrap();
  let source = "#include \"sdk.h\"\nCHECK(argument())\n";
  let path = fixture.0.join("proof.cc");
  let roots = [first.clone(), second.clone()];
  let before = audit_with_roots(&path, source, &roots);
  let offset = source.find("CHECK(argument").unwrap();
  assert_eq!(
    before.at("CHECK", offset).unwrap().definition_path,
    first.join("sdk.h").canonicalize().unwrap()
  );
  assert!(
    before
      .dependencies
      .iter()
      .any(|d| d.path.ends_with("sdk.h") && d.digest.is_none())
  );
  let reordered = audit_with_roots(&path, source, &[second, first]);
  assert_ne!(
    before.dependency_identity(),
    reordered.dependency_identity()
  );
  std::fs::write(fixture.0.join("sdk.h"), "#define CHECK(x) function(x)\n").unwrap();
  let shadowed = audit_with_roots(&path, source, &roots);
  assert!(shadowed.at("CHECK", offset).is_none());
  assert_ne!(before.dependency_identity(), shadowed.dependency_identity());
}

#[test]
fn angle_includes_use_explicit_roots_and_propagate_to_nested_headers() {
  let fixture = Fixture::new();
  let root = fixture.0.join("sdk");
  std::fs::create_dir(&root).unwrap();
  std::fs::write(fixture.0.join("sdk.h"), "#define WRONG(x) { local(x); }\n").unwrap();
  std::fs::write(root.join("sdk.h"), "#include <detail.h>\n").unwrap();
  std::fs::write(root.join("detail.h"), "#define CHECK(x) { target(x); }\n").unwrap();
  let source = "#include <sdk.h>\nCHECK(argument())\n";
  let path = fixture.0.join("proof.cc");
  assert!(audit(&path, source).bindings.is_empty());
  let evidence = audit_with_roots(&path, source, &[root]);
  assert!(
    evidence
      .at("CHECK", source.find("CHECK(argument").unwrap())
      .is_some()
  );
  assert!(
    evidence
      .at("WRONG", source.find("CHECK(argument").unwrap())
      .is_none()
  );
  assert_eq!(evidence.dependencies.len(), 2);
}

#[test]
fn unreadable_first_candidate_does_not_fall_back_to_another_root() {
  let fixture = Fixture::new();
  let root = fixture.0.join("sdk");
  std::fs::create_dir(&root).unwrap();
  // A directory is present at the local candidate path, but cannot be a header.
  std::fs::create_dir(fixture.0.join("sdk.h")).unwrap();
  std::fs::write(root.join("sdk.h"), "#define CHECK(x) { target(x); }\n").unwrap();
  let evidence = audit_with_roots(
    &fixture.0.join("proof.cc"),
    "#include \"sdk.h\"\nCHECK(argument())\n",
    &[root],
  );
  assert!(evidence.bindings.is_empty());
  assert_eq!(evidence.dependencies.len(), 1);
  assert!(evidence.dependencies[0].digest.is_none());
}

#[cfg(unix)]
#[test]
fn file_symlinks_do_not_prove_a_different_quoted_include_directory() {
  let fixture = Fixture::new();
  let real = fixture.0.join("real");
  std::fs::create_dir(&real).unwrap();
  std::fs::write(real.join("proof.h"), "#include \"detail.h\"\n").unwrap();
  std::fs::write(real.join("detail.h"), "#define CHECK(x) { target(x); }\n").unwrap();
  std::os::unix::fs::symlink(real.join("proof.h"), fixture.0.join("proof.h")).unwrap();
  let source = "#include \"proof.h\"\nCHECK(value())\n";
  let evidence = audit(&fixture.0.join("run.cc"), source);
  assert!(evidence.bindings.is_empty());
  assert_eq!(evidence.dependencies.len(), 1);
  assert!(evidence.dependencies[0].digest.is_none());
  std::fs::write(real.join("run.cc"), "#define CHECK(x) { target(x); }\n").unwrap();
  std::os::unix::fs::symlink(real.join("run.cc"), fixture.0.join("run.cc")).unwrap();
  assert!(
    audit(
      &fixture.0.join("run.cc"),
      "#define CHECK(x) { target(x); }\nCHECK(value())"
    )
    .bindings
    .is_empty()
  );
}
