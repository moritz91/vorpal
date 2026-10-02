use std::fs;
use vorpal_index::scope::resolve_path_prefix;

#[test]
fn prefixes_use_the_source_root_and_preserve_partial_names() {
  let root = std::env::temp_dir().join(format!("vorpal-prefix-{}", std::process::id()));
  fs::create_dir_all(root.join("src")).unwrap();
  let src = root.join("src").canonicalize().unwrap();
  assert_eq!(resolve_path_prefix("src", Some(&root)).unwrap(), src.to_string_lossy());
  assert_eq!(resolve_path_prefix("src/", Some(&root)).unwrap(), format!("{}{}", src.display(), std::path::MAIN_SEPARATOR));
  assert_eq!(resolve_path_prefix("src/part", Some(&root)).unwrap(), src.join("part").to_string_lossy());
  assert_eq!(resolve_path_prefix(&root.join("src").to_string_lossy(), None).unwrap(), src.to_string_lossy());
  assert!(resolve_path_prefix("src/", None).unwrap_err().contains("no source root"));
  assert!(resolve_path_prefix("", Some(&root)).is_err());
  #[cfg(windows)]
  {
    assert_eq!(resolve_path_prefix("src\\", Some(&root)).unwrap(), resolve_path_prefix("src/", Some(&root)).unwrap());
    let forward = root.join("src").to_string_lossy().replace('\\', "/");
    assert_eq!(resolve_path_prefix(&forward, None).unwrap(), src.to_string_lossy());
  }
  fs::remove_dir_all(root).unwrap();
}
