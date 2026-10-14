//! Priority changes dispatch only; canonical graph/spill bytes and admission bounds remain exact.
use std::{collections::HashMap, io};
use vorpal_ingest::{
  link_writer_spilled, stream_apply_spilled_with_priority, FileProduct, FileStat, OutlineExtractor,
  Resolver, StreamWork,
};

fn fixture() -> (Vec<FileStat>, HashMap<String, FileProduct>) {
  let extractor = OutlineExtractor::new().unwrap();
  let mut products = HashMap::new();
  let entries = (0..120)
    .map(|i| {
      let path = format!("src/work_{i:03}.rs");
      let source = format!("fn work_{i}() {{ work_{}(); }}", (i + 1) % 120);
      products.insert(
        path.clone(),
        extractor.extract_product(&path, &source).unwrap(),
      );
      FileStat {
        path,
        size: source.len() as u64,
        mtime_ns: i,
      }
    })
    .collect();
  (entries, products)
}

fn position(entry: &FileStat) -> usize {
  entry.path[9..12].parse().unwrap()
}

#[test]
fn priority_preserves_graph_bytes_skips_replay_and_starved_admission() {
  let (entries, products) = fixture();
  let interner = vorpal_ingest::Interner::default();
  let root = tempfile::tempdir().unwrap();
  let preferred = |entry: &FileStat| position(entry) % 3 == 0;
  let all = |_: &FileStat| true;
  let none = |_: &FileStat| false;
  let mut expected = None;
  for (case, budget, priority) in [
    ("fifo", 64 * 1024, None),
    (
      "mixed",
      64 * 1024,
      Some(&preferred as &(dyn Fn(&FileStat) -> bool + Sync)),
    ),
    ("starved", 100, Some(&preferred)),
    ("single_reservation", 1, Some(&preferred)),
    ("priority_only", 100, Some(&all)),
    ("ordinary_only", 100, Some(&none)),
  ] {
    let spill = root.path().join(format!("{case}.refs"));
    let (writer, references, stats, _flows) = stream_apply_spilled_with_priority(
      &interner,
      &entries,
      budget,
      &spill,
      None,
      None,
      None,
      priority,
      |entry, _| {
        let i = position(entry);
        if i % 7 == 0 {
          return Ok(StreamWork::Skipped);
        }
        let product = products[&entry.path].clone();
        Ok(if i % 2 == 0 {
          StreamWork::Parsed(entry.path.clone(), product)
        } else {
          StreamWork::Replayed(entry.path.clone(), product)
        })
      },
    )
    .unwrap();
    assert!(stats.peak_in_flight_bytes <= budget);
    let spill_bytes = ["refs", "args", "params", "rets", "sigs", "reqs"]
      .map(|extension| std::fs::read(spill.with_extension(extension)).unwrap());
    let (graph, resolution, _) =
      link_writer_spilled(&interner, writer, references, &Resolver::new()).unwrap();
    let output = root.path().join(case);
    graph.save(&output).unwrap();
    let bytes = ["nodes.vseg", "strings.heap", "graph.bin"]
      .map(|name| std::fs::read(output.join(name)).unwrap());
    let identity = (bytes, spill_bytes, stats.parsed, stats.replayed, resolution);
    if let Some(expected) = &expected {
      assert_eq!(&identity, expected, "{case}");
    } else {
      expected = Some(identity);
    }
  }
}

#[test]
fn failure_in_either_lane_aborts_without_a_partial_result() {
  let (entries, _) = fixture();
  let interner = vorpal_ingest::Interner::default();
  let root = tempfile::tempdir().unwrap();
  let preferred = |entry: &FileStat| position(entry) % 3 == 0;
  for fail_at in [6, 7] {
    let spill = root.path().join(format!("failure-{fail_at}.refs"));
    let result = stream_apply_spilled_with_priority(
      &interner,
      &entries,
      1,
      &spill,
      None,
      None,
      None,
      Some(&preferred),
      |entry, _| {
        if position(entry) == fail_at {
          Err(io::Error::other("native capture unavailable"))
        } else {
          Ok(StreamWork::Skipped)
        }
      },
    );
    match result {
      Err(error) => assert!(error.to_string().contains("native capture unavailable")),
      Ok(_) => panic!("a partial graph must not be returned"),
    }
  }
}
