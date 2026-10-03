use std::fs;
use vorpal_kg::{EdgeType, Kg, NodeId};

#[test]
fn cpp_calls_select_the_body_of_the_typed_receiver() {
  let base = std::env::temp_dir().join(format!("vorpal-cpp-members-{}", std::process::id()));
  let src = base.join("src");
  fs::create_dir_all(&src).unwrap();
  fs::write(src.join("cache.hpp"), "class Cache { int hidden(); public: int CreateScope(); protected: int guarded(); private: int secret(); };\nclass Other { public: int CreateScope(); };\nstruct DefaultPublic { int ping(); private: int hidden(); };\n").unwrap();
  fs::write(src.join("cache.cc"), "#include \"cache.hpp\"\nint Cache::CreateScope() { return 1; }\nint Other::CreateScope() { return 2; }\n").unwrap();
  fs::write(src.join("use.cc"), "#include \"cache.hpp\"\nCache* GetMeshCache();\nint begin(Cache* cache) { return cache->CreateScope(); }\nint other(Other& value) { return value.CreateScope(); }\nint unknown(auto* value) { return value->CreateScope(); }\nint factory(bool ready) { auto* cache = ready ? GetMeshCache() : nullptr; return cache->CreateScope(); }\nint assigned(Cache* input) { stored = input; return stored->CreateScope(); }\n").unwrap();
  let index = base.join("index");
  vorpal_index::build_index(&src, &index).unwrap();
  let kg = Kg::load(&index).unwrap();
  let id = |name: &str| -> NodeId {
    (0..kg.node_count() as u64).map(NodeId::new).find(|&id| kg.node(id).is_some_and(|n| n.name == name)).unwrap_or_else(|| panic!("missing {name}"))
  };
  let has_call = |from: &str, to: &str| kg.all_evidence().iter().any(|e| e.from as u64 == id(from).raw() && e.to as u64 == id(to).raw() && EdgeType(e.etype).base() == EdgeType::CALLS && e.outcome == vorpal_kg::EvidenceOutcome::Edge);
  assert!(has_call("begin", "Cache::CreateScope"));
  assert!(has_call("other", "Other::CreateScope"));
  assert!(has_call("factory", "Cache::CreateScope"));
  assert!(has_call("assigned", "Cache::CreateScope"));
  assert!(!has_call("begin", "Other::CreateScope"));
  assert!(!has_call("unknown", "Cache::CreateScope"));
  assert!(!has_call("unknown", "Other::CreateScope"));
  for (name, public) in [("CreateScope", true), ("guarded", false), ("secret", false), ("ping", true)] {
    assert_eq!(kg.node(id(name)).unwrap().exported, public, "{name}");
  }
  // The incremental resolver must retain the same body aliases as a full build.
  let generation = vorpal_kg::resolve_index_dir(&index);
  let map = vorpal_kg::NodeIdMap::from_dir(&generation).unwrap();
  let root = src.canonicalize().unwrap().to_string_lossy().into_owned();
  let path = src.join("use.cc").canonicalize().unwrap().to_string_lossy().into_owned();
  let pack = vorpal_ingest::PackReader::open_rooted(&generation, Some(&root)).unwrap();
  let product = vorpal_ingest::OutlineExtractor::new().unwrap().extract_product(&path, &fs::read_to_string(&path).unwrap()).unwrap();
  let mut bytes = Vec::new();
  vorpal_ingest::encode_product_into(&product, &mut bytes);
  let view = vorpal_ingest::decode_product_view(&bytes).unwrap();
  let interner = vorpal_ingest::Interner::default();
  let mut scratch = vorpal_ingest::Ingestor::new(&interner, vorpal_ingest::OutlineExtractor::new().unwrap());
  let ords = scratch.ingest_product_mapped(&path, product);
  let key = vorpal_kg::identity::FileKey::of(vorpal_kg::identity::tree_relative(&path, &root)).0;
  let reach = vorpal_ingest::ReachGraph::decode(&fs::read(generation.join(vorpal_ingest::REACH_GRAPH_FILE)).unwrap()).unwrap();
  let fetch = |path: &str| pack.get(path).map(<[u8]>::to_vec);
  let outcome = vorpal_ingest::scoped_resolve_file(&interner, &kg, &map, &vorpal_ingest::Resolver::new(), &fetch, &path, key, &view, &ords, usize::MAX, Some(&reach)).unwrap();
  let (_, start, rows) = map.files().iter().find(|&&(file, _, _)| file == key).copied().unwrap();
  let calls = |evidence: &[vorpal_kg::EvidenceRow]| {
    let mut calls: Vec<_> = evidence.iter().filter(|e| e.from as u64 >= start && (e.from as u64) < start + u64::from(rows) && e.outcome == vorpal_kg::EvidenceOutcome::Edge && EdgeType(e.etype).base() == EdgeType::CALLS).map(|e| (e.from, e.to, e.confidence, e.reason, e.candidates)).collect();
    calls.sort_unstable();
    calls
  };
  assert_eq!(calls(&outcome.evidence), calls(&kg.all_evidence()));
  drop(kg);
  // The receiver type alone cannot distinguish overloaded bodies.
  fs::write(src.join("over.hpp"), "class Over { public: int open(int); int open(double); };\n").unwrap();
  fs::write(src.join("over.cc"), "#include \"over.hpp\"\nint Over::open(int n) { return n; }\nint Over::open(double n) { return 0; }\nint overloaded(Over* value) { return value->open(1); }\n").unwrap();
  let overloaded_index = base.join("over-index");
  vorpal_index::build_index(&src, &overloaded_index).unwrap();
  let overloaded_kg = Kg::load(&overloaded_index).unwrap();
  let caller = (0..overloaded_kg.node_count() as u64).map(NodeId::new).find(|&id| overloaded_kg.node(id).is_some_and(|n| n.name == "overloaded")).unwrap();
  assert!(!overloaded_kg.all_evidence().iter().any(|e| e.from as u64 == caller.raw() && e.outcome == vorpal_kg::EvidenceOutcome::Edge && EdgeType(e.etype).base() == EdgeType::CALLS));
  fs::remove_dir_all(base).unwrap();
}

#[test]
fn cpp_inline_sdk_methods_are_callable_graph_definitions() {
  let base = std::env::temp_dir().join(format!("vorpal-cpp-inline-sdk-{}", std::process::id()));
  let src = base.join("src");
  fs::create_dir_all(&src).unwrap();
  fs::write(src.join("sdk.cc"), "int support(int);\nclass SDK { public: long SDKCALL Draw(int value) { return support(value); } };\nint invoke(SDK& object) { return object.Draw(1); }\n").unwrap();
  let index = base.join("index");
  let report = vorpal_index::build_index(&src, &index).unwrap();
  assert_eq!(report.error_files, 0);
  let kg = Kg::load(&index).unwrap();
  let id = |name: &str| -> NodeId {
    (0..kg.node_count() as u64)
      .map(NodeId::new)
      .find(|&id| kg.node(id).is_some_and(|n| n.name == name))
      .unwrap_or_else(|| panic!("missing {name}"))
  };
  let caller = id("invoke");
  let method = id("Draw");
  assert!(
    kg.all_evidence()
      .iter()
      .any(|e| e.from as u64 == caller.raw()
        && e.to as u64 == method.raw()
        && EdgeType(e.etype).base() == EdgeType::CALLS
        && e.outcome == vorpal_kg::EvidenceOutcome::Edge)
  );
  drop(kg);
  fs::remove_dir_all(base).unwrap();
}
