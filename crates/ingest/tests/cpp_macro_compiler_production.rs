#![cfg(feature = "builtin-parser")]
#[path = "support/cpp_compiler_provider.rs"]
mod provider;
use std::fs;
use vorpal_ingest::{ExtractionEnv, OutlineExtractor, encode_product_into};

const SOURCE: &str = "#include <unknown-sdk.h>\nvoid run() { CHECK(value())\nafter(); }\n#undef CHECK\nvoid CHECK(int);\nvoid ordinary() { CHECK(other()); }\nvoid value() {}\nvoid after() {}\nvoid other() {}\n";

#[test]
fn native_case_loop_generators_keep_original_bodies_and_reject_wrong_contexts() {
  use vorpal_core::tree_sitter::LanguageExt;
  for newline in ["\n", "\r\n"] {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.cc");
    let source = "void run() { switch(0) { CHECK(42) /* body */ { body(); } break; } after(); }\n#undef CHECK\nvoid ordinary() { CHECK(other()); }\n".replace('\n', newline);
    fs::write(&path, &source).unwrap();
    fs::write(
      dir.path().join("proof.h"),
      "#define CHECK(x) case x: for(int i=0;i<2;++i)\n",
    )
    .unwrap();
    let extractor = OutlineExtractor::new()
      .unwrap()
      .with_cpp_macro_compiler(provider::command(dir.path(), &path))
      .unwrap();
    for mode in [
      "case-loop-origin",
      "case-loop-unclosed",
      "case-loop-extra",
      "case-loop",
    ] {
      fs::write(dir.path().join("mode"), mode).unwrap();
      let product = extractor
        .extract_product(path.to_str().unwrap(), &source)
        .unwrap();
      assert_eq!(
        product.error_nodes == 0,
        mode == "case-loop",
        "{mode}: {}",
        product.error_nodes
      );
    }
    assert_eq!(
      extractor.extraction_identity_for_path(path.to_str().unwrap()),
      None
    );
    let product = extractor
      .extract_product(path.to_str().unwrap(), &source)
      .unwrap();
    let mut encoded = Vec::new();
    encode_product_into(&product, &mut encoded);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded(
        path.to_str().unwrap(),
        &source,
        product.source_size,
        product.source_mtime_ns,
        &mut streamed,
      )
      .unwrap();
    assert_eq!(encoded, streamed);
    let raw =
      vorpal_lang_registry::SgLang::Builtin(vorpal_language::SupportLang::Cpp).grep(&source);
    let handed = extractor
      .extract_product_from_root(path.to_str().unwrap(), &raw)
      .unwrap();
    let mut handed_bytes = Vec::new();
    encode_product_into(&handed, &mut handed_bytes);
    assert_eq!(encoded, handed_bytes);
    assert!(product.refs.iter().any(|r| r.kind == 0 && r.name == "body"));
    assert!(
      product
        .refs
        .iter()
        .any(|r| r.kind == 0 && r.name == "after")
    );
    assert_eq!(
      product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == "CHECK")
        .count(),
      1
    );
    assert!(!product.items.iter().any(|i| i.entry.name == "proof"));
    for changed in [
      source.replace("body();", "body()"),
      source.replace("switch(0)", "if(true)"),
      source.replace("CHECK(42)", "if(true) CHECK(42)"),
      source.replace("CHECK(42)", "object.CHECK(42)"),
    ] {
      fs::write(&path, &changed).unwrap();
      assert!(
        extractor
          .extract_product(path.to_str().unwrap(), &changed)
          .unwrap()
          .error_nodes
          > 0,
        "{changed}"
      );
    }
  }
}

#[test]
fn native_complete_declaration_generators_keep_original_invocations_and_following_code() {
  use vorpal_core::tree_sitter::LanguageExt;
  for newline in ["\n", "\r\n"] {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.cc");
    let source = "namespace scope { CHECK(Type)\nvoid following() { after(); } }\n#undef CHECK\nvoid ordinary() { CHECK(other()); }\n".replace('\n', newline);
    fs::write(&path, &source).unwrap();
    fs::write(
      dir.path().join("proof.h"),
      "#define CHECK(x) template<> int storage<x>(); template<> int other<x>();\n",
    )
    .unwrap();
    let extractor = OutlineExtractor::new()
      .unwrap()
      .with_cpp_macro_compiler(provider::command(dir.path(), &path))
      .unwrap();
    for mode in [
      "declaration-list-origin",
      "declaration-list-unclosed",
      "declaration-list-expression",
      "declaration-list",
      "declaration-list-definitions",
    ] {
      fs::write(dir.path().join("mode"), mode).unwrap();
      let product = extractor
        .extract_product(path.to_str().unwrap(), &source)
        .unwrap();
      assert_eq!(
        product.error_nodes == 0,
        matches!(mode, "declaration-list" | "declaration-list-definitions"),
        "{mode}: {}",
        product.error_nodes
      );
      if product.error_nodes == 0 {
        assert!(
          !product
            .items
            .iter()
            .any(|i| ["storage", "other", "Type"].contains(&i.entry.name.as_ref()))
        );
        assert_eq!(
          product
            .refs
            .iter()
            .filter(|r| r.kind == 0 && r.name == "CHECK")
            .count(),
          1
        );
        for name in ["after", "other"] {
          let call = product
            .refs
            .iter()
            .find(|r| r.kind == 0 && r.name == name)
            .unwrap();
          assert_eq!(
            &source[call.start as usize..call.end as usize],
            format!("{name}()")
          );
        }
        let mut owned = Vec::new();
        encode_product_into(&product, &mut owned);
        let mut streamed = Vec::new();
        extractor
          .extract_product_encoded(
            path.to_str().unwrap(),
            &source,
            product.source_size,
            product.source_mtime_ns,
            &mut streamed,
          )
          .unwrap();
        assert_eq!(owned, streamed);
        let raw =
          vorpal_lang_registry::SgLang::Builtin(vorpal_language::SupportLang::Cpp).grep(&source);
        let mut handed = Vec::new();
        encode_product_into(
          &extractor
            .extract_product_from_root(path.to_str().unwrap(), &raw)
            .unwrap(),
          &mut handed,
        );
        assert_eq!(owned, handed);
        assert_eq!(
          extractor.extraction_identity_for_path(path.to_str().unwrap()),
          None
        );
      }
    }
    for damage in [
      source.replace("after();", "after()"),
      source.replace("namespace scope {", "void invalid() {"),
      source.replace("CHECK(Type)", "if (ready) CHECK(Type)"),
    ] {
      fs::write(&path, &damage).unwrap();
      assert!(
        extractor
          .extract_product(path.to_str().unwrap(), &damage)
          .unwrap()
          .error_nodes
          > 0,
        "{damage}"
      );
    }
    fs::write(&path, &source).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
  }
}

#[test]
fn nested_native_numeric_argument_macros_preserve_runtime_calls_and_original_semicolons() {
  for newline in ["\n", "\r\n"] {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.cc");
    let source = "void run() { CHECK(value() + BUTTON)\nafter(); }\n#undef CHECK\nvoid ordinary() { CHECK(other()); }\n".replace('\n', newline);
    fs::write(&path, &source).unwrap();
    let extractor = OutlineExtractor::new()
      .unwrap()
      .with_cpp_macro_compiler(provider::command(dir.path(), &path))
      .unwrap();
    let header = "#define CHECK(x) if(x) { consume(); }\n#define BUTTON 1\n";
    fs::write(dir.path().join("proof.h"), header).unwrap();
    for mode in [
      "literal-argument-anchor",
      "literal-argument-offset",
      "literal-argument-duplicate",
      "literal-argument",
    ] {
      fs::write(dir.path().join("mode"), mode).unwrap();
      let product = extractor
        .extract_product(path.to_str().unwrap(), &source)
        .unwrap();
      assert_eq!(
        product.error_nodes == 0,
        mode == "literal-argument",
        "{mode}"
      );
      if mode == "literal-argument" {
        for name in ["value", "after", "other"] {
          let call = product
            .refs
            .iter()
            .find(|r| r.kind == 0 && r.name == name)
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
            .any(|r| ["BUTTON", "consume"].contains(&r.name.as_str()) && r.kind == 0)
        );
      }
    }
    for replacement in ["sizeof", "(1)", "1 + 2"] {
      fs::write(
        dir.path().join("proof.h"),
        header.replace("BUTTON 1", &format!("BUTTON {replacement}")),
      )
      .unwrap();
      assert!(
        extractor
          .extract_product(path.to_str().unwrap(), &source)
          .unwrap()
          .error_nodes
          > 0
      );
    }
    fs::write(
      dir.path().join("proof.h"),
      "#define CHECK(x) do { consume(x); } while(0)\n#define BUTTON 1\n",
    )
    .unwrap();
    assert!(
      extractor
        .extract_product(path.to_str().unwrap(), &source)
        .unwrap()
        .error_nodes
        > 0
    );
    let terminated = source.replace("CHECK(value() + BUTTON)", "CHECK(value() + BUTTON);");
    fs::write(&path, &terminated).unwrap();
    assert_eq!(
      extractor
        .extract_product(path.to_str().unwrap(), &terminated)
        .unwrap()
        .error_nodes,
      0
    );
    fs::write(&path, &source).unwrap();
    fs::write(
      dir.path().join("proof.h"),
      "#define CHECK(x) { ignored(); }\n#define BUTTON 1\n",
    )
    .unwrap();
    let product = extractor
      .extract_product(path.to_str().unwrap(), &source)
      .unwrap();
    assert_eq!(product.error_nodes, 0);
    assert!(
      !product
        .refs
        .iter()
        .any(|r| r.kind == 0 && r.name == "value")
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
  }
}

#[test]
fn native_function_head_generators_retain_anonymous_original_body_boundaries() {
  for newline in ["\n", "\r\n"] {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.cc");
    let source = "CHECK(42) { body(); }\n#undef CHECK\nvoid CHECK(int x) { ordinary(); }\nvoid following() { after(); }\n".replace('\n', newline);
    fs::write(&path, &source).unwrap();
    fs::write(
      dir.path().join("proof.h"),
      "#define CHECK(x) void test_##x()\n",
    )
    .unwrap();
    let extractor = OutlineExtractor::new()
      .unwrap()
      .with_cpp_macro_compiler(provider::command(dir.path(), &path))
      .unwrap();
    for mode in [
      "function-prefix-origin",
      "function-prefix-unclosed",
      "function-prefix",
    ] {
      fs::write(dir.path().join("mode"), mode).unwrap();
      let product = extractor
        .extract_product(path.to_str().unwrap(), &source)
        .unwrap();
      assert_eq!(
        product.error_nodes == 0,
        mode == "function-prefix",
        "{mode}"
      );
      if mode == "function-prefix" {
        assert!(!product.items.iter().any(|i| i.entry.name == "test_42"));
        assert_eq!(
          product
            .items
            .iter()
            .filter(|i| i.entry.name == "CHECK")
            .count(),
          1
        );
        for name in ["body", "ordinary", "after"] {
          let call = product
            .refs
            .iter()
            .find(|r| r.kind == 0 && r.name == name)
            .unwrap();
          assert_eq!(
            &source[call.start as usize..call.end as usize],
            format!("{name}()")
          );
        }
      }
    }
    let product = extractor
      .extract_product(path.to_str().unwrap(), &source)
      .unwrap();
    let mut owned = Vec::new();
    encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded(
        path.to_str().unwrap(),
        &source,
        product.source_size,
        product.source_mtime_ns,
        &mut streamed,
      )
      .unwrap();
    assert_eq!(owned, streamed);
    use vorpal_core::tree_sitter::LanguageExt;
    let raw =
      vorpal_lang_registry::SgLang::Builtin(vorpal_language::SupportLang::Cpp).grep(&source);
    let handed = extractor
      .extract_product_from_root(path.to_str().unwrap(), &raw)
      .unwrap();
    let mut encoded = Vec::new();
    encode_product_into(&handed, &mut encoded);
    assert_eq!(owned, encoded);
    for damage in [
      source.replace("body();", "body()"),
      source.replace(
        "CHECK(42) { body(); }",
        "void invalid() { CHECK(42) { body(); } }",
      ),
    ] {
      fs::write(&path, &damage).unwrap();
      assert!(
        extractor
          .extract_product(path.to_str().unwrap(), &damage)
          .unwrap()
          .error_nodes
          > 0,
        "{damage}"
      );
    }
    let separated = source.replace("CHECK(42)", "CHECK(42);");
    fs::write(&path, &separated).unwrap();
    let product = extractor
      .extract_product(path.to_str().unwrap(), &separated)
      .unwrap();
    let raw = OutlineExtractor::new()
      .unwrap()
      .extract_product(path.to_str().unwrap(), &separated)
      .unwrap();
    assert_eq!(product.error_nodes, raw.error_nodes);
    fs::write(&path, &source).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
  }
}

#[test]
fn native_x_generators_use_exact_fresh_token_sites_in_direct_blocks() {
  for newline in ["\n", "\r\n"] {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.cc");
    let source = "void run() { CHECK(DISPATCH)\nafter(); }\n#undef CHECK\nvoid ordinary() { CHECK(other()); }\n".replace('\n', newline);
    fs::write(&path, &source).unwrap();
    fs::write(
      dir.path().join("proof.h"),
      "#define CHECK(X) X(First) X(Second)\n#define DISPATCH(T) if(ready) { consume<T>(); }\n",
    )
    .unwrap();
    let extractor = OutlineExtractor::new()
      .unwrap()
      .with_cpp_macro_compiler(provider::command(dir.path(), &path))
      .unwrap();
    for mode in [
      "x-generator-offset",
      "x-generator-origin",
      "x-generator-length",
      "x-generator-unclosed",
      "x-generator",
    ] {
      fs::write(dir.path().join("mode"), mode).unwrap();
      let product = extractor
        .extract_product(path.to_str().unwrap(), &source)
        .unwrap();
      assert_eq!(product.error_nodes == 0, mode == "x-generator", "{mode}");
      if mode == "x-generator" {
        assert!(
          !product
            .refs
            .iter()
            .any(|r| ["DISPATCH", "first", "second", "consume"].contains(&r.name.as_str()))
        );
        for name in ["after", "other"] {
          let call = product
            .refs
            .iter()
            .find(|r| r.kind == 0 && r.name == name)
            .unwrap();
          assert_eq!(
            &source[call.start as usize..call.end as usize],
            format!("{name}()")
          );
        }
        assert_eq!(
          product
            .refs
            .iter()
            .filter(|r| r.kind == 0 && r.name == "CHECK")
            .count(),
          1
        );
        let mut owned = Vec::new();
        encode_product_into(&product, &mut owned);
        let mut streamed = Vec::new();
        extractor
          .extract_product_encoded(
            path.to_str().unwrap(),
            &source,
            product.source_size,
            product.source_mtime_ns,
            &mut streamed,
          )
          .unwrap();
        assert_eq!(owned, streamed);
        use vorpal_core::tree_sitter::LanguageExt;
        let raw =
          vorpal_lang_registry::SgLang::Builtin(vorpal_language::SupportLang::Cpp).grep(&source);
        let handed = extractor
          .extract_product_from_root(path.to_str().unwrap(), &raw)
          .unwrap();
        let mut encoded = Vec::new();
        encode_product_into(&handed, &mut encoded);
        assert_eq!(owned, encoded);
      }
    }
    assert_eq!(
      extractor.extraction_identity_for_path(path.to_str().unwrap()),
      None
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    // A generated statement list is not one statement inside a control arm.
    let source = "void run() { if (ready) CHECK(DISPATCH)\nafter(); }\n";
    fs::write(&path, source).unwrap();
    let product = extractor
      .extract_product(path.to_str().unwrap(), source)
      .unwrap();
    assert!(product.error_nodes > 0);
    for invalid in [
      "void run() { CHECK(DISPATCH), after() }\n",
      "void run() { CHECK(DISPATCH)\nafter() }\n",
    ] {
      fs::write(&path, invalid).unwrap();
      let product = extractor
        .extract_product(path.to_str().unwrap(), invalid)
        .unwrap();
      assert!(product.error_nodes > 0);
    }
  }
}

#[test]
fn fresh_native_stringification_and_pasting_keep_original_names_and_arguments() {
  for newline in ["\n", "\r\n"] {
    for pasted in [false, true] {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("main.cc");
      let invocation = if pasted {
        "CHECK(value)"
      } else {
        "CHECK(value())"
      };
      let source = format!("// Grüße 日本語\nvoid run() {{ {invocation}\nafter(); }}\n#undef CHECK\nvoid CHECK(int); void ordinary() {{ CHECK(other()); }}\nvoid following() {{ later(); }}\n").replace('\n', newline);
      fs::write(&path, &source).unwrap();
      let header = if pasted {
        "#define CHECK(x) try { log(#x); test_##x(); } catch (...) {}\n"
      } else {
        "#define CHECK(x) { log(#x); sink(x); }\n"
      };
      fs::write(dir.path().join("proof.h"), header.replace('\n', newline)).unwrap();
      if pasted {
        fs::write(dir.path().join("mode"), "native-operators").unwrap();
      }
      let env = ExtractionEnv {
        cpp_macro_compiler: Some(provider::command(dir.path(), &path)),
        ..Default::default()
      };
      let product = env
        .extractor()
        .unwrap()
        .extract_product(path.to_str().unwrap(), &source)
        .unwrap();
      assert_eq!(product.error_nodes, 0, "pasted={pasted}");
      for name in ["after", "other", "later"] {
        let call = product
          .refs
          .iter()
          .find(|r| r.kind == 0 && r.name == name)
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
          .any(|r| ["log", "sink", "test_value"].contains(&r.name.as_str()))
      );
      assert_eq!(
        product
          .refs
          .iter()
          .filter(|r| r.kind == 0 && r.name == "value")
          .count(),
        usize::from(!pasted)
      );
      assert_eq!(
        product
          .refs
          .iter()
          .filter(|r| r.kind == 0 && r.name == "CHECK")
          .count(),
        1
      );
      assert!(product.items.iter().any(|n| n.entry.name == "following"));
      assert_eq!(fs::read_to_string(&path).unwrap(), source);
      assert!(
        OutlineExtractor::new()
          .unwrap()
          .extract_product(path.to_str().unwrap(), &source)
          .unwrap()
          .error_nodes
          > 0
      );
    }
  }
}

#[test]
fn stringification_alone_does_not_turn_argument_text_into_runtime_calls() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("main.cc");
  fs::write(&path, SOURCE).unwrap();
  fs::write(
    dir.path().join("proof.h"),
    "#define CHECK(x) { log(#x); }\n",
  )
  .unwrap();
  let product = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_compiler(provider::command(dir.path(), &path))
    .unwrap()
    .extract_product(path.to_str().unwrap(), SOURCE)
    .unwrap();
  assert_eq!(product.error_nodes, 0);
  assert!(
    !product
      .refs
      .iter()
      .any(|r| r.kind == 0 && r.name == "value")
  );
  assert!(
    product
      .refs
      .iter()
      .any(|r| r.kind == 0 && r.name == "after")
  );
  assert!(
    product
      .refs
      .iter()
      .any(|r| r.kind == 0 && r.name == "other")
  );
}

#[test]
fn native_operator_recovery_retains_missing_original_semicolons() {
  for (header, invocation) in [
    (
      "#define CHECK(x) do { log(#x); test_##x(); } while(false)\n",
      "CHECK(value)",
    ),
    (
      "#define CHECK(x) { log(#x); test_##x(); }\n",
      "CHECK(value)\n#undef CHECK\nCHECK(other())",
    ),
  ] {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.cc");
    let source = format!("void run() {{ {invocation}\nafter(); }}\n");
    fs::write(&path, &source).unwrap();
    fs::write(dir.path().join("proof.h"), header).unwrap();
    fs::write(dir.path().join("mode"), "native-operators").unwrap();
    let product = OutlineExtractor::new()
      .unwrap()
      .with_cpp_macro_compiler(provider::command(dir.path(), &path))
      .unwrap()
      .extract_product(path.to_str().unwrap(), &source)
      .unwrap();
    assert!(product.error_nodes > 0, "{header}");
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
  }
}

#[test]
fn fresh_compiler_products_keep_original_sites_and_never_authorize_replay() {
  for newline in ["\n", "\r\n"] {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.cc");
    let source = format!("// Grüße 日本語{newline}{}", SOURCE.replace('\n', newline));
    fs::write(&path, &source).unwrap();
    fs::write(
      dir.path().join("proof.h"),
      "#define CHECK(x) { sink(x); }\n",
    )
    .unwrap();
    let command = provider::command(dir.path(), &path);
    let env = ExtractionEnv {
      cpp_macro_compiler: Some(command),
      ..Default::default()
    };
    assert!(!env.is_default());
    let extractor = env.extractor().unwrap();
    let path = path.to_str().unwrap();
    assert_eq!(extractor.extraction_identity_for_path(path), None);
    let product = extractor.extract_product(path, &source).unwrap();
    assert_eq!(product.error_nodes, 0);
    for name in ["value", "after", "other"] {
      let call = product
        .refs
        .iter()
        .find(|r| r.kind == 0 && r.name == name)
        .unwrap();
      assert_eq!(
        &source[call.start as usize..call.end as usize],
        format!("{name}()")
      );
    }
    let checks: Vec<_> = product
      .refs
      .iter()
      .filter(|r| r.kind == 0 && r.name == "CHECK")
      .collect();
    assert_eq!(checks.len(), 1);
    assert_eq!(
      &source[checks[0].start as usize..checks[0].end as usize],
      "CHECK(other())"
    );
    assert!(!product.refs.iter().any(|r| r.name == "sink"));
    let mut owned = Vec::new();
    encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded(
        path,
        &source,
        product.source_size,
        product.source_mtime_ns,
        &mut streamed,
      )
      .unwrap();
    assert_eq!(owned, streamed);
    use vorpal_core::tree_sitter::LanguageExt;
    let raw =
      vorpal_lang_registry::SgLang::Builtin(vorpal_language::SupportLang::Cpp).grep(&source);
    let handed = extractor.extract_product_from_root(path, &raw).unwrap();
    let mut handed_bytes = Vec::new();
    encode_product_into(&handed, &mut handed_bytes);
    assert_eq!(owned, handed_bytes);
    assert_eq!(
      fs::read_to_string(dir.path().join("runs"))
        .unwrap()
        .lines()
        .count(),
      3
    );
    assert_eq!(fs::read_to_string(path).unwrap(), source);
  }
}

#[test]
fn malformed_native_mismatched_stale_or_failed_runs_decline_without_hiding_errors() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("main.cc");
  fs::write(&path, SOURCE).unwrap();
  fs::write(
    dir.path().join("proof.h"),
    "#define CHECK(x) { sink(x); }\n",
  )
  .unwrap();
  let mut command = provider::command(dir.path(), &path);
  command.timeout_seconds = 1;
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_compiler(command)
    .unwrap();
  for mode in [
    "stale",
    "source",
    "context",
    "after",
    "incomplete",
    "volatile",
    "directive",
    "anchor",
    "callee",
    "header",
    "json",
    "exit",
    "timeout",
    "volume",
  ] {
    fs::write(dir.path().join("mode"), mode).unwrap();
    let product = extractor
      .extract_product(path.to_str().unwrap(), SOURCE)
      .unwrap();
    assert!(product.error_nodes > 0, "{mode}");
  }
  fs::write(dir.path().join("mode"), "").unwrap();
  for changed in [false, true, false] {
    fs::write(
      dir.path().join("native-only.h"),
      if changed { "changed" } else { "stable" },
    )
    .unwrap();
    let product = extractor
      .extract_product(path.to_str().unwrap(), SOURCE)
      .unwrap();
    assert_eq!(product.error_nodes == 0, !changed);
  }
  fs::remove_file(dir.path().join("proof.h")).unwrap();
  assert!(
    extractor
      .extract_product(path.to_str().unwrap(), SOURCE)
      .unwrap()
      .error_nodes
      > 0
  );
  assert_eq!(fs::read_to_string(path).unwrap(), SOURCE);
}

#[test]
fn ignored_unevaluated_and_unsupported_macro_arguments_do_not_invent_runtime_calls() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("main.cc");
  let source = SOURCE.replace("CHECK(value())\n", "CHECK(value());\n");
  fs::write(&path, &source).unwrap();
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_compiler(provider::command(dir.path(), &path))
    .unwrap();
  for header in [
    "#define CHECK(x) {}\n",
    "#define CHECK(x) { (void)sizeof(x); }\n",
    "#define CHECK(x) #x\n",
  ] {
    fs::write(dir.path().join("proof.h"), header).unwrap();
    let product = extractor
      .extract_product(path.to_str().unwrap(), &source)
      .unwrap();
    assert!(
      !product
        .refs
        .iter()
        .any(|r| r.kind == 0 && r.name == "value"),
      "{header}"
    );
    assert!(
      product
        .refs
        .iter()
        .any(|r| r.kind == 0 && r.name == "after")
    );
    assert!(
      product
        .refs
        .iter()
        .any(|r| r.kind == 0 && r.name == "other")
    );
    assert_eq!(
      product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == "CHECK")
        .count(),
      1
    );
  }
}

#[test]
fn observed_definition_names_do_not_remove_ordinary_same_name_definitions_after_undef() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("main.cc");
  let source = format!(
    "{SOURCE}\nvoid DECL() {{ generated_body(); }}\n#undef DECL\nvoid DECL(int x) {{ ordinary_body(); }}\n"
  );
  fs::write(&path, &source).unwrap();
  fs::write(
    dir.path().join("proof.h"),
    "#define CHECK(x) { sink(x); }\n",
  )
  .unwrap();
  fs::write(dir.path().join("mode"), "definitions").unwrap();
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_compiler(provider::command(dir.path(), &path))
    .unwrap();
  let product = extractor
    .extract_product(path.to_str().unwrap(), &source)
    .unwrap();
  let definitions: Vec<_> = product
    .items
    .iter()
    .filter(|i| i.entry.name == "DECL")
    .collect();
  assert_eq!(definitions.len(), 1);
  assert_eq!(
    &source[definitions[0].entry.range.byte_offset.clone()],
    "void DECL(int x) { ordinary_body(); }"
  );
  assert!(!product.items.iter().any(|i| i.entry.name == "generated"));
}

#[test]
fn timed_out_provider_cannot_leave_preprocessing_descendants_running() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("main.cc");
  fs::write(&path, SOURCE).unwrap();
  fs::write(
    dir.path().join("proof.h"),
    "#define CHECK(x) { sink(x); }\n",
  )
  .unwrap();
  fs::write(dir.path().join("mode"), "descendant").unwrap();
  let mut command = provider::command(dir.path(), &path);
  command.timeout_seconds = 1;
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_compiler(command)
    .unwrap();
  assert!(
    extractor
      .extract_product(path.to_str().unwrap(), SOURCE)
      .unwrap()
      .error_nodes
      > 0
  );
  let pid: u32 = fs::read_to_string(dir.path().join("child.pid"))
    .unwrap()
    .parse()
    .unwrap();
  let running = || {
    #[cfg(windows)]
    unsafe {
      use windows_sys::Win32::{
        Foundation::{CloseHandle, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
      };
      let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
      if handle.is_null() {
        return false;
      }
      let mut status = 0;
      let active = GetExitCodeProcess(handle, &mut status) != 0 && status == STILL_ACTIVE as u32;
      CloseHandle(handle);
      active
    }
    #[cfg(unix)]
    {
      #[cfg(target_os = "linux")]
      if fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| s.split(") ").nth(1).is_some_and(|s| s.starts_with('Z')))
      {
        return false;
      }
      unsafe { libc::kill(pid as i32, 0) == 0 }
    }
  };
  let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
  while running() && std::time::Instant::now() < deadline {
    std::thread::sleep(std::time::Duration::from_millis(10));
  }
  assert!(
    !running(),
    "native preprocessing descendant survived timeout"
  );
}

#[test]
fn selected_compiler_failures_never_fall_back_to_metadata_but_other_files_can() {
  let dir = tempfile::tempdir().unwrap();
  let selected = dir.path().join("selected.cc");
  let other = dir.path().join("other.cc");
  let source = "#include \"proof.h\"\nvoid run() { CHECK(value())\nafter(); }\n";
  fs::write(&selected, source).unwrap();
  fs::write(&other, source).unwrap();
  fs::write(
    dir.path().join("proof.h"),
    "#define CHECK(x) { sink(x); }\n",
  )
  .unwrap();
  fs::write(dir.path().join("mode"), "exit").unwrap();
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_recovery(&[])
    .unwrap()
    .with_cpp_macro_compiler(provider::command(dir.path(), &selected))
    .unwrap();
  assert!(
    extractor
      .extract_product(selected.to_str().unwrap(), source)
      .unwrap()
      .error_nodes
      > 0
  );
  assert_eq!(
    extractor
      .extract_product(other.to_str().unwrap(), source)
      .unwrap()
      .error_nodes,
    0
  );
  assert_eq!(
    extractor.extraction_identity_for_path(selected.to_str().unwrap()),
    None
  );
  assert!(
    extractor
      .extraction_identity_for_path(other.to_str().unwrap())
      .is_some()
  );
}

#[test]
fn observed_type_name_expansions_are_not_published_as_original_named_types() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("main.cc");
  let source = format!(
    "{SOURCE}\nstruct TYPE {{ int member; }};\n#undef TYPE\nstruct TYPE {{ int ordinary; }};\n"
  );
  fs::write(&path, &source).unwrap();
  fs::write(
    dir.path().join("proof.h"),
    "#define CHECK(x) { sink(x); }\n",
  )
  .unwrap();
  fs::write(dir.path().join("mode"), "type-definitions").unwrap();
  let extractor = OutlineExtractor::new()
    .unwrap()
    .with_cpp_macro_compiler(provider::command(dir.path(), &path))
    .unwrap();
  let product = extractor
    .extract_product(path.to_str().unwrap(), &source)
    .unwrap();
  let types: Vec<_> = product
    .items
    .iter()
    .filter(|i| i.entry.name == "TYPE")
    .collect();
  assert_eq!(types.len(), 1);
  assert!(source[types[0].entry.range.byte_offset.clone()].contains("ordinary"));
}
