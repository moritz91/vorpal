use std::fs;
use vorpal_index::{GraphTarget, SearchFilter};
use vorpal_kg::{Kg, SymbolSelector};

#[test]
fn graph_suffix_selectors_accept_both_separator_spellings() {
  let base = std::env::temp_dir().join(format!("vorpal-suffix-{}", std::process::id()));
  let source = base.join("src");
  fs::create_dir_all(source.join("nested")).unwrap();
  fs::write(source.join("nested/target.rs"), "pub fn target() {}\n").unwrap();
  fs::write(source.join("caller.rs"), "fn caller() { target(); }\n").unwrap();
  let index = base.join("index");
  vorpal_index::build_index(&source, &index).unwrap();
  let kg = Kg::load(&index).unwrap();
  for suffix in ["nested/target.rs", "nested\\target.rs"] {
    let target = GraphTarget { name: "target".into(), path_suffix: Some(suffix.into()), ..Default::default() };
    let ids = vorpal_index::resolve_target(&kg, &target).unwrap();
    assert_eq!(ids.len(), 1, "{suffix}");
    let view = kg.node(ids[0]).unwrap();
    for selector in [
      SymbolSelector { id: Some(ids[0].raw()), path_suffix: Some(suffix), ..Default::default() },
      SymbolSelector { external_id: view.external_id, path_suffix: Some(suffix), ..Default::default() },
    ] {
      assert_eq!(kg.select(&selector), ids, "{suffix}");
    }
    let result = vorpal_index::graph_query_selected(&index, "callers", &target).unwrap();
    assert!(result.contains("caller"), "{result}");
    let filtered = vorpal_index::SearchFilter { path_suffix: Some(suffix.into()), ..SearchFilter::default() };
    let result = vorpal_index::search_records_filtered(&index, "target", 5, &filtered).unwrap();
    assert!(result.iter().any(|record| record.node.name == "target"), "{result:?}");
  }
  drop(kg);
  fs::remove_dir_all(base).unwrap();
}
