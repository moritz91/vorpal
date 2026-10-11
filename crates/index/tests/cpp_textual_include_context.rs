//! Genuine include context through the production index, spill and pinned queries.
use std::fs;
use vorpal_core::Language;
use vorpal_index::{CppTextualIncludeContext, ExtractionEnv, Kg, build_index_env};
use vorpal_kg::{EdgeType, NodeId};

fn encode(product: &vorpal_ingest::FileProduct) -> Vec<u8> {
  let mut bytes = Vec::new();
  vorpal_ingest::encode_product_into(product, &mut bytes);
  bytes
}

fn find(kg: &Kg, name: &str) -> NodeId {
  (0..kg.node_count())
    .map(|id| NodeId::new(id as u64))
    .find(|&id| kg.node(id).is_some_and(|node| node.name == name))
    .unwrap()
}

#[test]
fn production_products_preserve_foreign_owner_evidence_and_all_original_parts() {
  for newline in ["\n", "\r\n"] {
    let base = fs::canonicalize(std::env::temp_dir()).unwrap();
    let dir = tempfile::tempdir_in(base).unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    let root = src.join("root.cc");
    let includes: Vec<_> = ["prefix.cc", "head.cc", "tail.cc"]
      .into_iter()
      .map(|name| src.join(name))
      .collect();
    let write =
      |path: &std::path::Path, text: &str| fs::write(path, text.replace('\n', newline)).unwrap();
    write(
      &root,
      "static int before(int x) { return x; }\nstatic int after(int x) { return x; }\nnamespace Demo {\n#include \"prefix.cc\"\n#include \"head.cc\"\n#include \"tail.cc\"\n}\n",
    );
    write(&includes[0], "// α\ntemplate<class T>\n");
    write(&includes[1], "T read(T value) {\n  before(value);\n");
    write(
      &includes[2],
      "  after(value);\n  return value;\n}\nint next() { return read(1); }\n",
    );
    let env = ExtractionEnv {
      cpp_textual_include_contexts: vec![CppTextualIncludeContext {
        root: root.clone(),
        includes: includes.clone(),
      }],
      ..Default::default()
    };
    assert!(!env.is_default());
    let extractor = env.extractor().unwrap();
    for path in std::iter::once(&root).chain(&includes) {
      let source = fs::read_to_string(path).unwrap();
      let key = fs::canonicalize(path).unwrap();
      let key = key.to_str().unwrap();
      assert!(extractor.extraction_identity_for_path(key).is_none());
      let mut owned = extractor.extract_product(key, &source).unwrap();
      owned.source_size = source.len() as u64;
      owned.source_mtime_ns = 17;
      let mut encoded = Vec::new();
      extractor
        .extract_product_encoded(key, &source, owned.source_size, 17, &mut encoded)
        .unwrap();
      assert_eq!(encoded, encode(&owned));
      let lang = vorpal_lang_registry::SgLang::from_path(key).unwrap();
      use vorpal_core::tree_sitter::LanguageExt;
      let parsed = lang.grep(&source);
      let scanned = extractor.extract_product_from_root(key, &parsed).unwrap();
      let mut equivalent = owned.clone();
      equivalent.source_size = 0;
      equivalent.source_mtime_ns = 0;
      assert_eq!(encode(&scanned), encode(&equivalent));
    }
    let index = dir.path().join("index");
    let build = |output: &std::path::Path| {
      build_index_env(&src, output, Default::default(), Default::default(), &env).unwrap()
    };
    let report = build(&index);
    assert_eq!(report.error_nodes, 0);
    assert_eq!(report.indexed, 4);
    assert_eq!(
      build(&index).indexed,
      4,
      "common parses cannot replay stale physical products"
    );
    let generation = vorpal_kg::resolve_index_dir(&index);
    let kg = Kg::load(&generation).unwrap();
    let root_key = fs::canonicalize(&root).unwrap();
    let root_file = (0..kg.node_count())
      .map(|id| NodeId::new(id as u64))
      .find(|&id| {
        kg.node(id).is_some_and(|node| {
          node.kind == vorpal_kg::SymbolKind::File && node.path == root_key.to_str().unwrap()
        })
      })
      .unwrap();
    assert_eq!(
      kg.out_neighbors(root_file)
        .iter()
        .filter(|(_, edge)| edge.base() == EdgeType::IMPORTS)
        .count(),
      3,
      "original selected includes retain real import edges"
    );
    let read = find(&kg, "read");
    let after = find(&kg, "after");
    assert!(
      kg.out_neighbors(read)
        .iter()
        .any(|(id, edge)| *id == after && edge.base() == EdgeType::CALLS)
    );
    assert!(
      kg.out_neighbors(find(&kg, "next"))
        .iter()
        .any(|(id, edge)| *id == read && edge.base() == EdgeType::CALLS)
    );
    let context = kg.node(read).unwrap().source_context.unwrap();
    assert_eq!(context.parts.len(), 3);
    let occurrence = kg.edge_evidence(read, after).into_iter().next().unwrap();
    let origins = kg.evidence_paths(&occurrence);
    assert_eq!(origins.len(), 1);
    assert!(std::path::Path::new(origins[0]).ends_with("tail.cc"));
    let text =
      vorpal_index::explain_edge_on(&kg, Some(&generation), read.raw(), after.raw()).unwrap();
    assert!(
      text.contains("tail.cc") && text.contains("after(value)"),
      "{text}"
    );
    let target = vorpal_index::GraphTarget {
      id: Some(read.raw()),
      ..Default::default()
    };
    let vorpal_index::records::Selected::Hits(sites) =
      vorpal_index::records::related_records_with_sites(
        &kg,
        Some(&generation),
        "callees",
        &target,
        None,
      )
      .unwrap()
    else {
      panic!("call sites");
    };
    let site = sites.iter().find(|hit| hit.node.id == after.raw()).unwrap();
    assert!(std::path::Path::new(site.site_path.as_ref().unwrap()).ends_with("tail.cc"));
    assert!(site.site.as_ref().unwrap().contains("after(value)"));
    let vorpal_index::records::Selected::Hits(snippets) =
      vorpal_index::records::snippet_records(&kg, Some(&generation), &target, 0, 4096).unwrap()
    else {
      panic!("original snippet");
    };
    assert_eq!(snippets[0].parts.len(), 3);
    let parts = vorpal_index::records::definition_context_parts(context, usize::MAX).unwrap();
    let reconstructed: String = parts.iter().map(|part| part.body.as_str()).collect();
    assert!(reconstructed.starts_with("template<class T>"));
    assert!(reconstructed.ends_with('}'));
    assert!(reconstructed.contains("after(value)"));
    let changed = fs::read_to_string(&root)
      .unwrap()
      .replace("namespace Demo", "namespace Changed");
    fs::write(&root, changed).unwrap();
    assert!(
      matches!(
        vorpal_index::records::definition_context_parts(context, 1),
        Err(vorpal_index::records::SnippetError::Stale(_))
      ),
      "root dependency is validated even when output truncates before it"
    );
    assert!(extractor.check_cpp_context_freshness().is_err());
    let vorpal_index::records::Selected::Hits(stale_sites) =
      vorpal_index::records::related_records_with_sites(
        &kg,
        Some(&generation),
        "callees",
        &target,
        None,
      )
      .unwrap()
    else {
      panic!("stale call sites");
    };
    assert!(
      stale_sites.iter().all(|hit| hit.site.is_none()),
      "unchanged occurrence bytes cannot bypass changed root proof"
    );
    let modified = build(&index);
    assert!(!modified.reused && !modified.graph_reused);
    let scratch = dir.path().join("scratch");
    build(&scratch);
    assert_eq!(
      fs::read(index.join("CURRENT")).unwrap(),
      fs::read(scratch.join("CURRENT")).unwrap()
    );
    fs::remove_file(&includes[2]).unwrap();
    assert!(build_index_env(&src, &index, Default::default(), Default::default(), &env).is_err());
  }
}

#[test]
fn missing_or_ignored_inputs_and_overlapping_groups_cannot_hide_definitions() {
  let dir = tempfile::tempdir().unwrap();
  let src = fs::canonicalize(dir.path()).unwrap();
  let root = src.join("root.cc");
  let fragment = src.join("part.cc");
  fs::write(&root, "#include \"part.cc\"\n").unwrap();
  fs::write(&fragment, "int real() { return 1; }\n").unwrap();
  let group = CppTextualIncludeContext {
    root,
    includes: vec![fragment],
  };
  let mut env = ExtractionEnv {
    cpp_textual_include_contexts: vec![group.clone(), group],
    ..Default::default()
  };
  assert!(env.extractor().is_err());
  env.cpp_textual_include_contexts.pop();
  let extractor = env.extractor().unwrap();
  assert!(
    extractor
      .validate_cpp_context_manifest(std::iter::once(&src.join("root.cc")))
      .is_err()
  );
}

#[test]
fn disabling_context_configuration_retires_its_products_and_required_metadata() {
  let dir = tempfile::tempdir().unwrap();
  let src = dir.path().join("src");
  fs::create_dir(&src).unwrap();
  let root = src.join("root.cc");
  let head = src.join("head.cc");
  let tail = src.join("tail.cc");
  fs::write(&root, "#include \"head.cc\"\n#include \"tail.cc\"\n").unwrap();
  fs::write(&head, "int actual() {\n").unwrap();
  fs::write(&tail, "  return 1;\n}\n").unwrap();
  let env = ExtractionEnv {
    cpp_textual_include_contexts: vec![CppTextualIncludeContext {
      root,
      includes: vec![head, tail],
    }],
    ..Default::default()
  };
  let index = dir.path().join("index");
  build_index_env(&src, &index, Default::default(), Default::default(), &env).unwrap();
  let prior = vorpal_kg::resolve_index_dir(&index);
  assert!(prior.join("source_contexts.json").exists());
  let default = ExtractionEnv::default();
  let report = build_index_env(
    &src,
    &index,
    Default::default(),
    Default::default(),
    &default,
  )
  .unwrap();
  assert!(!report.graph_reused && !report.reused);
  let current = vorpal_kg::resolve_index_dir(&index);
  assert!(!current.join("source_contexts.json").exists());
  let kg = Kg::load(&current).unwrap();
  assert!((0..kg.node_count()).all(|id| {
    kg.node(NodeId::new(id as u64))
      .unwrap()
      .source_context
      .is_none()
  }));
  let scratch = dir.path().join("scratch");
  build_index_env(
    &src,
    &scratch,
    Default::default(),
    Default::default(),
    &default,
  )
  .unwrap();
  assert_eq!(
    fs::read(index.join("CURRENT")).unwrap(),
    fs::read(scratch.join("CURRENT")).unwrap()
  );
}
