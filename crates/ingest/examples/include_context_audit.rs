//! Inspect genuine textual include pieces. This never supplies index products.
use std::path::PathBuf;
use vorpal_core::Language;
use vorpal_core::tree_sitter::LanguageExt;
use vorpal_ingest::cpp_include_context::audit_context;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let mut args = std::env::args_os().skip(1).map(PathBuf::from);
  let first = args
    .next()
    .ok_or("usage: include_context_audit [--project] ROOT SELECTED_INCLUDE...")?;
  let project = first == std::path::Path::new("--project");
  let root = if project { args.next().ok_or("missing root")? } else { first };
  let selected: Vec<_> = args.collect();
  if project {
    let report = vorpal_ingest::OutlineExtractor::new()?.audit_include_projection(&root, &selected)?;
    println!("{} definitions, {} references, {} diagnostics; identity {}", report.definitions.len(), report.references.len(), report.diagnostics.len(), report.identity);
    for definition in &report.definitions {
      let context = definition.entry.source_context.as_ref().unwrap();
      if context.parts.len() > 1 {
        println!("{} owner {:?}, name {:?}, parts {:?}", definition.entry.name,
          definition.parent_entity_index, context.name, context.parts);
      }
    }
    let mut foreign_calls = 0;
    for reference in &report.references {
      if let Some(owner) = report.definitions.iter().find(|definition| definition.entity_index == reference.owner_entity_index) {
        if owner.entry.source_context.as_ref().unwrap().name.path != reference.physical.path {
          foreign_calls += 1;
          println!("cross-file reference {} in {:?}, owner {}", reference.reference.name, reference.physical, owner.entry.name);
        }
      }
    }
    println!("{foreign_calls} references with foreign physical owners; projection audit, no production handoff");
    return Ok(());
  }
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
