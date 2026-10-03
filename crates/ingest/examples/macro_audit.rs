//! Read-only statement-macro evidence audit. This does not enable parser recovery.
use std::path::{Path, PathBuf};

fn main() {
  let mut args = std::env::args().skip(1);
  let mut roots = Vec::new();
  let mut paths = Vec::new();
  while let Some(arg) = args.next() {
    if arg == "--include-root" {
      roots.push(PathBuf::from(
        args.next().expect("--include-root requires a path"),
      ));
    } else {
      paths.push(arg);
    }
  }
  for path in paths {
    let source = std::fs::read_to_string(&path).unwrap();
    let evidence =
      vorpal_ingest::cpp_macro_evidence::audit_with_roots(Path::new(&path), &source, &roots);
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
      "{}: {} bindings, {} consulted includes, dependency identity {:016x}",
      path,
      evidence.bindings.len(),
      evidence.dependencies.len(),
      evidence.dependency_identity()
    );
  }
}
