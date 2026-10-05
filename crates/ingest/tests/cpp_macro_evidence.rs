use std::path::Path;
use vorpal_ingest::cpp_macro_evidence::{audit, audit_with_roots};

#[test]
fn uncanonicalized_macro_names_cannot_claim_effect_free_intervals() {
  for name in ["α", "\\u03B1", "$restore"] {
    let source = format!(
      "#define CHECK(x) {{ sink(x); }}\n#define {name} 0\nvoid run() {{ CHECK(value()) }}\n"
    );
    let evidence = audit(Path::new("unicode.cc"), &source);
    assert!(
      evidence.bindings.is_empty(),
      "{name}: {:?}",
      evidence.bindings
    );
  }
  let source =
    "#define CHECK(x) { sink(x, \"α\", R\"(\\u03B1)\"); /* α */ }\nvoid run() { CHECK(value()) }\n";
  assert_eq!(audit(Path::new("unicode.cc"), source).bindings.len(), 1);
}

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
#define NESTED(x) { void local() { sink(x); } }
#define METHOD(x) { struct Local { void local() { sink(x); } }; }
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

#[test]
fn conditional_definedness_groups_preserve_only_unaffected_entering_definitions() {
  let definition = "#define CHECK(x) { function(x); }\n";
  for group in [
    "#ifdef PLATFORM\nstruct Windows {};\n#else\nstruct Other {};\n#endif\n",
    "#ifndef PLATFORM\n#ifdef DEBUG\nstruct Debug {};\n#endif\n#endif\n",
    "#ifdef PLATFORM\n#define OTHER(x) { other(x); }\n#endif\n",
  ] {
    let source = format!("{definition}{group}CHECK(argument())\n");
    let evidence = audit(Path::new("proof.cc"), &source);
    let old = evidence
      .at("CHECK", source.find("CHECK(argument").unwrap())
      .unwrap();
    assert_eq!(old.definition_span, 0..definition.len());
    assert!(evidence.at("OTHER", source.len() - 1).is_none());
  }
  for group in [
    "#ifdef PLATFORM\n#undef CHECK\n#endif\n",
    "#ifndef PLATFORM\n#else\n#define CHECK(x) { other(x); }\n#endif\n",
    "#ifdef PLATFORM\n#pragma pop_macro(\"CHECK\")\n#endif\n",
    "#ifdef PLATFORM\n#if EXPAND()\nstruct Other {};\n#endif\n#endif\n",
    "#ifdef PLATFORM\nstruct Other {};\n",
  ] {
    let source = format!("{definition}{group}CHECK(argument())\n");
    assert!(
      audit(Path::new("proof.cc"), &source)
        .at("CHECK", source.find("CHECK(argument").unwrap())
        .is_none(),
      "{group}"
    );
  }
}

#[test]
fn conditional_includes_track_all_branches_and_cannot_introduce_definitions() {
  let fixture = Fixture::new();
  let first = fixture.0.join("first.h");
  let second = fixture.0.join("second.h");
  std::fs::write(&first, "struct First {};\n#define NEW(x) { other(x); }\n").unwrap();
  std::fs::write(&second, "struct Second {};\n").unwrap();
  let source = "#define CHECK(x) { function(x); }\n#ifdef PLATFORM\n#include \"first.h\"\n#else\n#include \"second.h\"\n#endif\nCHECK(argument())\n";
  let path = fixture.0.join("proof.cc");
  let before = audit(&path, source);
  assert!(
    before
      .at("CHECK", source.find("CHECK(argument").unwrap())
      .is_some()
  );
  assert!(before.bindings.iter().all(|b| b.definition.name != "NEW"));
  assert_eq!(before.dependencies.len(), 2);
  std::fs::write(&second, "#undef CHECK\n").unwrap();
  let changed = audit(&path, source);
  assert!(
    changed
      .at("CHECK", source.find("CHECK(argument").unwrap())
      .is_none()
  );
  assert_ne!(before.dependency_identity(), changed.dependency_identity());
  std::fs::remove_file(&second).unwrap();
  let missing = audit(&path, source);
  assert!(
    missing
      .at("CHECK", source.find("CHECK(argument").unwrap())
      .is_none()
  );
  assert!(missing.dependencies.iter().any(|d| d.digest.is_none()));
}

struct Fixture(std::path::PathBuf);
impl Fixture {
  fn new() -> Self {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = physical_temp_dir().join(format!(
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

#[test]
fn only_definite_pragma_once_visits_skip_repeated_header_effects() {
  let fixture = Fixture::new();
  let header = fixture.0.join("once.h");
  let path = fixture.0.join("run.cc");
  std::fs::write(&header, "#pragma once\n#undef CHECK\n").unwrap();
  let source =
    "#include \"once.h\"\n#define CHECK(x) { effect(x); }\n#include \"once.h\"\nCHECK(value())\n";
  let evidence = audit(&path, source);
  assert!(
    evidence
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_some()
  );
  assert_eq!(evidence.dependencies.len(), 1);
  let identity = evidence.dependency_identity();
  for source in [
    "#ifdef PLATFORM\n#include \"once.h\"\n#endif\n#define CHECK(x) { effect(x); }\n#include \"once.h\"\nCHECK(value())\n",
    "#include \"outer.h\"\n#define CHECK(x) { effect(x); }\n#include \"once.h\"\nCHECK(value())\n",
  ] {
    std::fs::write(
      fixture.0.join("outer.h"),
      "#ifdef PLATFORM\n#include \"once.h\"\n#endif\n",
    )
    .unwrap();
    assert!(
      audit(&path, source)
        .at("CHECK", source.find("CHECK(value").unwrap())
        .is_none()
    );
  }
  for replacement in [
    "#undef CHECK\n",
    "#ifdef PLATFORM\n#pragma once\n#endif\n#undef CHECK\n",
  ] {
    std::fs::write(&header, replacement).unwrap();
    let changed = audit(&path, source);
    assert_ne!(identity, changed.dependency_identity());
    assert!(
      changed
        .at("CHECK", source.find("CHECK(value").unwrap())
        .is_none()
    );
  }
}

#[test]
fn definite_once_breaks_guarded_recursion_but_not_unguarded_cycles() {
  let fixture = Fixture::new();
  let path = fixture.0.join("run.cc");
  let header = fixture.0.join("once.h");
  let source = "#include \"once.h\"\nCHECK(value())\n";
  std::fs::write(
    &header,
    "#pragma once\n#define CHECK(x) { effect(x); }\n#include \"once.h\"\n",
  )
  .unwrap();
  assert!(
    audit(&path, source)
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_some()
  );
  std::fs::write(
    &header,
    "#define CHECK(x) { effect(x); }\n#include \"once.h\"\n#pragma once\n",
  )
  .unwrap();
  assert!(
    audit(&path, source)
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_none()
  );
}

#[test]
fn possible_once_visits_cannot_claim_later_redefinitions_execute() {
  let fixture = Fixture::new();
  let header = fixture.0.join("once.h");
  let path = fixture.0.join("run.cc");
  let source = "#ifdef PLATFORM\n#include \"once.h\"\n#endif\n#define CHECK(x) ordinary(x)\n#include \"once.h\"\nCHECK(value())\n";
  std::fs::write(&header, "#pragma once\n#define CHECK(x) { effect(x); }\n").unwrap();
  assert!(
    audit(&path, source)
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_none()
  );
  // The same uncertainty applies to a pragma inside the header's own branch.
  std::fs::write(
    &header,
    "#ifdef PLATFORM\n#pragma once\n#endif\n#define CHECK(x) { effect(x); }\n",
  )
  .unwrap();
  let source =
    "#include \"once.h\"\n#define CHECK(x) ordinary(x)\n#include \"once.h\"\nCHECK(value())\n";
  assert!(
    audit(&path, source)
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_none()
  );
}

#[test]
fn possibly_skipped_outer_headers_do_not_mark_nested_once_as_definite() {
  let fixture = Fixture::new();
  let path = fixture.0.join("run.cc");
  std::fs::write(
    fixture.0.join("outer.h"),
    "#pragma once\n#ifdef PLATFORM\n#include \"inner.h\"\n#endif\n",
  )
  .unwrap();
  std::fs::write(fixture.0.join("inner.h"), "#pragma once\n#undef CHECK\n").unwrap();
  let source = "#ifdef FIRST\n#include \"outer.h\"\n#endif\n#include \"outer.h\"\n#define CHECK(x) { effect(x); }\n#include \"inner.h\"\nCHECK(value())\n";
  assert!(
    audit(&path, source)
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_none()
  );
  // Alternative branches still cannot establish a definite nested visit.
  std::fs::write(
    fixture.0.join("outer.h"),
    "#pragma once\n#ifdef PLATFORM\nstruct First {};\n#else\n#include \"inner.h\"\n#endif\n",
  )
  .unwrap();
  assert!(
    audit(&path, source)
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_none()
  );
}

#[test]
fn nonexpanding_conditions_preserve_only_unchanged_entering_definitions() {
  for condition in [
    "0",
    "1",
    "defined(PLATFORM)",
    "defined PLATFORM",
    "defined(A) && !defined(B)",
    "(0 || defined(A)) && 1",
  ] {
    let source = format!(
      "#define CHECK(x) {{ effect(x); }}\n#if {condition}\nstruct First {{}};\n#elif !defined(OTHER) || 0\nstruct Second {{}};\n#else\nstruct Third {{}};\n#endif\nCHECK(value())\n"
    );
    let evidence = audit(Path::new("proof.cc"), &source);
    assert!(
      evidence
        .at("CHECK", source.find("CHECK(value").unwrap())
        .is_some(),
      "{condition}"
    );
    let changed = source.replace("struct Second {};", "#undef CHECK");
    assert!(
      audit(Path::new("proof.cc"), &changed)
        .at("CHECK", changed.find("CHECK(value").unwrap())
        .is_none(),
      "possible undef: {condition}"
    );
  }
  for condition in [
    "UNKNOWN",
    "UNKNOWN()",
    "1 / 0",
    "defined(A) && UNKNOWN",
    "2",
  ] {
    let source = format!(
      "#define CHECK(x) {{ effect(x); }}\n#if {condition}\nstruct First {{}};\n#endif\nCHECK(value())\n"
    );
    assert!(
      audit(Path::new("proof.cc"), &source)
        .at("CHECK", source.find("CHECK(value").unwrap())
        .is_none(),
      "{condition}"
    );
  }
}

#[test]
fn literal_condition_branches_do_not_promote_new_definitions_or_skip_effects() {
  for condition in ["0", "1", "defined(PLATFORM)"] {
    let source =
      format!("#if {condition}\n#define NEW(x) {{ effect(x); }}\n#endif\nNEW(value())\n");
    assert!(audit(Path::new("proof.cc"), &source).bindings.is_empty());
    let source = format!(
      "#define CHECK(x) {{ effect(x); }}\n#if {condition}\nstruct First {{}};\n#elif UNKNOWN\nstruct Second {{}};\n#endif\nCHECK(value())\n"
    );
    assert!(
      audit(Path::new("proof.cc"), &source)
        .at("CHECK", source.find("CHECK(value").unwrap())
        .is_none()
    );
  }
}

#[test]
fn nonexpanding_condition_includes_track_even_literal_inactive_branches() {
  let fixture = Fixture::new();
  let first = fixture.0.join("first.h");
  let second = fixture.0.join("second.h");
  std::fs::write(&first, "struct First {};\n").unwrap();
  std::fs::write(&second, "struct Second {};\n").unwrap();
  let source = "#define CHECK(x) { effect(x); }\n#if 0\n#include \"first.h\"\n#else\n#include \"second.h\"\n#endif\nCHECK(value())\n";
  let path = fixture.0.join("proof.cc");
  let before = audit(&path, source);
  assert!(
    before
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_some()
  );
  assert_eq!(before.dependencies.len(), 2);
  std::fs::write(&first, "#undef CHECK\n").unwrap();
  let changed = audit(&path, source);
  assert_ne!(before.dependency_identity(), changed.dependency_identity());
  assert!(
    changed
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_none(),
    "the audit must not evaluate #if 0"
  );
}

#[test]
fn opaque_include_or_directive_effects_prevent_later_proof_restarts() {
  for boundary in [
    "#include \"unknown.h\"",
    "#pragma push_macro(\"CHECK\")",
    "#if UNKNOWN\nstruct First {};\n#endif",
    "#if defined(PLATFORM)\n#pragma push_macro(\"CHECK\")\n#endif",
    "#ifdef PLATFORM\n#unknown effect\n#endif",
  ] {
    let source =
      format!("{boundary}\n#define CHECK(x) {{ effect(x); }}\nRESTORE();\nCHECK(value())\n");
    assert!(
      audit(Path::new("proof.cc"), &source).bindings.is_empty(),
      "{boundary}"
    );
  }
}

#[cfg(any(unix, windows))]
fn make_directory_alias(target: &std::path::Path, alias: &std::path::Path) {
  #[cfg(unix)]
  std::os::unix::fs::symlink(target, alias).unwrap();
  #[cfg(windows)]
  {
    let result = std::process::Command::new("cmd")
      .args(["/d", "/c", "mklink", "/J"])
      .arg(alias)
      .arg(target)
      .output()
      .unwrap();
    assert!(
      result.status.success(),
      "{}",
      String::from_utf8_lossy(&result.stderr)
    );
  }
}

#[cfg(any(unix, windows))]
#[test]
fn directory_aliases_cannot_change_quoted_include_proofs() {
  let fixture = Fixture::new();
  let real = fixture.0.join("real");
  let headers = real.join("headers");
  std::fs::create_dir_all(&headers).unwrap();
  let alias = fixture.0.join("alias");
  make_directory_alias(&headers, &alias);
  std::fs::write(headers.join("proof.h"), "#include \"../detail.h\"\n").unwrap();
  std::fs::write(real.join("detail.h"), "#define CHECK(x) { effect(x); }\n").unwrap();
  std::fs::write(
    fixture.0.join("detail.h"),
    "#define CHECK(x) expression(x)\n",
  )
  .unwrap();
  let source = "#include \"alias/proof.h\"\nCHECK(value())\n";
  let evidence = audit(&fixture.0.join("run.cc"), source);
  assert!(evidence.bindings.is_empty());
  assert!(
    evidence
      .dependencies
      .iter()
      .any(|d| d.path.ends_with("alias/proof.h") && d.digest.is_none())
  );
  let rooted = audit_with_roots(
    &fixture.0.join("run.cc"),
    "#include <proof.h>\nCHECK(value())\n",
    std::slice::from_ref(&alias),
  );
  assert!(rooted.bindings.is_empty());
  assert_eq!(
    rooted.include_roots.as_slice(),
    std::slice::from_ref(&alias),
    "root spelling must not erase the alias"
  );
  let direct = "#define CHECK(x) { effect(x); }\nCHECK(value())\n";
  assert!(
    audit(&alias.join("run.cc"), direct).bindings.is_empty(),
    "source parent aliases are also rejected"
  );
  // The alias is removed directly; no recursive operation follows its target.
  #[cfg(windows)]
  std::fs::remove_dir(&alias).unwrap();
  #[cfg(unix)]
  std::fs::remove_file(&alias).unwrap();
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
fn split_runtime_guards_remain_opaque_macro_evidence_boundaries() {
  let source = "#define CHECK(x) { sink(x); }\nvoid split() {\n#ifdef PLATFORM\nif (first()) {\n#else\nif (second()) {\n#endif\nshared();\n#ifdef PLATFORM\n} else { fallback(); }\n#else\n} else { fallback(); }\n#endif\n}\nvoid run() { CHECK(value()) }\n";
  let evidence = audit(Path::new("guards.cc"), source);
  assert!(
    evidence
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_none()
  );
}

#[test]
fn literal_macro_stack_pragmas_only_invalidate_the_named_binding() {
  let source = "#define CHECK(x) { use(x); }\n#pragma push_macro(\"OTHER\")\n#pragma pop_macro(\"OTHER\")\nvoid run() { CHECK(value()) }\n";
  let evidence = audit(Path::new("stack.cc"), source);
  assert!(
    evidence
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_some()
  );
  assert!(evidence.macro_names.contains("OTHER"));
  for argument in [
    "pop_macro(\"CHECK\")",
    "pop_macro(NAME)",
    "pop_macro(\"\\u0043HECK\")",
    "warning(push)",
  ] {
    let source = format!(
      "#define CHECK(x) {{ use(x); }}\n#pragma {argument}\nvoid run() {{ CHECK(value()) }}\n"
    );
    assert!(
      audit(Path::new("stack.cc"), &source)
        .at("CHECK", source.find("CHECK(value").unwrap())
        .is_none(),
      "{argument}"
    );
  }
  let source = "#define CHECK(x) { use(x); }\n#ifdef PLATFORM\n#pragma pop_macro(\"OTHER\")\n#endif\nvoid run() { CHECK(value()) }\n";
  assert!(
    audit(Path::new("stack.cc"), source)
      .at("CHECK", source.find("CHECK(value").unwrap())
      .is_some()
  );
}
