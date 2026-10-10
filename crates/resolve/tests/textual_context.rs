//! A physical occurrence may bind to its logical owner only within the exact include proof.
use vorpal_resolve::{
  Confidence, Interner, NodeId, RefKind, RefSpillWriter, Reference, Resolver, Symbol, SymbolKind,
  SymbolTable, resolve_all,
};

fn symbol<'i>(itn: &'i Interner, id: u64, path: &str) -> Symbol<'i> {
  Symbol {
    id: NodeId::new(id),
    kind: SymbolKind::Function,
    path: itn.intern(path),
    exported: false,
    owner: None,
  }
}

#[test]
fn private_visibility_and_foreign_owner_require_the_same_proof() {
  let itn = Interner::new();
  for (root, identity, expected) in [
    ("root.cc", "proof", true),
    ("other.cc", "proof", false),
    ("root.cc", "changed", false),
  ] {
    let mut table = SymbolTable::new();
    table.insert_context(&itn, NodeId::new(91), 123, "root.cc", "proof"); // unreferenced owner
    table.insert(&itn, "secret", symbol(&itn, 7, "prefix.cc"));
    table.insert_context(&itn, NodeId::new(7), 456, root, identity);
    table.finalize();
    let reference = Reference::new(&itn, NodeId::new(0), "tail.cc", "secret", RefKind::Call)
      .with_evidence(12, 20)
      .with_source_context(Some(itn.reference_context_id(
        "root.cc",
        "proof",
        Some(123),
      )));
    let result = Resolver::new().resolve(&itn, &table, &reference, None);
    assert_eq!(result.target, expected.then_some(NodeId::new(7)));
    if expected {
      assert_eq!(result.confidence, Confidence::LOCAL);
    }
    let (edges, _) = resolve_all(&itn, &table, &[reference], &Resolver::new(), None);
    if expected {
      assert_eq!(edges[0].from, NodeId::new(91));
      assert_eq!(edges[0].span, (12, 20));
      assert_eq!(edges[0].from_path_bits, itn.intern("tail.cc").to_bits());
    } else {
      assert!(edges.is_empty());
    }
  }
}

#[test]
fn missing_conflicting_or_mixed_owner_never_falls_back_to_file_node() {
  let itn = Interner::new();
  for mode in 0..3 {
    let mut table = SymbolTable::new();
    table.insert(&itn, "target", symbol(&itn, 7, "head.cc"));
    table.insert_context(&itn, NodeId::new(7), 456, "root.cc", "proof");
    if mode > 0 {
      table.insert_context(
        &itn,
        NodeId::new(19),
        123,
        "root.cc",
        if mode == 1 { "changed" } else { "proof" },
      );
    }
    if mode == 2 {
      table.insert_context(&itn, NodeId::new(20), 123, "root.cc", "proof");
    }
    table.finalize();
    let reference =
      Reference::new(&itn, NodeId::new(0), "tail.cc", "target", RefKind::Call).with_source_context(
        Some(itn.reference_context_id("root.cc", "proof", Some(123))),
      );
    assert!(table.reference_owner(&itn, &reference).is_none());
    assert!(
      Resolver::new()
        .resolve(&itn, &table, &reference, None)
        .target
        .is_none()
    );
    let (edges, stats) = resolve_all(&itn, &table, &[reference], &Resolver::new(), None);
    assert!(edges.is_empty());
    assert_eq!(stats.masked, 1);
  }
}

#[test]
fn spill_preserves_session_context_and_original_physical_site() {
  let itn = Interner::new();
  let path = std::env::temp_dir().join(format!("vorpal-context-spill-{}", std::process::id()));
  let reference = Reference::new(&itn, NodeId::new(0), "tail.cc", "call", RefKind::Call)
    .with_evidence(5, 13)
    .with_source_context(Some(itn.reference_context_id(
      "root.cc",
      "proof",
      Some(u128::MAX),
    )));
  let mut writer = RefSpillWriter::create(&itn, &path).unwrap();
  writer.push(&reference).unwrap();
  let spill = writer.finish().unwrap();
  let restored = spill.chunks().unwrap().next().unwrap().unwrap();
  assert_eq!(restored, vec![reference]);
  assert_eq!(
    itn
      .reference_context(restored[0].source_context.unwrap())
      .owner_external,
    Some(u128::MAX)
  );
  spill.remove().unwrap();
}
