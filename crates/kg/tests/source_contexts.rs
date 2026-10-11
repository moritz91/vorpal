use std::borrow::Cow;
use std::path::Path;
use vorpal_kg::{FileBlock, Kg, KgWriter, NodeId, SegmentLayout};
use vorpal_outline::model::{
  DefinitionSourceContext, EntryRole, OutlineEntry, OutlineItem, PhysicalSourceSpan,
  SourcePosition, SourceRange, SymbolType,
};

fn range(start: usize, end: usize) -> SourceRange {
  SourceRange {
    byte_offset: start..end,
    start: SourcePosition {
      line: 0,
      column: start,
    },
    end: SourcePosition {
      line: 0,
      column: end,
    },
  }
}

fn items(name: &'static str, context: bool, end: usize) -> Vec<OutlineItem<'static>> {
  vec![OutlineItem {
    entry: OutlineEntry {
      role: EntryRole::Item,
      symbol_type: SymbolType::Function,
      name: Cow::Borrowed(name),
      range: range(2, end),
      signature: Cow::Borrowed("int f()"),
      ast_kind: Cow::Borrowed("function_definition"),
      source_context: context.then(|| {
        Box::new(DefinitionSourceContext {
          root: "root.cc".into(),
          identity: "a".repeat(64),
          references: Vec::new(),
          inputs: ["root.cc", "prefix.cc", "head.cc", "tail.cc"]
            .into_iter()
            .map(|path| vorpal_outline::model::SourceContextInput {
              path: path.into(),
              digest: 0,
            })
            .collect(),
          name: PhysicalSourceSpan {
            path: "head.cc".into(),
            range: range(3, 4),
          },
          parts: vec![
            PhysicalSourceSpan {
              path: "prefix.cc".into(),
              range: range(0, 3),
            },
            PhysicalSourceSpan {
              path: "head.cc".into(),
              range: range(2, end),
            },
            PhysicalSourceSpan {
              path: "tail.cc".into(),
              range: range(0, end),
            },
          ],
        })
      }),
    },
    is_import: false,
    is_exported: false,
    members: Vec::new(),
  }]
}

fn ingest(writer: &mut KgWriter, path: &str, items: &[OutlineItem<'_>]) -> FileBlock {
  writer.forget_identity_scope();
  let rows = writer.node_count() as u32;
  let heap = writer.heap_len();
  let edges = writer.edges_len() as u32;
  writer.ingest_file(path, items);
  FileBlock {
    rows: rows..writer.node_count() as u32,
    heap: heap..writer.heap_len(),
    edges: edges..writer.edges_len() as u32,
  }
}

fn graph(plain: bool) -> Kg {
  let mut writer = KgWriter::new();
  let mut files = vec![("head.cc", items("f", true, 8))];
  if plain {
    files.push(("a.cc", items("g", false, 8)));
  }
  let buckets = vorpal_kg::identity::bucket_count_for(files.len());
  files.sort_by_key(|(path, _)| (vorpal_kg::identity::bucket_of(path, buckets), *path));
  for (path, items) in files {
    ingest(&mut writer, path, &items);
  }
  writer.seal()
}

fn context_node(kg: &Kg) -> (NodeId, vorpal_kg::NodeView<'_>) {
  (0..kg.node_count())
    .find_map(|row| {
      let id = NodeId::new(row as u64);
      let node = kg.node(id)?;
      (node.name == "f").then_some((id, node))
    })
    .unwrap()
}

#[test]
fn original_parts_round_trip_flat_and_bucketed_with_required_metadata() {
  let kg = graph(true);
  let dir = tempfile::tempdir().unwrap();
  let flat = dir.path().join("flat");
  let bucketed = dir.path().join("bucketed");
  kg.save(&flat).unwrap();
  kg.save_with(
    &bucketed,
    &SegmentLayout::Bucketed {
      tree_root: "".into(),
      prior: None,
      live_files: 2,
    },
  )
  .unwrap();
  for path in [&flat, &bucketed] {
    let loaded = Kg::load(path).unwrap();
    let (_, original) = context_node(&kg);
    let (_, persisted) = context_node(&loaded);
    assert_eq!(persisted, original);
    assert_eq!(persisted.source_context.unwrap().parts.len(), 3);
    assert!(path.join("source_contexts.json").is_file());
    std::fs::remove_file(path.join("source_contexts.json")).unwrap();
    assert!(
      Kg::load(path).is_err(),
      "context metadata cannot be optional"
    );
  }
}

#[test]
fn absorption_and_husk_reuse_do_not_lose_or_leak_contexts() {
  let mut shard = KgWriter::new();
  shard.ingest_file("head.cc", &items("f", true, 8));
  let mut writer = KgWriter::new();
  writer.absorb(&mut shard);
  shard.reset_for_reuse();
  shard.ingest_file("other.cc", &items("g", false, 8));
  writer.absorb(&mut shard);
  let kg = writer.seal();
  assert!(context_node(&kg).1.source_context.is_some());
  for row in 0..kg.node_count() {
    let node = kg.node(NodeId::new(row as u64)).unwrap();
    if node.path == "other.cc" {
      assert!(node.source_context.is_none());
    }
  }
}

fn artifacts(path: &Path) -> Vec<(String, Vec<u8>)> {
  let mut result: Vec<_> = std::fs::read_dir(path)
    .unwrap()
    .map(|entry| {
      let entry = entry.unwrap();
      (
        entry.file_name().to_string_lossy().into_owned(),
        std::fs::read(entry.path()).unwrap(),
      )
    })
    .collect();
  result.sort_by(|a, b| a.0.cmp(&b.0));
  result
}

#[test]
fn tombstoned_contexts_and_shifted_dense_ids_match_scratch_exactly() {
  let mut retained = KgWriter::new();
  let _old = ingest(&mut retained, "head.cc", &items("f", true, 8));
  let plain = ingest(&mut retained, "a.cc", &items("g", false, 8));
  let current = ingest(&mut retained, "head.cc", &items("f", true, 12));
  let watermark = retained.edges_len();
  let (live, _) = retained.seal_canonical(&[plain, current], watermark);
  let mut scratch = KgWriter::new();
  ingest(&mut scratch, "a.cc", &items("g", false, 8));
  ingest(&mut scratch, "head.cc", &items("f", true, 12));
  let scratch = scratch.seal();
  assert_eq!(context_node(&live), context_node(&scratch));
  assert_eq!(
    context_node(&live).1.source_context.unwrap().parts[1]
      .range
      .byte_offset
      .end,
    12
  );
  let dir = tempfile::tempdir().unwrap();
  live.save(&dir.path().join("live")).unwrap();
  scratch.save(&dir.path().join("scratch")).unwrap();
  assert_eq!(
    artifacts(&dir.path().join("live")),
    artifacts(&dir.path().join("scratch"))
  );
}

#[test]
fn malformed_mixed_or_inconsistent_metadata_is_rejected() {
  let dir = tempfile::tempdir().unwrap();
  let first = dir.path().join("first");
  let second = dir.path().join("second");
  graph(false).save(&first).unwrap();
  graph(true).save(&second).unwrap();
  let original = std::fs::read(first.join("source_contexts.json")).unwrap();
  std::fs::copy(
    second.join("source_contexts.json"),
    first.join("source_contexts.json"),
  )
  .unwrap();
  assert!(Kg::load(&first).is_err());
  std::fs::write(first.join("source_contexts.json"), b"broken").unwrap();
  assert!(Kg::load(&first).is_err());
  let text = String::from_utf8(original).unwrap();
  // Valid JSON with identical node attributes and the original sidecar stamp
  // must still fail: every source/proof field is bound to the node fingerprint.
  for tampered in [
    text.replace(&"a".repeat(64), &"b".repeat(64)),
    text.replace("root.cc", "other.cc"),
  ] {
    std::fs::write(first.join("source_contexts.json"), tampered).unwrap();
    assert!(Kg::load(&first).is_err());
  }
  let text = text.replace("head.cc", "wrong.cc");
  std::fs::write(first.join("source_contexts.json"), text).unwrap();
  assert!(Kg::load(&first).is_err());
}

#[test]
fn ordinary_graphs_remain_without_context_artifacts() {
  let mut writer = KgWriter::new();
  writer.ingest_file("other.cc", &items("g", false, 8));
  let kg = writer.seal();
  let dir = tempfile::tempdir().unwrap();
  kg.save(dir.path()).unwrap();
  assert!(!dir.path().join("source_contexts.json").exists());
  assert!(Kg::load(dir.path()).is_ok());
}

#[test]
fn coincident_physical_occurrences_keep_all_real_paths_after_load() {
  let mut definitions = items("f", true, 8);
  let context = definitions[0].entry.source_context.as_mut().unwrap();
  for path in ["head.cc", "tail.cc"] {
    context
      .references
      .push(vorpal_outline::model::ContextReferenceSite {
        name_hash: 17,
        edge_type: vorpal_kg::EdgeType::CALLS.0,
        physical: PhysicalSourceSpan {
          path: path.into(),
          range: range(2, 5),
        },
      });
  }
  let mut writer = KgWriter::new();
  writer.ingest_file("head.cc", &definitions);
  let graph = writer.seal();
  let dir = tempfile::tempdir().unwrap();
  graph.save(dir.path()).unwrap();
  let graph = Kg::load(dir.path()).unwrap();
  let owner = context_node(&graph).0;
  let mut occurrence = vorpal_kg::EvidenceRow {
    from: owner.raw() as u32,
    to: u32::MAX,
    name_hash: 17,
    etype: vorpal_kg::EdgeType::CALLS.0,
    reason: 0,
    confidence: 0,
    outcome: vorpal_kg::EvidenceOutcome::External,
    candidates: 0,
    span_start: 2,
    span_end: 5,
    alternatives: vorpal_kg::AltSet::EMPTY,
  };
  assert_eq!(
    graph.evidence_paths(&occurrence),
    vec!["head.cc", "tail.cc"]
  );
  occurrence.name_hash = 18;
  assert!(
    graph.evidence_paths(&occurrence).is_empty(),
    "missing proof cannot guess owner path"
  );
}
