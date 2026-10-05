//! Read-only statement-macro audit. No recovered products enter index caches.
use std::path::{Path, PathBuf};

fn main() {
  let mut args = std::env::args().skip(1);
  let mut roots = Vec::new();
  let mut paths = Vec::new();
  let mut recover = false;
  let mut directives = false;
  while let Some(arg) = args.next() {
    if arg == "--recover" {
      recover = true;
    } else if arg == "--directives" {
      directives = true;
    } else if arg == "--include-root" {
      roots.push(PathBuf::from(
        args.next().expect("--include-root requires a path"),
      ));
    } else {
      paths.push(arg);
    }
  }
  assert!(!(recover && directives), "choose --recover or --directives");
  for path in paths {
    let source = std::fs::read_to_string(&path).unwrap();
    if directives {
      match vorpal_ingest::cpp_directive_audit::audit(&source) {
        Ok(spans) => {
          println!("{path}: {} directive spans", spans.len());
          for directive in spans {
            println!("  {:?}", directive.span);
          }
        }
        Err(uncertain) => println!("{path}: uncertain lexical boundary at {}", uncertain.offset),
      }
      continue;
    }
    if recover {
      let report =
        vorpal_ingest::cpp_macro_recovery::audit_recovery(Path::new(&path), &source, &roots);
      println!("{path}: {report:?}");
      continue;
    }
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
