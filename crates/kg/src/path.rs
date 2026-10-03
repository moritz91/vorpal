//! Path selectors compare separator spellings without allocating or changing identity.

/// String-suffix semantics with `/` and `\` treated as equivalent separators.
/// Case, partial filenames and an explicitly supplied trailing separator stay significant.
/// This does not canonicalize paths or rewrite the paths stored in the graph.
pub fn path_has_suffix(path: &str, suffix: &str) -> bool {
  if path.ends_with(suffix) {
    return true;
  }
  let path = path.as_bytes();
  let suffix = suffix.as_bytes();
  if suffix.len() > path.len() {
    return false;
  }
  path[path.len() - suffix.len()..].iter().zip(suffix).all(|(&actual, &want)| {
    actual == want || (matches!(actual, b'/' | b'\\') && matches!(want, b'/' | b'\\'))
  })
}

#[cfg(test)]
mod tests {
  use super::path_has_suffix;

  #[test]
  fn suffixes_keep_string_semantics_across_separator_styles() {
    for path in [r"\\?\C:\repo\src\café.cc", "C:/repo/src/café.cc", "/repo/src/café.cc"] {
      assert!(path_has_suffix(path, "src/café.cc"));
      assert!(path_has_suffix(path, r"src\café.cc"));
      assert!(path_has_suffix(path, "fé.cc"));
      assert!(path_has_suffix(path, ""));
      assert!(!path_has_suffix(path, "other/café.cc"));
      assert!(!path_has_suffix(path, "src/Café.cc"));
      assert!(!path_has_suffix(path, "src/café.cc/"));
      assert!(!path_has_suffix(path, "long/long/long/long/src/café.cc"));
    }
  }
}
