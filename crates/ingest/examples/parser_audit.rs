//! Inspect parser errors and extracted definitions without writing an index.
use vorpal_core::{Language, tree_sitter::LanguageExt};
use vorpal_ingest::OutlineExtractor;
use vorpal_lang_registry::SgLang;

fn main() {
  let mut args = std::env::args().skip(1).peekable();
  if args.peek().is_some_and(|a| a == "--cpp-headers") {
    args.next();
    SgLang::register_globs(std::collections::HashMap::from([
      ("cpp".into(), vec!["*.h".into()]),
    ])).unwrap();
  }
  let extractor = OutlineExtractor::new().unwrap();
  for path in args {
    let source = std::fs::read_to_string(&path).unwrap();
    let lang = SgLang::from_path(&path).unwrap();
    let parsed = lang.grep(&source);
    let product = extractor.extract_product(&path, &source).unwrap();
    println!("{}: errors {}, damaged {}, items {}", path, product.error_nodes, product.error_bytes, product.items.len());
    for node in parsed.root().dfs().filter(|n| n.is_error() || n.is_missing()).take(25) {
      let range = node.range();
      let text = node.text();
      let preview: String = text.chars().take(140).collect();
      println!("  {} {:?} {:?}", if node.is_missing() { "MISSING" } else { "ERROR" }, range, preview);
    }
  }
}
