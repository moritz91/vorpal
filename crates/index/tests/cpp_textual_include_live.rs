//! Deferred live commits cannot publish a context after one original input changed.
use std::fs;
use vorpal_index::{CppTextualIncludeContext, ExtractionEnv, build_index_env, build_index_live};

#[test]
fn input_edits_between_live_parse_and_persist_refuse_commit() {
  let dir = tempfile::tempdir().unwrap();
  let src = dir.path().join("src");
  fs::create_dir(&src).unwrap();
  let root = src.join("root.cc");
  let head = src.join("head.cc");
  let tail = src.join("tail.cc");
  fs::write(
    &root,
    "namespace Real {\n#include \"head.cc\"\n#include \"tail.cc\"\n}\n",
  )
  .unwrap();
  fs::write(&head, "int real() {\n").unwrap();
  fs::write(&tail, "return 1;\n}\n").unwrap();
  let env = ExtractionEnv {
    cpp_textual_include_contexts: vec![CppTextualIncludeContext {
      root,
      includes: vec![head, tail.clone()],
    }],
    ..Default::default()
  };
  let out = dir.path().join("index");
  build_index_env(&src, &out, Default::default(), Default::default(), &env).unwrap();
  let original = fs::read(out.join("CURRENT")).unwrap();
  let live = build_index_live(&src, &out, Some(&Default::default()), &env).unwrap();
  assert_eq!(live.report.error_nodes, 0);
  assert!(live.kg.is_some());
  fs::write(&tail, "return 2;\n}\n").unwrap();
  assert!(live.pending.unwrap().persist().is_err());
  assert_eq!(fs::read(out.join("CURRENT")).unwrap(), original);
  let live = build_index_live(&src, &out, Some(&Default::default()), &env).unwrap();
  live.pending.unwrap().persist().unwrap();
  let scratch = dir.path().join("scratch");
  build_index_env(&src, &scratch, Default::default(), Default::default(), &env).unwrap();
  assert_eq!(
    fs::read(out.join("CURRENT")).unwrap(),
    fs::read(scratch.join("CURRENT")).unwrap()
  );
}
