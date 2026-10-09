//! Scoped scanner context for callers that have independently proven every use.
use std::ffi::{CString, c_char, c_void};

unsafe extern "C" {
  fn tree_sitter_cpp_set_statement_macros(names: *const c_char) -> *const c_char;
  fn tree_sitter_cpp_set_statement_macro_sites(sites: *const Sites) -> *const Sites;
  fn ts_vorpal_lexer_byte_offset(lexer: *const c_void) -> u32;
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
  let restore = set_context(
    if text_is_empty {
      std::ptr::null()
    } else {
      names.as_ptr()
    },
    std::ptr::null(),
  );
  let result = parse();
  drop(restore);
  result
}

/// One independently proven original invocation. Offsets are source bytes, not
/// character columns. Calls at other positions, even with this name, remain raw.
#[derive(Debug, Clone)]
pub struct CppStatementMacroSite {
  pub offset: u32,
  pub name: String,
  pub open_if: bool,
}

#[repr(C)]
struct Site {
  offset: u32,
  name: *const c_char,
  open_if: bool,
}
#[repr(C)]
struct Sites {
  sites: *const Site,
  length: usize,
  byte_offset: unsafe extern "C" fn(*const c_void) -> u32,
}

struct Restore {
  names: *const c_char,
  sites: *const Sites,
}
impl Drop for Restore {
  fn drop(&mut self) {
    // SAFETY: both pointers belong to enclosing synchronous scopes or are null;
    // those scopes outlive this restoration even during unwinding.
    unsafe {
      tree_sitter_cpp_set_statement_macros(self.names);
      tree_sitter_cpp_set_statement_macro_sites(self.sites);
    }
  }
}
fn set_context(names: *const c_char, sites: *const Sites) -> Restore {
  // SAFETY: callers keep the borrowed inputs alive until this guard drops. Both
  // contexts are TLS, excluded from scanner serialization and mutually exclusive.
  unsafe {
    Restore {
      names: tree_sitter_cpp_set_statement_macros(names),
      sites: tree_sitter_cpp_set_statement_macro_sites(sites),
    }
  }
}

/// Run a fresh synchronous parse with proof limited to exact original sites.
/// The caller must independently prove source, definition, arguments and context
/// for each site. Do not reuse a tree produced under a different proof context.
/// The runtime supplies its byte-offset reader explicitly; the standalone stock
/// tree-sitter CLI needs neither that symbol nor any changed TSLexer ABI.
pub fn with_cpp_statement_macro_sites<R>(
  sites: &[CppStatementMacroSite],
  parse: impl FnOnce() -> R,
) -> R {
  assert!(sites.windows(2).all(|pair| pair[0].offset < pair[1].offset));
  let names: Vec<_> = sites
    .iter()
    .map(|site| {
      assert!(!site.name.is_empty() && site.name.len() <= 128);
      assert!(site.name.as_bytes()[0].is_ascii_alphabetic() || site.name.starts_with('_'));
      assert!(
        site
          .name
          .bytes()
          .all(|b| b.is_ascii_alphanumeric() || b == b'_')
      );
      CString::new(site.name.as_str()).unwrap()
    })
    .collect();
  let records: Vec<_> = sites
    .iter()
    .zip(&names)
    .map(|(site, name)| Site {
      offset: site.offset,
      name: name.as_ptr(),
      open_if: site.open_if,
    })
    .collect();
  let context = Sites {
    sites: records.as_ptr(),
    length: records.len(),
    byte_offset: ts_vorpal_lexer_byte_offset,
  };
  let restore = set_context(
    std::ptr::null(),
    if records.is_empty() {
      std::ptr::null()
    } else {
      &context
    },
  );
  let result = parse();
  drop(restore);
  result
}
