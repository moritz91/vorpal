#![cfg(feature = "builtin-parser")]
use std::path::Path;
use vorpal_ingest::cpp_macro_recovery::audit_recovery;

#[test]
fn proven_statements_keep_argument_calls_and_following_function_spans() {
  let lf = "#define CHECK(x) if (!(x)) { throw 0; }\nvoid run() { CHECK(value()) after(); }\nvoid following() { next(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let audit = audit_recovery(Path::new("fixture.cc"), &source, &[]);
    assert!(!audit.has_error, "{audit:?}");
    assert_eq!(audit.eligible_names, ["CHECK"]);
    assert_eq!(audit.macro_spans.len(), 1);
    assert_eq!(&source[audit.macro_spans[0].clone()], "CHECK(value())");
    assert!(audit.functions.contains(&"following".to_owned()));
    for name in ["value", "after", "next"] {
      let (_, span) = audit.calls.iter().find(|(n, _)| n == name).unwrap();
      assert_eq!(&source[span.clone()], format!("{name}()"));
    }
    assert!(!audit.calls.iter().any(|(name, _)| name == "CHECK"));
  }
}

#[test]
fn unchanged_conditional_groups_keep_recovery_and_real_argument_spans() {
  let prefix = "#define CHECK(x) { function(x); }\n";
  let body = "void run() { CHECK(value()) after(); }\n";
  let safe = format!(
    "{prefix}#ifdef PLATFORM\nstruct First {{}};\n#else\nstruct Second {{}};\n#endif\n{body}"
  );
  let report = audit_recovery(Path::new("fixture.cc"), &safe, &[]);
  assert!(!report.has_error, "{report:?}");
  assert_eq!(report.eligible_names, ["CHECK"]);
  assert_eq!(&safe[report.macro_spans[0].clone()], "CHECK(value())");
  assert!(!report.calls.iter().any(|(name, _)| name == "CHECK"));
  for name in ["value", "after"] {
    let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
    assert_eq!(&safe[span.clone()], format!("{name}()"));
  }
  let invalidated = format!("{prefix}#ifdef PLATFORM\n#undef CHECK\n#endif\n{body}");
  let report = audit_recovery(Path::new("fixture.cc"), &invalidated, &[]);
  assert!(report.has_error);
  assert!(report.eligible_names.is_empty());
}

#[test]
fn ordinary_calls_bad_arity_and_out_of_lifetime_uses_remain_errors() {
  for source in [
    "void run() { CHECK(value()) }",
    "#define CHECK(x) function(x)\nvoid run() { CHECK(value()) }",
    "#define CHECK(x) { function(x); }\nvoid run() { CHECK(value(), second()) }",
    "#define CHECK(x) { function(x); }\nvoid run() { CHECK(template_call<A,B>()) }",
    "#define CHECK(x) { function(x); }\nvoid run() { CHECK(value()) }\n#undef CHECK\nvoid other() { CHECK(value()) }",
    "void before() { CHECK(value()) }\n#define CHECK(x) { function(x); }\nvoid run() { CHECK(value()) }",
    "#define CHECK(x) { function(x); }\nvoid run() { CHECK(value()) genuine() }",
  ] {
    assert!(
      audit_recovery(Path::new("fixture.cc"), source, &[]).has_error,
      "{source}"
    );
  }
}

#[test]
fn parser_reuse_and_parallel_workers_do_not_inherit_macro_context() {
  let proven = "#define CHECK(x) { function(x); }\nvoid run() { CHECK(value()) }";
  let ordinary = "void run() { CHECK(value()) }";
  let workers: Vec<_> = (0..4)
    .map(|_| {
      std::thread::spawn(move || {
        for _ in 0..20 {
          assert!(!audit_recovery(Path::new("fixture.cc"), proven, &[]).has_error);
          assert!(audit_recovery(Path::new("fixture.cc"), ordinary, &[]).has_error);
        }
      })
    })
    .collect();
  for worker in workers {
    worker.join().unwrap();
  }
}

#[test]
fn scoped_context_restores_after_panic_and_nested_parse() {
  use vorpal_core::tree_sitter::LanguageExt;
  use vorpal_language::{SupportLang, with_cpp_statement_macros};
  let source = "void run() { CHECK(value()) }";
  with_cpp_statement_macros(&["CHECK".to_owned()], || {
    assert!(!SupportLang::Cpp.grep(source).root().has_error());
    let result = std::panic::catch_unwind(|| {
      with_cpp_statement_macros(&["OTHER".to_owned()], || panic!("fixture"))
    });
    assert!(result.is_err());
    assert!(!SupportLang::Cpp.grep(source).root().has_error());
  });
  assert!(SupportLang::Cpp.grep(source).root().has_error());
}

#[test]
fn consecutive_statements_and_literal_commas_preserve_arguments() {
  let source = r##"#define CHECK(x) { function(x); }
void run() {
  CHECK("a,b)")
  CHECK(R"tag(a,b))tag")
  CHECK((first(), second()))
  CHECK(/* comma, close) */ value())
  after();
}
"##;
  let audit = audit_recovery(Path::new("fixture.cc"), source, &[]);
  assert!(!audit.has_error, "{audit:?}");
  assert_eq!(audit.macro_spans.len(), 4);
  for name in ["first", "second", "value", "after"] {
    let (_, span) = audit.calls.iter().find(|(n, _)| n == name).unwrap();
    assert_eq!(&source[span.clone()], format!("{name}()"));
  }
  for arguments in ["value(,)", "[] { first(); second(); }()", "{1, 2}"] {
    let source =
      format!("#define CHECK(x) {{ function(x); }}\nvoid run() {{ CHECK({arguments}) }}");
    if arguments == "value(,)" || arguments == "{1, 2}" {
      assert!(audit_recovery(Path::new("fixture.cc"), &source, &[]).has_error);
    } else {
      assert!(!audit_recovery(Path::new("fixture.cc"), &source, &[]).has_error);
    }
  }
}

#[test]
fn header_edit_creation_and_removal_change_fresh_recovery_proof() {
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let dir = physical_temp_dir().join(format!(
    "vorpal-macro-recovery-{}-{nonce}",
    std::process::id()
  ));
  std::fs::create_dir(&dir).unwrap();
  let path = dir.join("fixture.cc");
  let header = dir.join("proof.h");
  let source = "#include \"proof.h\"\nvoid run() { CHECK(value()) }";
  let missing = audit_recovery(&path, source, &[]);
  assert!(missing.has_error);
  std::fs::write(&header, "#define CHECK(x) { function(x); }\n").unwrap();
  let present = audit_recovery(&path, source, &[]);
  assert!(!present.has_error);
  assert_ne!(missing.dependency_identity, present.dependency_identity);
  std::fs::write(&header, "#define CHECK(x) function(x)\n").unwrap();
  let edited = audit_recovery(&path, source, &[]);
  assert!(edited.has_error);
  assert_ne!(present.dependency_identity, edited.dependency_identity);
  std::fs::remove_file(&header).unwrap();
  let removed = audit_recovery(&path, source, &[]);
  assert!(removed.has_error);
  assert_eq!(missing.dependency_identity, removed.dependency_identity);
  std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn production_owned_streaming_and_scan_handoff_share_proof_and_identity() {
  use vorpal_core::Language;
  use vorpal_core::tree_sitter::LanguageExt;
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_lang_registry::SgLang;
  let source = "#define CHECK(x) { function(x); }\nvoid run() { CHECK(value()) after(); }\nvoid following() { next(); }";
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  let path = "fixture.cc";
  let product = extractor.extract_product(path, source).unwrap();
  assert_eq!(product.error_nodes, 0);
  for name in ["value", "after", "next"] {
    let call = product
      .refs
      .iter()
      .find(|r| r.name == name && r.kind == 0)
      .unwrap();
    assert_eq!(
      &source[call.start as usize..call.end as usize],
      format!("{name}()")
    );
  }
  assert!(
    !product
      .refs
      .iter()
      .any(|r| r.name == "CHECK" && r.kind == 0)
  );
  let mut owned = Vec::new();
  encode_product_into(&product, &mut owned);
  let mut streamed = Vec::new();
  extractor
    .extract_product_encoded(path, source, 0, 0, &mut streamed)
    .unwrap();
  assert_eq!(owned, streamed);
  let raw = SgLang::from_path(path).unwrap().grep(source);
  assert!(raw.root().has_error());
  let handoff = extractor.extract_product_from_root(path, &raw).unwrap();
  let mut bank = Vec::new();
  encode_product_into(&handoff, &mut bank);
  assert_eq!(owned, bank);
  // Default extraction never acquires a recovered tree from parser reuse.
  let ordinary = OutlineExtractor::new()
    .unwrap()
    .extract_product(path, source)
    .unwrap();
  assert_ne!(ordinary.grammar_digest, product.grammar_digest);
  let nested = vorpal_language::with_cpp_statement_macros(&["CHECK".to_owned()], || {
    OutlineExtractor::new()
      .unwrap()
      .extract_product(path, source)
      .unwrap()
  });
  let mut expected = Vec::new();
  let mut actual = Vec::new();
  encode_product_into(&ordinary, &mut expected);
  encode_product_into(&nested, &mut actual);
  assert_eq!(
    expected, actual,
    "default extraction clears surrounding scanner context"
  );
}

#[test]
fn pragma_operators_and_invoked_paste_wrappers_cannot_restore_hidden_definitions() {
  for effect in [
    "__pragma(pop_macro(\"CHECK\"));",
    "_Pragma(\"pop_macro(\\\"CHECK\\\")\");",
    "#define RESTORE() __pragma(pop_macro(\"CHECK\"))\nRESTORE();",
    "#define JOIN(a,b) a##b\nJOIN(__pr,agma)(pop_macro(\"CHECK\"));",
    "#define JOIN(a,b) a%:%:b\nJOIN(__pr,agma)(pop_macro(\"CHECK\"));",
    "__pr\\\nagma(pop_macro(\"CHECK\"));",
    "#define JOIN(a,b) a##b\n#define RESTORE() JOIN(__pr,agma)(pop_macro(\"CHECK\"))\nRESTORE();",
  ] {
    let source = format!(
      "#define CHECK(x) expression(x)\n#pragma push_macro(\"CHECK\")\n#undef CHECK\n#define CHECK(x) {{ effect(x); }}\n{effect}\nvoid run() {{ CHECK(value()) }}\n"
    );
    for source in [source.clone(), source.replace('\n', "\r\n")] {
      let report = audit_recovery(Path::new("fixture.cc"), &source, &[]);
      assert!(report.has_error, "{effect}: {report:?}");
      assert!(report.eligible_names.is_empty(), "{effect}: {report:?}");
      assert!(report.macro_spans.is_empty());
    }
  }
}

#[test]
fn inert_operator_mentions_and_unused_paste_definitions_keep_valid_recovery() {
  let source = r#"
// __pragma(pop_macro("CHECK"))
#define TEXT "_Pragma ## __pragma"
#define JOIN(a,b) a##b
#define CHECK(x) { effect(x); }
void run() { const char* text = R"(__pragma ## _Pragma)"; CHECK(value()) }
"#;
  let report = audit_recovery(Path::new("fixture.cc"), source, &[]);
  assert!(!report.has_error, "{report:?}");
  assert_eq!(report.eligible_names, ["CHECK"]);
  assert_eq!(&source[report.macro_spans[0].clone()], "CHECK(value())");
}

#[test]
fn nonexpanding_conditions_keep_recovery_without_selecting_a_branch() {
  for condition in ["0", "1", "defined(A) && !defined(B)"] {
    let source = format!(
      "#define CHECK(x) {{ effect(x); }}\n#if {condition}\nstruct First {{}};\n#else\nstruct Second {{}};\n#endif\nvoid run() {{ CHECK(value()) after(); }}\n"
    );
    for source in [source.clone(), source.replace('\n', "\r\n")] {
      let report = audit_recovery(Path::new("fixture.cc"), &source, &[]);
      assert!(!report.has_error, "{condition}: {report:?}");
      assert_eq!(report.eligible_names, ["CHECK"]);
      assert_eq!(&source[report.macro_spans[0].clone()], "CHECK(value())");
      for name in ["value", "after"] {
        let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
        assert_eq!(&source[span.clone()], format!("{name}()"));
      }
      assert!(!report.calls.iter().any(|(name, _)| name == "CHECK"));
    }
  }
}

#[test]
fn opaque_headers_cannot_supply_hidden_restore_macros_to_later_local_proof() {
  for boundary in [
    "#include \"unavailable.h\"",
    "#if defined(PLATFORM)\n#pragma push_macro(\"CHECK\")\n#endif",
    "#ifdef PLATFORM\n#unknown effect\n#endif",
  ] {
    let source = format!(
      "{boundary}\n#define CHECK(x) {{ effect(x); }}\nRESTORE();\nvoid run() {{ CHECK(value()) after(); }}\n"
    );
    let report = audit_recovery(Path::new("fixture.cc"), &source, &[]);
    assert!(report.has_error, "{report:?}");
    assert!(report.eligible_names.is_empty());
    assert!(report.macro_spans.is_empty());
  }
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
