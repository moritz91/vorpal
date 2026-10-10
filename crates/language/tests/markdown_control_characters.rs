#![cfg(feature = "tree-sitter-md")]
use tree_sitter::Parser;
use vorpal_core::tree_sitter::{LanguageExt, StrDoc};
use vorpal_language::SupportLang;

#[test]
fn markdown_control_bytes_are_content_and_preserve_physical_spans() {
  for newline in ["\n", "\r\n"] {
    for content in [
      "diagram\0\0tail",
      "\u{1c}\0\0 branch\n\u{14}\0\0 leaf",
      "α\0ω",
      "\0",
    ] {
      let source =
        format!("# Before\n\n```\n{content}\n```\n\n## Following\n").replace('\n', newline);
      let parsed = SupportLang::Markdown.grep(&source);
      assert!(!parsed.root().has_error(), "{:?}", source);
      let fence = parsed
        .root()
        .dfs()
        .find(|n| n.kind().as_ref() == "code_fence_content")
        .unwrap();
      assert_eq!(
        &source[fence.range()],
        format!("{content}\n").replace('\n', newline)
      );
      assert!(
        parsed
          .root()
          .dfs()
          .any(|n| n.kind().as_ref() == "atx_heading" && n.text().contains("Following"))
      );
      assert_eq!(parsed.root().range(), 0..source.len());
    }
  }
  for source in [
    "# head\0tail\n",
    "plain\0tail\n",
    "    code\0tail\n",
    "> quote\0tail\n",
    "<div>html\0tail</div>\n",
    "nul\0",
  ] {
    assert!(
      !SupportLang::Markdown.grep(source).root().has_error(),
      "{:?}",
      source
    );
  }
}

#[test]
fn inline_nul_has_replacement_character_classification_without_rewriting() {
  let mut parser = Parser::new();
  parser
    .set_language(&tree_sitter_md::INLINE_LANGUAGE.into())
    .unwrap();
  for source in [
    "plain\0tail",
    "`code\0tail`",
    "**bold\0tail**",
    "[link\0text](path)",
    "<https://example.org/\0path>",
    "<b>html\0tail</b>",
    "\0",
  ] {
    let parsed = parser.parse(source, None).unwrap();
    assert!(
      !parsed.root_node().has_error(),
      "{:?}: {}",
      source,
      parsed.root_node().to_sexp()
    );
    assert_eq!(parsed.root_node().byte_range(), 0..source.len());
    assert_eq!(
      parsed.root_node().utf8_text(source.as_bytes()).unwrap(),
      source
    );
    let replacement = source.replace('\0', "\u{fffd}");
    let semantic = parser.parse(&replacement, None).unwrap();
    assert_eq!(
      parsed.root_node().to_sexp(),
      semantic.root_node().to_sexp(),
      "{:?}",
      source
    );
  }
}

#[test]
fn markdown_nul_edits_and_real_eof_match_fresh_trees() {
  let source = "# Before\n\n```\ndiagram\n```\n\n# After\n";
  let old = StrDoc::new(source, SupportLang::Markdown);
  for offset in [0, source.find("diagram").unwrap(), source.len()] {
    let changed = format!("{}\0{}", &source[..offset], &source[offset..]);
    let incremental =
      StrDoc::try_new_incremental(&changed, SupportLang::Markdown, Some((source, &old.tree)))
        .unwrap();
    let fresh = StrDoc::new(&changed, SupportLang::Markdown);
    assert!(!fresh.tree.root_node().has_error());
    assert_eq!(
      incremental.tree.root_node().to_sexp(),
      fresh.tree.root_node().to_sexp()
    );
    let removed = StrDoc::try_new_incremental(
      source,
      SupportLang::Markdown,
      Some((&changed, &incremental.tree)),
    )
    .unwrap();
    assert_eq!(
      removed.tree.root_node().to_sexp(),
      old.tree.root_node().to_sexp()
    );
  }
  for source in ["", "\0", "```\n\0", "\0\0"] {
    let parsed = SupportLang::Markdown.grep(source);
    assert!(!parsed.root().has_error(), "{:?}", source);
    assert_eq!(parsed.root().range(), 0..source.len());
  }
}
