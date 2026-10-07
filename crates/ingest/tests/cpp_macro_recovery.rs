#![cfg(feature = "builtin-parser")]
use std::path::Path;
use vorpal_ingest::cpp_macro_recovery::audit_recovery;

#[test]
fn literal_diagnostic_pragmas_preserve_macro_arguments_and_following_functions() {
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  let lf = "#pragma pack(push, 1)\n#pragma warning(push, 1)\n#pragma warning(disable: 4100 4996)\n#define CHECK(x) { sink(x); }\nvoid run() { CHECK(value()) }\n#pragma warning(pop)\n#pragma pack(pop)\nvoid following() { after(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let report = audit_recovery(Path::new("pragmas.cc"), &source, &[]);
    assert!(!report.has_error, "{report:?}");
    assert_eq!(report.macro_spans.len(), 1);
    assert_eq!(&source[report.macro_spans[0].clone()], "CHECK(value())");
    assert!(report.functions.iter().any(|name| name == "following"));
    for name in ["value", "after"] {
      let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
      assert_eq!(&source[span.clone()], format!("{name}()"));
    }
    assert!(
      !report
        .calls
        .iter()
        .any(|(n, _)| matches!(n.as_str(), "CHECK" | "pack" | "warning"))
    );
    let product = extractor.extract_product("pragmas.cc", &source).unwrap();
    assert_eq!(product.error_nodes, 0);
    let mut owned = Vec::new();
    encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("pragmas.cc", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    assert!(
      OutlineExtractor::new()
        .unwrap()
        .extract_product("pragmas.cc", &source)
        .unwrap()
        .error_nodes
        > 0
    );
    let unsafe_source = source.replace("warning(push, 1)", "warning(push, LEVEL)");
    assert!(audit_recovery(Path::new("pragmas.cc"), &unsafe_source, &[]).has_error);
  }
}

#[test]
fn sdk_declaration_errors_do_not_mask_intact_include_metadata_or_header_errors() {
  use std::fs;
  use vorpal_core::Language;
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_language::LanguageExt;
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let root = std::env::temp_dir().join(format!(
    "vorpal-guarded-metadata-{}-{nonce}",
    std::process::id()
  ));
  fs::create_dir_all(&root).unwrap();
  let header = root.join("sdk.h");
  let safe = "#define SDK_BEGIN namespace sdk {\n#define SDK_END }\n#if defined(ENABLE)\nSDK_BEGIN\nextern const int variable;\nSDK_END\n#endif\n";
  let source =
    "#define CHECK(x) { sink(x); }\n#include \"sdk.h\"\nvoid run() { CHECK(value()) after(); }\n";
  let path = root.join("run.cc");
  fs::write(&path, source).unwrap();
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  for contents in [safe.to_owned(), safe.replace('\n', "\r\n")] {
    fs::write(&header, &contents).unwrap();
    let report = audit_recovery(&path, source, &[]);
    assert!(!report.has_error, "{report:?}");
    assert_eq!(report.macro_spans.len(), 1);
    for name in ["value", "after"] {
      let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
      assert_eq!(&source[span.clone()], format!("{name}()"));
    }
    let product = extractor
      .extract_product(path.to_str().unwrap(), source)
      .unwrap();
    assert_eq!(product.error_nodes, 0);
    let mut owned = Vec::new();
    encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded(path.to_str().unwrap(), source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let raw = vorpal_lang_registry::SgLang::from_path(&path)
      .unwrap()
      .grep(source);
    assert!(raw.root().has_error());
    let handoff = extractor
      .extract_product_from_root(path.to_str().unwrap(), &raw)
      .unwrap();
    let mut scanned = Vec::new();
    encode_product_into(&handoff, &mut scanned);
    assert_eq!(owned, scanned);
    // The original header's unsupported C++ namespace macros are still diagnosed.
    assert!(
      extractor
        .extract_product(header.to_str().unwrap(), &contents)
        .unwrap()
        .error_nodes
        > 0
    );
  }
  fs::write(&header, safe.replace("defined(ENABLE)", "EXPANDING")).unwrap();
  assert!(audit_recovery(&path, source, &[]).has_error);
  fs::write(&header, safe).unwrap();
  assert!(!audit_recovery(&path, source, &[]).has_error);
  fs::remove_dir_all(root).unwrap();
}

#[test]
fn statement_recovery_cannot_admit_namespace_or_linkage_compounds() {
  use vorpal_ingest::OutlineExtractor;
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  for enclosing in ["namespace scope", "extern \"C\""] {
    let lf = format!(
      "#define CHECK(x) {{ sink(x); }}\n{enclosing} {{ CHECK(value()) }}\nvoid after() {{ real(); }}\n"
    );
    for source in [lf.clone(), lf.replace('\n', "\r\n")] {
      let report = audit_recovery(Path::new("scope.cc"), &source, &[]);
      assert!(report.has_error, "{report:?}");
      assert!(report.eligible_names.is_empty(), "{report:?}");
      assert_eq!(report.context_errors.len(), 1, "{report:?}");
      assert_eq!(&source[report.context_errors[0].clone()], "CHECK(value())");
      assert!(report.functions.iter().any(|name| name == "after"));
      assert!(
        extractor
          .extract_product("scope.cc", &source)
          .unwrap()
          .error_nodes
          > 0
      );
    }
    let source = format!(
      "#define CHECK(x) {{ sink(x); }}\n{enclosing} {{ void run() {{ CHECK(value()) }} }}\n"
    );
    let report = audit_recovery(Path::new("scope.cc"), &source, &[]);
    assert!(
      !report.has_error,
      "a function body admits the statement: {report:?}"
    );
    assert_eq!(report.macro_spans.len(), 1);
    for delimiter in ["", ";"] {
      let source = format!(
        "#define CHECK(x) {{ sink(x); }}\n{enclosing} {{ CHECK(first()){delimiter} CHECK(second()){delimiter} }}\n"
      );
      let report = audit_recovery(Path::new("multiple-scopes.cc"), &source, &[]);
      assert!(report.has_error, "{report:?}");
      assert_eq!(report.context_errors.len(), 2, "{report:?}");
      assert!(report.eligible_names.is_empty());
    }
  }
  let source =
    "#define CHECK(x) { sink(x); }\nnamespace scope { auto run = []() { CHECK(value()) }; }\n";
  let report = audit_recovery(Path::new("lambda.cc"), source, &[]);
  assert!(
    !report.has_error,
    "lambda bodies admit statements: {report:?}"
  );
  assert_eq!(report.macro_spans.len(), 1);
}

#[test]
fn empty_preprocessing_arguments_are_proved_in_the_replacement() {
  use vorpal_core::Language;
  use vorpal_core::tree_sitter::LanguageExt;
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_lang_registry::SgLang;
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  for invocation in [
    "CHECK()",
    "CHECK(/* empty, argument */)",
    "CHECK( \t )",
    "CHECK(\x0b)",
    "CHECK(\x0c)",
  ] {
    let lf = format!("#define CHECK(x) {{ sink(x); }}\nvoid run() {{ {invocation} after(); }}\n");
    for source in [lf.clone(), lf.replace('\n', "\r\n")] {
      let report = audit_recovery(Path::new("empty.cc"), &source, &[]);
      assert!(!report.has_error, "{report:?}");
      assert_eq!(report.eligible_names, ["CHECK"]);
      assert_eq!(report.macro_spans.len(), 1);
      assert_eq!(&source[report.macro_spans[0].clone()], invocation);
      assert!(!report.calls.iter().any(|(name, _)| name == "CHECK"));
      let product = extractor.extract_product("empty.cc", &source).unwrap();
      assert_eq!(product.error_nodes, 0);
      let mut owned = Vec::new();
      encode_product_into(&product, &mut owned);
      let mut streamed = Vec::new();
      extractor
        .extract_product_encoded("empty.cc", &source, 0, 0, &mut streamed)
        .unwrap();
      assert_eq!(owned, streamed);
      let raw = SgLang::from_path("empty.cc").unwrap().grep(&source);
      let handoff = extractor
        .extract_product_from_root("empty.cc", &raw)
        .unwrap();
      let mut bank = Vec::new();
      encode_product_into(&handoff, &mut bank);
      assert_eq!(owned, bank);
    }
  }
  let source = "#define ZERO() { sink(); }\nvoid run() { ZERO() }\n";
  let report = audit_recovery(Path::new("zero.cc"), source, &[]);
  assert!(!report.has_error, "{report:?}");
  assert_eq!(report.eligible_names, ["ZERO"]);
}

#[test]
fn semicolons_cannot_hide_incompatible_replacement_syntax() {
  for (definition, invocation) in [
    ("#define DECLARE(x) { int x; }", "DECLARE(1 + 2)"),
    ("#define JUMP(x) { goto x; }", "JUMP(target())"),
    ("#define CHECK(x) if (x) { sink(); }", "CHECK()"),
  ] {
    let source = format!("{definition}\nvoid run() {{ {invocation}; after(); }}\n");
    let report = audit_recovery(Path::new("invalid-replacement.cc"), &source, &[]);
    assert!(report.has_error, "{report:?}");
    assert_eq!(report.context_errors.len(), 1, "{report:?}");
    assert_eq!(&source[report.context_errors[0].clone()], invocation);
    assert!(report.calls.iter().any(|(name, _)| name == "after"));
  }
}

#[test]
fn macro_arguments_must_fit_their_actual_replacement_context() {
  use vorpal_core::Language;
  use vorpal_core::tree_sitter::LanguageExt;
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_lang_registry::SgLang;
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  for (definition, invocation) in [
    ("#define DECLARE(name) { int name; }", "DECLARE(1 + 2)"),
    ("#define JUMP(label) { goto label; }", "JUMP(target())"),
    (
      "#define NESTED(x) { void local() { sink(x); } }",
      "NESTED(1)",
    ),
    (
      "#define METHOD(x) { struct Local { void local() { sink(x); } }; }",
      "METHOD(1)",
    ),
    ("#define DECLARE(\\u03B1) { int α; }", "DECLARE(1 + 2)"),
    ("#define DECLARE(x) { int \\u0078; }", "DECLARE(1 + 2)"),
    (
      "#define COPY(value) { __asm { mov eax,value } }",
      "COPY(target())",
    ),
    (
      "#define TEXT(value) { use(\"prefix\" value); }",
      "TEXT(unexpanded)",
    ),
  ] {
    let source = format!("{definition}\nvoid run() {{ {invocation} }}\n");
    for source in [source.clone(), source.replace('\n', "\r\n")] {
      let audit = audit_recovery(Path::new("roles.cc"), &source, &[]);
      assert!(
        audit.has_error,
        "replacement-context syntax must not become false-clean: {source}: {audit:?}"
      );
      assert!(audit.eligible_names.is_empty());
      let product = extractor.extract_product("roles.cc", &source).unwrap();
      assert!(product.error_nodes > 0);
      let mut owned = Vec::new();
      encode_product_into(&product, &mut owned);
      let mut streamed = Vec::new();
      extractor
        .extract_product_encoded("roles.cc", &source, 0, 0, &mut streamed)
        .unwrap();
      assert_eq!(owned, streamed);
      let raw = SgLang::from_path("roles.cc").unwrap().grep(&source);
      let handoff = extractor
        .extract_product_from_root("roles.cc", &raw)
        .unwrap();
      let mut bank = Vec::new();
      encode_product_into(&handoff, &mut bank);
      assert_eq!(owned, bank);
    }
  }
}

#[test]
fn replacement_proofs_preserve_tokens_literals_and_original_call_spans() {
  let lf = "#define DECLARE(name) { int name; }\n#define JUMP(label) { goto label; }\n#define COPY(value) { __asm { mov eax,value } }\n#define CHECK(value) { use(value, \"value\", R\"(value)\", 'v'); /* value */ }\nvoid run() { DECLARE(local) JUMP(done) COPY(12) CHECK(real()) after(); done: ; }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let audit = audit_recovery(Path::new("roles.cc"), &source, &[]);
    assert!(!audit.has_error, "{audit:?}");
    assert_eq!(audit.eligible_names, ["CHECK", "COPY", "DECLARE", "JUMP"]);
    for name in ["real", "after"] {
      let (_, span) = audit.calls.iter().find(|(n, _)| n == name).unwrap();
      assert_eq!(&source[span.clone()], format!("{name}()"));
    }
  }
  // Legacy MSVC coalesces adjacent operators; decline an ambiguous boundary.
  let source = "#define ADD(value) { use(1+value); }\nvoid run() { ADD(+real()) }";
  assert!(audit_recovery(Path::new("tokens.cc"), source, &[]).has_error);
  let source = "#define ADD(value) { use(1+ value); }\nvoid run() { ADD(+real()) }";
  assert!(!audit_recovery(Path::new("tokens.cc"), source, &[]).has_error);
  let source = "#define DECLARE(name) { int name; }\nvoid run() { DECLARE(valid) DECLARE(1+2) }";
  assert!(
    audit_recovery(Path::new("blocked.cc"), source, &[])
      .eligible_names
      .is_empty()
  );
}

#[test]
fn compound_lambda_replacements_keep_original_argument_spans() {
  let lf = "#define ACTION(x) { auto action = [&] { sink(x); }; action(); }\nint value(); void sink(int);\nvoid run() { ACTION(value()) after(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let report = audit_recovery(Path::new("lambda.cc"), &source, &[]);
    assert!(!report.has_error, "{report:?}");
    assert_eq!(report.eligible_names, ["ACTION"]);
    assert_eq!(&source[report.macro_spans[0].clone()], "ACTION(value())");
    for name in ["value", "after"] {
      let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
      assert_eq!(&source[span.clone()], format!("{name}()"));
    }
    assert!(!report.calls.iter().any(|(name, _)| name == "ACTION"));
  }
}

#[test]
fn noncanonical_macro_effect_names_decline_proof_but_unicode_literals_do_not() {
  for name in ["α", "\\u03B1", "$restore"] {
    let source = format!(
      "#define CHECK(x) {{ sink(x); }}\n#define {name} }}\nvoid run() {{ CHECK({name}) }}\n"
    );
    let report = audit_recovery(Path::new("unicode.cc"), &source, &[]);
    assert!(report.has_error, "{source}: {report:?}");
    assert!(report.eligible_names.is_empty());
  }
  let source = "#define CHECK(x) { sink(x, \"α\", R\"(\\u03B1)\"); /* α */ }\nint α();\nvoid run() { CHECK(α()) after(); }\n";
  let report = audit_recovery(Path::new("unicode.cc"), source, &[]);
  assert!(!report.has_error, "{report:?}");
  assert_eq!(report.eligible_names, ["CHECK"]);
  for name in ["α", "after"] {
    let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
    assert_eq!(&source[span.clone()], format!("{name}()"));
  }
}

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
  for condition in [
    "0",
    "1",
    "defined(A) && !defined(B)",
    "0x10 == 020",
    "(~0 & 3) != 0",
    "2 <= 3",
  ] {
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

#[test]
fn expanding_argument_and_keyword_macros_cannot_hide_real_syntax_errors() {
  for source in [
    "void effect(int);\n#define END }\n#define CHECK(x) { effect(x); }\nvoid run() { CHECK(END) after(); }\n",
    "void effect(int);\n#define END }\n#define CHECK(x) { effect(x); }\nvoid run() { CHECK(EN\\\nD) after(); }\n",
    "void effect(int);\n#define if(x) effect(x)\n#define CHECK(x) if(x) effect(x);\nvoid run() { CHECK(1) after(); }\n",
    "#define effect }\n#define CHECK(x) { effect(x); }\nvoid run() { CHECK(1) after(); }\n",
  ] {
    for newline in ["\n", "\r\n"] {
      let source = source.replace('\n', newline);
      let report = audit_recovery(Path::new("expanding.cc"), &source, &[]);
      assert!(report.has_error, "{report:?}");
      assert!(report.eligible_names.is_empty(), "{report:?}");
      assert!(report.macro_spans.is_empty());
    }
  }
}

#[test]
fn unused_macros_and_names_inside_literals_do_not_block_argument_recovery() {
  let source = "#define END }\n#define CHECK(x) { effect(x); }\nvoid run() { CHECK(\"END\") CHECK(value() /* END */) }\n";
  let report = audit_recovery(Path::new("literal.cc"), source, &[]);
  assert!(!report.has_error, "{report:?}");
  assert_eq!(report.eligible_names, ["CHECK"]);
  assert_eq!(report.macro_spans.len(), 2);
  assert!(
    report
      .calls
      .iter()
      .any(|(name, range)| name == "value" && &source[range.clone()] == "value()")
  );
}

#[test]
fn invocation_comments_keep_original_spans_and_match_production_paths() {
  use vorpal_core::Language;
  use vorpal_core::tree_sitter::LanguageExt;
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_lang_registry::SgLang;
  let lf = "#define CHECK(x) { effect(x); }\nvoid run() {\nCHECK /* α CHECK(other()) */ /***/ (value())\nCHECK // CHECK(other())\n \t\x0b\x0c(value())\nafter();\n}\nvoid following() { next(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let audit = audit_recovery(Path::new("comments.cc"), &source, &[]);
    assert!(!audit.has_error, "{audit:?}");
    assert_eq!(audit.eligible_names, ["CHECK"]);
    assert_eq!(audit.macro_spans.len(), 2);
    let recovered = vorpal_language::with_cpp_statement_macros(&audit.eligible_names, || {
      SgLang::from_path("comments.cc").unwrap().grep(&source)
    });
    assert_eq!(
      recovered
        .root()
        .dfs()
        .filter(|n| n.kind() == "comment")
        .count(),
      3
    );
    for node in recovered
      .root()
      .dfs()
      .filter(|n| n.kind() == "macro_statement")
    {
      let name = node.field("name").unwrap();
      assert_eq!(&source[name.range()], "CHECK");
      assert!(node.text().starts_with("CHECK "));
    }
    assert!(
      !audit
        .calls
        .iter()
        .any(|(name, _)| ["CHECK", "other"].contains(&name.as_str()))
    );
    for name in ["value", "after", "next"] {
      for (_, span) in audit.calls.iter().filter(|(n, _)| n == name) {
        assert_eq!(&source[span.clone()], format!("{name}()"));
      }
    }
    assert_eq!(audit.calls.iter().filter(|(n, _)| n == "value").count(), 2);
    let extractor = OutlineExtractor::new()
      .unwrap()
      .with_cpp_macro_recovery(&[])
      .unwrap();
    let product = extractor.extract_product("comments.cc", &source).unwrap();
    assert_eq!(product.error_nodes, 0);
    let mut owned = Vec::new();
    encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("comments.cc", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let raw = SgLang::from_path("comments.cc").unwrap().grep(&source);
    assert!(raw.root().has_error());
    let handoff = extractor
      .extract_product_from_root("comments.cc", &raw)
      .unwrap();
    let mut encoded = Vec::new();
    encode_product_into(&handoff, &mut encoded);
    assert_eq!(owned, encoded);
  }
}

#[test]
fn comment_and_control_whitespace_invocations_cannot_escape_proof_intervals() {
  for spacing in ["\x0b", " /* note */ ", " // note\n "] {
    let source = format!(
      "void before() {{ CHECK{spacing}(value()) }}\n#define CHECK(x) {{ effect(x); }}\nvoid run() {{ CHECK(value()) }}\n"
    );
    let audit = audit_recovery(Path::new("comments.cc"), &source, &[]);
    assert!(audit.has_error, "{spacing:?}: {audit:?}");
    assert!(audit.eligible_names.is_empty(), "{spacing:?}: {audit:?}");
    assert!(audit.macro_spans.is_empty());
  }
  for invocation in [
    "CHECK /* note */ (value(), second())",
    "CHECK // note\n (value(,))",
    "CHECK /* line\\\nsplice */ (value())",
    "CHECK // line\\\nsplice\n (value())",
    "CHECK /* trigraph??/ splice */ (value())",
    "CHECK // trigraph??/ splice\n (value())",
    "CHECK /* unterminated (value())",
    "CHECK /* note */ (value()) genuine()",
  ] {
    let source = format!("#define CHECK(x) {{ effect(x); }}\nvoid run() {{ {invocation} }}\n");
    let audit = audit_recovery(Path::new("comments.cc"), &source, &[]);
    assert!(audit.has_error, "{invocation:?}: {audit:?}");
  }
  assert!(
    audit_recovery(
      Path::new("ordinary.cc"),
      "void run() { CHECK /* note */ (value()) }",
      &[]
    )
    .has_error
  );
}

#[test]
fn unrelated_literal_macro_stack_metadata_keeps_recovery_and_spans() {
  let lf = "#define CHECK(x) { sink(x); }\n#pragma push_macro(\"OTHER\")\n#pragma pop_macro(\"OTHER\")\nvoid run() { CHECK(value()) after(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let report = audit_recovery(Path::new("stack.cc"), &source, &[]);
    assert!(!report.has_error, "{report:?}");
    assert_eq!(report.eligible_names, ["CHECK"]);
    for name in ["value", "after"] {
      let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
      assert_eq!(&source[span.clone()], format!("{name}()"));
    }
    assert!(!report.calls.iter().any(|(n, _)| n == "CHECK"));
  }
  for source in [
    "#define CHECK(x) { sink(x); }\n#pragma pop_macro(\"CHECK\")\nvoid run() { CHECK(value()) }",
    "#define CHECK(x) { sink(x); }\n#pragma pop_macro(\"value\")\nvoid run() { CHECK(value()) }",
    "#define CHECK(x) { value(x); }\n#pragma pop_macro(\"value\")\nvoid run() { CHECK(1) }",
    "#define CHECK(x) { sink(x); }\n#undef 123invalid\nvoid run() { CHECK(value()) }",
  ] {
    let report = audit_recovery(Path::new("stack.cc"), source, &[]);
    assert!(report.has_error, "{source}: {report:?}");
    assert!(report.eligible_names.is_empty());
  }
}

#[test]
fn unused_pragma_definitions_preserve_production_spans_without_executing_wrappers() {
  use vorpal_core::Language;
  use vorpal_core::tree_sitter::LanguageExt;
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_lang_registry::SgLang;
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  let definitions = "#define DIRECT() __pragma(pop_macro(\"CHECK\"))\n#define PORTABLE() _Pragma(\"pop_macro(\\\"CHECK\\\")\")\n#define WRAPPER() DIRECT()\n#define CHECK(x) { sink(x); }\n";
  let lf = format!("{definitions}void run() {{ CHECK(value()) after(); }}\n");
  for source in [lf.clone(), lf.replace('\n', "\r\n")] {
    let report = audit_recovery(Path::new("unused.cc"), &source, &[]);
    assert!(!report.has_error, "{report:?}");
    assert_eq!(report.eligible_names, ["CHECK"]);
    assert_eq!(&source[report.macro_spans[0].clone()], "CHECK(value())");
    for name in ["value", "after"] {
      let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
      assert_eq!(&source[span.clone()], format!("{name}()"));
    }
    assert!(!report.calls.iter().any(|(name, _)| name == "CHECK"));
    let product = extractor.extract_product("unused.cc", &source).unwrap();
    assert_eq!(product.error_nodes, 0);
    let mut owned = Vec::new();
    encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("unused.cc", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let raw = SgLang::from_path("unused.cc").unwrap().grep(&source);
    let handoff = extractor
      .extract_product_from_root("unused.cc", &raw)
      .unwrap();
    let mut bank = Vec::new();
    encode_product_into(&handoff, &mut bank);
    assert_eq!(owned, bank);
    for use_site in ["DIRECT();", "PORTABLE();", "WRAPPER();"] {
      let changed = source.replace("void run()", &format!("{use_site}\nvoid run()"));
      let report = audit_recovery(Path::new("used.cc"), &changed, &[]);
      assert!(report.has_error, "{report:?}");
      assert!(report.eligible_names.is_empty());
      assert!(
        extractor
          .extract_product("used.cc", &changed)
          .unwrap()
          .error_nodes
          > 0
      );
    }
  }
}

#[test]
fn statement_macro_expression_uses_are_diagnosed_without_inventing_macro_calls() {
  use vorpal_core::Language;
  use vorpal_core::tree_sitter::LanguageExt;
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_lang_registry::SgLang;
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  for expression in [
    "return CHECK(value());",
    "int x = CHECK(value());",
    "use(CHECK(value()));",
    "object.CHECK(value());",
    "::CHECK(value());",
    "if (CHECK(value())) after();",
    "for (; CHECK(value());) after();",
    "int array[CHECK(value())];",
    "throw CHECK(value());",
  ] {
    let lf = format!("#define CHECK(x) {{ sink(x); }}\nint run() {{ {expression} after(); }}\n");
    for source in [lf.clone(), lf.replace('\n', "\r\n")] {
      let report = audit_recovery(Path::new("context.cc"), &source, &[]);
      assert!(report.has_error, "{expression}: {report:?}");
      assert!(report.eligible_names.is_empty());
      assert_eq!(report.context_errors.len(), 1, "{expression}: {report:?}");
      assert!(source[report.context_errors[0].clone()].contains("CHECK(value())"));
      assert!(!report.calls.iter().any(|(name, _)| name == "CHECK"));
      for name in ["value", "after"] {
        let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
        assert_eq!(&source[span.clone()], format!("{name}()"));
      }
      let product = extractor.extract_product("context.cc", &source).unwrap();
      assert!(product.error_nodes > 0);
      assert!(!product.refs.iter().any(|r| r.name == "CHECK"));
      assert!(product.refs.iter().any(|r| r.name == "value"));
      let mut owned = Vec::new();
      encode_product_into(&product, &mut owned);
      let mut streamed = Vec::new();
      extractor
        .extract_product_encoded("context.cc", &source, 0, 0, &mut streamed)
        .unwrap();
      assert_eq!(owned, streamed);
      let raw = SgLang::from_path("context.cc").unwrap().grep(&source);
      let handoff = extractor
        .extract_product_from_root("context.cc", &raw)
        .unwrap();
      let mut bank = Vec::new();
      encode_product_into(&handoff, &mut bank);
      assert_eq!(owned, bank);
    }
  }
  let source = "#define CHECK(x) { sink(x); }\nvoid first() { CHECK(value()); }\n#undef CHECK\nint CHECK(int); int second() { return CHECK(value()); }\n";
  let report = audit_recovery(Path::new("interval.cc"), source, &[]);
  assert!(report.context_errors.is_empty(), "{report:?}");
  assert!(!report.has_error, "{report:?}");
  assert_eq!(
    report
      .calls
      .iter()
      .filter(|(name, _)| name == "CHECK")
      .count(),
    1,
    "the ordinary call after undef retains its span"
  );
}

#[test]
fn surrounding_macro_expansions_and_gnu_statement_expressions_decline_context_errors() {
  use vorpal_ingest::OutlineExtractor;
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  for (definitions, context) in [
    ("#define IDENTITY(x) x\n", "IDENTITY(CHECK(value()));"),
    ("#define return\n", "return CHECK(value());"),
    ("", "(CHECK(value()));"),
    ("", "sizeof(CHECK(value()));"),
  ] {
    let lf = format!(
      "#define CHECK(x) {{ sink(x); x; }}\n{definitions}void run() {{ {context} after(); }}\n"
    );
    for source in [lf.clone(), lf.replace('\n', "\r\n")] {
      let audit = audit_recovery(Path::new("valid-context.cc"), &source, &[]);
      assert!(!audit.has_error, "{context}: {audit:?}");
      assert!(audit.context_errors.is_empty(), "{context}: {audit:?}");
      assert!(!audit.calls.iter().any(|(name, _)| name == "CHECK"));
      for name in ["value", "after"] {
        let (_, span) = audit.calls.iter().find(|(n, _)| n == name).unwrap();
        assert_eq!(&source[span.clone()], format!("{name}()"));
      }
      let product = extractor
        .extract_product("valid-context.cc", &source)
        .unwrap();
      assert_eq!(product.error_nodes, 0, "{context}");
      assert!(!product.refs.iter().any(|r| r.name == "CHECK"));
    }
  }
}

#[test]
fn dangling_if_macros_retain_else_calls_and_nearest_if_binding() {
  use vorpal_core::Language;
  use vorpal_core::tree_sitter::LanguageExt;
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_lang_registry::SgLang;
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  for replacement in [
    "if (x) { sink(x); }",
    "if (x) {} else if (other()) { sink(x); }",
    "if (x) {} else while (other()) if (x) { sink(x); }",
  ] {
    for body in [
      "CHECK /* name */ (value()) /* boundary */ else after();",
      "if (outer()) CHECK(value()) else after();",
      "if (outer()) CHECK(value()) else after(); else fallback();",
    ] {
      let lf = format!(
        "#define CHECK(x) {replacement}\nvoid run() {{ {body} }}\nvoid following() {{ next(); }}\n"
      );
      for source in [lf.clone(), lf.replace('\n', "\r\n")] {
        let audit = audit_recovery(Path::new("open-if.cc"), &source, &[]);
        assert!(!audit.has_error, "{source}: {audit:?}");
        assert_eq!(audit.eligible_names, ["CHECK"]);
        for name in ["value", "after", "next"] {
          let (_, span) = audit.calls.iter().find(|(n, _)| n == name).unwrap();
          assert_eq!(&source[span.clone()], format!("{name}()"));
        }
        assert!(
          !audit
            .calls
            .iter()
            .any(|(n, _)| matches!(n.as_str(), "CHECK" | "other" | "sink"))
        );
        let product = extractor.extract_product("open-if.cc", &source).unwrap();
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
        let following = product
          .items
          .iter()
          .find(|n| n.entry.name == "following")
          .unwrap();
        assert_eq!(
          &source[following.entry.range.byte_offset.clone()],
          "void following() { next(); }"
        );
        let mut owned = Vec::new();
        encode_product_into(&product, &mut owned);
        let mut streamed = Vec::new();
        extractor
          .extract_product_encoded("open-if.cc", &source, 0, 0, &mut streamed)
          .unwrap();
        assert_eq!(owned, streamed);
        let raw = SgLang::from_path("open-if.cc").unwrap().grep(&source);
        let handoff = extractor
          .extract_product_from_root("open-if.cc", &raw)
          .unwrap();
        let mut scanned = Vec::new();
        encode_product_into(&handoff, &mut scanned);
        assert_eq!(owned, scanned);
        let names = ["CHECK".to_owned()];
        let parsed = vorpal_language::with_cpp_statement_macro_kinds(&names, &names, || {
          SgLang::from_path("open-if.cc").unwrap().grep(&source)
        });
        let statement = parsed
          .root()
          .dfs()
          .find(|n| n.kind().as_ref() == "macro_statement")
          .unwrap();
        assert_eq!(statement.field("name").unwrap().text(), "CHECK");
        assert_eq!(statement.field("arguments").unwrap().text(), "(value())");
        assert_eq!(
          statement.field("alternative").unwrap().text(),
          "else after();"
        );
        if body.starts_with("if (outer())") {
          let outer = statement.parent().unwrap();
          assert_eq!(outer.kind().as_ref(), "if_statement");
          assert_eq!(
            outer.field("alternative").is_some(),
            body.contains("fallback")
          );
        }
      }
    }
  }
}

#[test]
fn closed_macro_else_and_extra_semicolons_remain_original_span_errors() {
  use vorpal_ingest::OutlineExtractor;
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  for replacement in [
    "{ sink(x); }",
    "if (x) { sink(x); } else { sink(0); }",
    "try { sink(x); } catch (...) {}",
  ] {
    for body in [
      "CHECK(value()) else after();",
      "CHECK(value()); else after();",
      "if (outer()) CHECK(value()); else after();",
    ] {
      let lf = format!(
        "#define CHECK(x) {replacement}\nvoid run() {{ {body} }}\nvoid following() {{ next(); }}\n"
      );
      for source in [lf.clone(), lf.replace('\n', "\r\n")] {
        let report = audit_recovery(Path::new("invalid-else.cc"), &source, &[]);
        assert!(report.has_error, "{source}: {report:?}");
        assert!(
          report
            .context_errors
            .iter()
            .any(|span| source[span.clone()].starts_with("CHECK(value())")
              && source[span.clone()].ends_with("else")),
          "{report:?}"
        );
        assert!(!report.calls.iter().any(|(n, _)| n == "CHECK"));
        assert!(report.calls.iter().any(|(n, _)| n == "value"));
        assert!(report.calls.iter().any(|(n, _)| n == "next"));
        assert!(
          extractor
            .extract_product("invalid-else.cc", &source)
            .unwrap()
            .error_nodes
            > 0
        );
      }
    }
    let source = format!(
      "#define CHECK(x) {replacement}\nvoid run() {{ if (outer()) CHECK(value()) else after(); }}\n"
    );
    let report = audit_recovery(Path::new("outer-else.cc"), &source, &[]);
    assert!(!report.has_error, "{report:?}");
    assert!(report.calls.iter().any(|(n, _)| n == "after"));
  }
  for body in [
    "CHECK(value()); else after();",
    "if (outer()) CHECK(value()); else after();",
    "CHECK(value()) else",
    "CHECK(value()) else after()",
  ] {
    let source = format!("#define CHECK(x) if (x) {{ sink(x); }}\nvoid run() {{ {body} }}\n");
    assert!(
      audit_recovery(Path::new("broken-open-if.cc"), &source, &[]).has_error,
      "{source}"
    );
  }
  let mixed = "#define CHECK(x) if (x) { sink(x); }\nvoid first() { CHECK(value()) }\n#undef CHECK\n#define CHECK(x) { sink(x); }\nvoid second() { CHECK(value()) }\n";
  assert!(
    audit_recovery(Path::new("mixed.cc"), mixed, &[])
      .eligible_names
      .is_empty()
  );
  let ordinary = "void run() { CHECK(value()) else after(); }\n";
  assert!(audit_recovery(Path::new("ordinary.cc"), ordinary, &[]).has_error);
}

#[test]
fn typed_macro_scanner_context_is_nested_panic_safe_and_thread_local() {
  use vorpal_core::tree_sitter::LanguageExt;
  use vorpal_language::{SupportLang, with_cpp_statement_macro_kinds, with_cpp_statement_macros};
  let source = "void run() { CHECK(value()) else after(); }";
  let names = ["CHECK".to_owned()];
  let shape = || {
    let parsed = SupportLang::Cpp.grep(source);
    parsed
      .root()
      .dfs()
      .any(|n| n.kind().as_ref() == "macro_statement" && n.field("alternative").is_some())
  };
  assert!(!shape());
  with_cpp_statement_macro_kinds(&names, &names, || {
    assert!(shape());
    with_cpp_statement_macros(&names, || assert!(!shape()));
    assert!(shape());
    with_cpp_statement_macros(&[], || assert!(!shape()));
    let panic =
      std::panic::catch_unwind(|| with_cpp_statement_macros(&names, || panic!("fixture")));
    assert!(panic.is_err());
    assert!(shape());
    assert!(
      !std::thread::spawn(move || {
        let parsed = SupportLang::Cpp.grep(source);
        parsed
          .root()
          .dfs()
          .any(|n| n.kind().as_ref() == "macro_statement")
      })
      .join()
      .unwrap()
    );
  });
  assert!(!shape());
}

#[test]
fn opaque_macro_prefixes_decline_additional_else_context_diagnostics() {
  let source = "#define PREFIX() if (outer())\n#define CHECK(x) { sink(x); }\nvoid run() { PREFIX() CHECK(value()) else after(); }\n";
  let report = audit_recovery(Path::new("opaque-prefix.cc"), source, &[]);
  assert!(report.context_errors.is_empty(), "{report:?}");
  // The unsupported prefix remains outside recovery; no native-invalid claim.
  let source =
    "#define CHECK(x) { sink(x); }\nvoid run() { CHECK(first()) CHECK(value()) else after(); }\n";
  let report = audit_recovery(Path::new("adjacent.cc"), source, &[]);
  assert!(report.has_error);
  assert_eq!(report.context_errors.len(), 1, "{report:?}");
}

#[test]
fn conditional_function_bodies_retain_proven_macro_argument_calls() {
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap();
  let lf = "#define CHECK(x) { sink(x); }\n\n#if defined(WIDE)\nvoid wide() {\n#else\nvoid narrow() {\n#endif\nCHECK(value()) after();\n}\nvoid following() { final_call(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let report = audit_recovery(Path::new("conditional.cc"), &source, &[]);
    assert!(!report.has_error, "{report:?}");
    assert!(report.context_errors.is_empty());
    assert_eq!(report.macro_spans.len(), 1);
    assert_eq!(&source[report.macro_spans[0].clone()], "CHECK(value())");
    for name in ["wide", "narrow", "following"] {
      assert!(report.functions.iter().any(|n| n == name));
    }
    let product = extractor
      .extract_product("conditional.cc", &source)
      .unwrap();
    assert_eq!(product.error_nodes, 0);
    assert!(
      !product
        .refs
        .iter()
        .any(|r| r.kind == 0 && matches!(r.name.as_str(), "CHECK" | "sink"))
    );
    for name in ["value", "after"] {
      let calls: Vec<_> = product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == name)
        .collect();
      assert_eq!(calls.len(), 2, "{:?}", product.refs);
      for call in calls {
        assert_eq!(
          &source[call.start as usize..call.end as usize],
          format!("{name}()")
        );
      }
    }
    assert_eq!(
      product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == "final_call")
        .count(),
      1
    );
    let mut owned = Vec::new();
    encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("conditional.cc", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let orphan = source.replace("CHECK(value()) after();", "CHECK(value()) else after();");
    assert!(
      extractor
        .extract_product("conditional.cc", &orphan)
        .unwrap()
        .error_nodes
        > 0
    );
    for uncertain in [
      source.replace("defined(WIDE)", "FLAG"),
      source.replace(
        "CHECK(value()) after();",
        "#include \"missing.h\"\nCHECK(value()) after();",
      ),
      source.replace(
        "CHECK(value()) after();",
        "#undef CHECK\nCHECK(value()) after();",
      ),
    ] {
      let report = audit_recovery(Path::new("conditional.cc"), &uncertain, &[]);
      assert!(report.eligible_names.is_empty(), "{report:?}");
      assert!(report.has_error);
    }
    let unproven = source.replace("#define CHECK(x) { sink(x); }", "void CHECK(int);");
    assert!(
      extractor
        .extract_product("conditional.cc", &unproven)
        .unwrap()
        .error_nodes
        > 0
    );
    assert!(
      OutlineExtractor::new()
        .unwrap()
        .extract_product("conditional.cc", &source)
        .unwrap()
        .error_nodes
        > 0
    );
  }
}
