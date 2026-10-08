#![cfg(feature = "tree-sitter-cpp")]
use vorpal_language::{
  CppStatementMacroSite, LanguageExt, SupportLang, with_cpp_statement_macro_kinds,
  with_cpp_statement_macro_sites, with_cpp_statement_macros,
};

fn site(source: &str, needle: &str, open_if: bool) -> CppStatementMacroSite {
  CppStatementMacroSite {
    offset: source.find(needle).unwrap().try_into().unwrap(),
    name: "CHECK".to_owned(),
    open_if,
  }
}

#[test]
fn original_byte_sites_survive_bom_utf8_crlf_comments_and_unproven_calls() {
  for newline in ["\n", "\r\n"] {
    let source = "\u{feff}// Grüße 日本語\nvoid run() { CHECK /* spacing */ (first()) CHECK(second()); }\nvoid broken() { CHECK(third()) }".replace('\n', newline);
    let sites = [site(&source, "CHECK /*", false)];
    let parsed = with_cpp_statement_macro_sites(&sites, || SupportLang::Cpp.grep(&source));
    let root = parsed.root();
    let macros: Vec<_> = root
      .dfs()
      .filter(|n| n.kind().as_ref() == "macro_statement")
      .collect();
    assert_eq!(macros.len(), 1, "{}", root.text());
    assert_eq!(macros[0].range().start, sites[0].offset as usize);
    assert_eq!(macros[0].field("name").unwrap().text(), "CHECK");
    assert_eq!(macros[0].field("arguments").unwrap().text(), "(first())");
    assert!(
      root.has_error(),
      "the unproven missing semicolon must remain"
    );
    let call = root
      .dfs()
      .find(|n| n.kind().as_ref() == "call_expression" && n.text() == "CHECK(second())")
      .unwrap();
    assert_eq!(call.range().start, source.find("CHECK(second())").unwrap());
    assert!(
      root
        .dfs()
        .any(|n| n.kind().as_ref() == "call_expression" && n.text() == "first()")
    );
    assert!(
      SupportLang::Cpp
        .grep(&source)
        .root()
        .dfs()
        .all(|n| n.kind().as_ref() != "macro_statement")
    );
  }
}

#[test]
fn syntax_class_is_per_site_and_wrong_offsets_or_names_never_enable_recovery() {
  let source = "void run() { CHECK(first()) if (flag) CHECK(second()) else after(); }";
  let sites = [
    site(source, "CHECK(first())", false),
    site(source, "CHECK(second())", true),
  ];
  let parsed = with_cpp_statement_macro_sites(&sites, || SupportLang::Cpp.grep(source));
  assert!(!parsed.root().has_error());
  assert_eq!(
    parsed
      .root()
      .dfs()
      .filter(|n| n.kind().as_ref() == "macro_statement")
      .count(),
    2
  );
  let source = "void run() { CHECK(value()) }";
  for incorrect in [
    CppStatementMacroSite {
      offset: site(source, "CHECK", false).offset + 1,
      name: "CHECK".into(),
      open_if: false,
    },
    CppStatementMacroSite {
      offset: site(source, "CHECK", false).offset,
      name: "OTHER".into(),
      open_if: false,
    },
  ] {
    let parsed = with_cpp_statement_macro_sites(&[incorrect], || SupportLang::Cpp.grep(source));
    assert!(parsed.root().has_error());
    assert!(
      parsed
        .root()
        .dfs()
        .all(|n| n.kind().as_ref() != "macro_statement")
    );
  }
}

#[test]
fn contexts_restore_across_modes_threads_panics_and_parser_reuse() {
  let source = "void run() { CHECK(value()) }";
  std::thread::scope(|scope| {
    for _ in 0..4 {
      scope.spawn(|| {
        let mut parser = tree_sitter::Parser::new();
        parser
          .set_language(&SupportLang::Cpp.get_ts_language())
          .unwrap();
        let sites = [site(source, "CHECK", false)];
        with_cpp_statement_macro_sites(&sites, || {
          assert!(!parser.parse(source, None).unwrap().root_node().has_error());
          with_cpp_statement_macros(&[], || {
            assert!(parser.parse(source, None).unwrap().root_node().has_error())
          });
          with_cpp_statement_macro_kinds(&["OTHER".into()], &[], || {
            assert!(parser.parse(source, None).unwrap().root_node().has_error())
          });
          let panic =
            std::panic::catch_unwind(|| with_cpp_statement_macro_sites(&[], || panic!("fixture")));
          assert!(panic.is_err());
          assert!(!parser.parse(source, None).unwrap().root_node().has_error());
        });
        assert!(parser.parse(source, None).unwrap().root_node().has_error());
        with_cpp_statement_macros(&["CHECK".into()], || {
          with_cpp_statement_macro_sites(&[], || {
            assert!(parser.parse(source, None).unwrap().root_node().has_error())
          });
          assert!(!parser.parse(source, None).unwrap().root_node().has_error());
        });
      });
    }
  });
}

#[test]
fn included_range_offsets_are_absolute_original_bytes() {
  let prefix = "// é 日本語\r\nignored fragment\r\n";
  let source = format!("{prefix}void run() {{ CHECK(value()) }}");
  let start = prefix.len();
  let mut parser = tree_sitter::Parser::new();
  parser
    .set_language(&SupportLang::Cpp.get_ts_language())
    .unwrap();
  parser
    .set_included_ranges(&[tree_sitter::Range {
      start_byte: start,
      end_byte: source.len(),
      start_point: tree_sitter::Point::new(2, 0),
      end_point: tree_sitter::Point::new(2, source.len() - start),
    }])
    .unwrap();
  let sites = [site(&source, "CHECK", false)];
  let tree = with_cpp_statement_macro_sites(&sites, || parser.parse(&source, None).unwrap());
  assert!(!tree.root_node().has_error());
  assert!(tree.root_node().to_sexp().contains("macro_statement"));
  assert!(parser.parse(&source, None).unwrap().root_node().has_error());
}
