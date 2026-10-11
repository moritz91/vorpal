#![cfg(feature = "builtin-parser")]
//! Real SDK/compiler controls, enabled only by the dedicated macOS CI job.
//! No generated protocol fixtures or Foundation declarations substitute for it.
use serde::Deserialize;
use std::{fs, path::PathBuf};
use vorpal_ingest::{OutlineExtractor, cpp_macro_compiler::CompilerCommand, encode_product_into};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
  name: String,
  path: PathBuf,
  probe_path: PathBuf,
  header_path: PathBuf,
  command: CompilerCommand,
}

fn require_macos() {
  #[cfg(not(target_os = "macos"))]
  panic!("Foundation controls require a real macOS host");
}

#[test]
fn genuine_foundation_context_preserves_aliases_spans_and_freshness() {
  let Some(manifest) = std::env::var_os("VORPAL_TEST_OBJC_FOUNDATION_CASES") else {
    eprintln!("not exercised: genuine macOS Foundation manifest is absent");
    return;
  };
  require_macos();
  let cases: Vec<Case> = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
  assert_eq!(cases.len(), 2);
  let mut identities = Vec::new();
  for case in cases {
    let extractor = OutlineExtractor::new()
      .unwrap()
      .with_cpp_macro_compiler(case.command)
      .unwrap();
    let source = fs::read_to_string(&case.path).unwrap();
    let path = case.path.to_str().unwrap();
    let product = extractor.extract_product(path, &source).unwrap();
    eprintln!(
      "{}: Catch2 {} diagnostics / {} damaged bytes",
      case.name, product.error_nodes, product.error_bytes
    );
    for span in [201224..201229, 203143..203151] {
      assert!(
        !product
          .error_spans
          .iter()
          .any(|&(start, end)| start < span.end as u32 && end > span.start as u32),
        "{}: physical ownership alias remains damaged: {:?}",
        case.name,
        span
      );
    }
    assert!(product.items.iter().any(|item| {
      item.entry.name == "m_substr"
        || item
          .members
          .iter()
          .any(|member| member.entry.name == "m_substr")
    }));
    assert!(!product.refs.iter().any(|r| r.kind == 0
      && matches!(
        r.name.as_str(),
        "CATCH_ARC_STRONG" | "CATCH_UNSAFE_UNRETAINED"
      )));
    identities.push(product.grammar_digest);
    let probe = fs::read_to_string(&case.probe_path).unwrap();
    let path = case.probe_path.to_str().unwrap();
    let clean = extractor.extract_product(path, &probe).unwrap();
    assert_eq!(clean.error_nodes, 0, "{}", case.name);
    assert!(
      clean
        .items
        .iter()
        .find(|i| i.entry.name == "Holder")
        .unwrap()
        .members
        .iter()
        .any(|i| i.entry.name == "stored")
    );
    for name in ["malloc", "authored_after", "CHECK", "HANDLE"] {
      let calls: Vec<_> = clean
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == name)
        .collect();
      assert_eq!(calls.len(), 1, "{name}");
      assert!(
        probe[calls[0].start as usize..calls[0].end as usize].starts_with(&format!("{name}("))
      );
    }
    let mut owned = Vec::new();
    encode_product_into(&clean, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded(
        path,
        &probe,
        clean.source_size,
        clean.source_mtime_ns,
        &mut streamed,
      )
      .unwrap();
    assert_eq!(owned, streamed);
    let header = fs::read(&case.header_path).unwrap();
    let invalid =
      b"#import <Foundation/Foundation.h>\n#define CHECK UNPROVEN\n#define HANDLE UNPROVEN\n";
    fs::write(&case.header_path, invalid).unwrap();
    let changed = extractor.extract_product(path, &probe).unwrap();
    assert!(changed.error_nodes > 0);
    assert_ne!(changed.grammar_digest, clean.grammar_digest);
    fs::remove_file(&case.header_path).unwrap();
    assert!(extractor.extract_product(path, &probe).unwrap().error_nodes > 0);
    fs::write(&case.header_path, header).unwrap();
    assert_eq!(
      extractor.extract_product(path, &probe).unwrap().error_nodes,
      0
    );
    let invalid = probe.replace("CHECK();", "CHECK()");
    fs::write(&case.probe_path, &invalid).unwrap();
    assert!(
      extractor
        .extract_product(path, &invalid)
        .unwrap()
        .error_nodes
        > 0
    );
    fs::write(&case.probe_path, probe).unwrap();
  }
  assert_ne!(
    identities[0], identities[1],
    "ARC and MRC cannot share a product context"
  );
}
