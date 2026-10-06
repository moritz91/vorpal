//! Scoped scanner context for callers that have independently proven every use.
use std::ffi::{CString, c_char};

unsafe extern "C" {
  fn tree_sitter_cpp_set_statement_macros(names: *const c_char) -> *const c_char;
}

/// Run a synchronous parse with proven closed ASCII statement macro names.
/// Use with_cpp_statement_macro_kinds for replacements retaining a dangling if.
/// The caller must prove
/// all invocation sites and arities before entering; the scanner has no byte offset.
/// The context is thread-local, nested, panic-safe and does not own or mutate source.
pub fn with_cpp_statement_macros<R>(names: &[String], parse: impl FnOnce() -> R) -> R {
  with_cpp_statement_macro_kinds(names, &[], parse)
}

/// Names whose independently instantiated replacements retain a dangling if.
/// The subset and syntax class must agree at every invocation of each name.
/// `?` is internal framing, never a source token or serialized scanner state.
pub fn with_cpp_statement_macro_kinds<R>(
  names: &[String],
  open_if_names: &[String],
  parse: impl FnOnce() -> R,
) -> R {
  assert!(open_if_names.iter().all(|name| names.contains(name)));
  let mut text = String::new();
  for name in names {
    assert!(!name.is_empty() && name.len() <= 128);
    assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    if open_if_names.contains(name) {
      text.push('?');
    }
    text.push_str(name);
    text.push('\n');
  }
  let text_is_empty = text.is_empty();
  let names = CString::new(text).unwrap();
  struct Restore(*const c_char);
  impl Drop for Restore {
    fn drop(&mut self) {
      // SAFETY: the previous pointer belongs to a surrounding synchronous scope,
      // or is null. That scope outlives this guard, including during unwinding.
      unsafe {
        tree_sitter_cpp_set_statement_macros(self.0);
      }
    }
  }
  // SAFETY: this CString lives through the parse and guard restoration. C retains
  // only the pointer in TLS, never in scanner/tree state or another thread.
  let restore = Restore(unsafe {
    tree_sitter_cpp_set_statement_macros(if text_is_empty {
      std::ptr::null()
    } else {
      names.as_ptr()
    })
  });
  let result = parse();
  drop(restore);
  result
}
