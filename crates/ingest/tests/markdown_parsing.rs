#![cfg(feature = "builtin-parser")]
use vorpal_core::tree_sitter::LanguageExt;
use vorpal_ingest::{OutlineExtractor, encode_product_into};

#[test]
fn nul_diagrams_keep_headings_physical_content_and_extraction_parity() {
  for newline in ["\n", "\r\n"] {
    let source = "# Before\n\n```\n\u{1c}\0\0 α branch\n\u{14}\0\0 ω leaf\n```\n\n## Following\n"
      .replace('\n', newline);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("diagram.md");
    std::fs::write(&path, &source).unwrap();
    let path = path.to_str().unwrap();
    let extractor = OutlineExtractor::new().unwrap();
    let product = extractor.extract_product(path, &source).unwrap();
    assert_eq!(product.error_nodes, 0);
    assert_eq!(std::fs::read(path).unwrap(), source.as_bytes());
    let following = product
      .items
      .iter()
      .flat_map(|i| i.members.iter())
      .find(|i| i.entry.name == "Following")
      .unwrap();
    assert!(source[following.entry.range.byte_offset.clone()].starts_with("## Following"));
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
    let raw =
      vorpal_lang_registry::SgLang::Builtin(vorpal_language::SupportLang::Markdown).grep(&source);
    let handed = extractor
      .extract_product_from_root(path, &raw)
      .unwrap();
    let mut encoded = Vec::new();
    encode_product_into(&handed, &mut encoded);
    assert_eq!(owned, encoded);
  }
}
