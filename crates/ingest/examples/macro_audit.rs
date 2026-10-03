//! Read-only statement-macro evidence audit. This does not enable parser recovery.
use std::path::Path;

fn main() {
  for path in std::env::args().skip(1) {
    let source = std::fs::read_to_string(&path).unwrap();
    let evidence = vorpal_ingest::cpp_macro_evidence::audit(Path::new(&path), &source);
    for binding in &evidence.bindings {
      println!(
        "{}: {}({}) active {:?}, definition {}:{:?}",
        path,
        binding.definition.name,
        binding.definition.parameters,
        binding.active,
        binding.definition.definition_path.display(),
        binding.definition.definition_span
      );
    }
    println!(
      "{}: {} bindings, {} consulted includes",
      path,
      evidence.bindings.len(),
      evidence.dependencies.len()
    );
  }
}
