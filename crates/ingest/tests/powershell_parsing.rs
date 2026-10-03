use vorpal_core::{Language, tree_sitter::LanguageExt};
use vorpal_ingest::OutlineExtractor;
use vorpal_lang_registry::SgLang;

#[test]
fn numeric_multipliers_preserve_literal_spans_and_following_functions() {
  let language = SgLang::from_path("sizes.ps1").unwrap();
  let extractor = OutlineExtractor::new().unwrap();
  for number in [
    "1MB", "2Kb", "3gB", "4TB", "5Pb", "0x10MB", "1.5MB", ".5KB", "1e2GB",
  ] {
    let lf = format!("$size = {number}\nfunction Following {{ Write-Output $size }}\n");
    for source in [lf.clone(), lf.replace('\n', "\r\n")] {
      let parsed = language.grep(&source);
      assert!(!parsed.root().has_error(), "{number}");
      assert!(parsed.root().dfs().any(|n| {
        matches!(
          n.kind().as_ref(),
          "decimal_integer_literal" | "hexadecimal_integer_literal" | "real_literal"
        ) && &source[n.range()] == number
      }));
      let product = extractor.extract_product("sizes.ps1", &source).unwrap();
      assert_eq!(product.error_nodes, 0, "{number}");
      assert!(product.items.iter().any(|i| i.entry.name == "Following"));
    }
  }
  for invalid in [
    "$size = (1MB",
    "$size = 1MB +",
    "function Broken { $size = 1MB",
  ] {
    assert!(language.grep(invalid).root().has_error(), "{invalid}");
  }
}
