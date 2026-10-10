//! Inspect genuine textual include pieces. This never supplies index products.
use std::path::PathBuf;
use vorpal_core::Language;
use vorpal_core::tree_sitter::LanguageExt;
use vorpal_ingest::cpp_include_context::audit_context;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let mut args = std::env::args_os().skip(1).map(PathBuf::from);
  let root = args
    .next()
    .ok_or("usage: include_context_audit ROOT SELECTED_INCLUDE...")?;
  let selected: Vec<_> = args.collect();
  let context = audit_context(&root, &selected)?;
  let parsed = vorpal_language::with_cpp_statement_macros(&[], || {
    vorpal_lang_registry::SgLang::from_path("context.cc")
      .unwrap()
      .grep(&context.source)
  });
  println!(
    "{} files, {} original pieces, identity {}, current {}",
    context.files.len(),
    context.pieces.len(),
    context.identity(),
    context.is_current()
  );
  let mut errors = 0;
  for node in parsed.root().dfs() {
    if node.kind() == "ERROR" || node.is_missing() {
      errors += 1;
      println!(
        "diagnostic {:?}: {:?}",
        node.range(),
        context.physical_spans(node.range())
      );
    }
    if matches!(
      node.kind().as_ref(),
      "function_definition" | "template_declaration"
    ) {
      if let Some(spans) = context.physical_spans(node.range()) {
        if spans.len() > 1 {
          println!(
            "{} {:?}: {:?}",
            node.kind(),
            node.range(),
            spans
              .iter()
              .map(|span| (&context.files[span.file].path, &span.bytes))
              .collect::<Vec<_>>()
          );
        }
      }
    }
  }
  println!("{errors} diagnostics; syntax audit only, no compiler or MCP recovery claim");
  Ok(())
}
