//! Original-span directive inventory, independent of unexpanded C++ declarations.
//! This is diagnostic only; it does not evaluate guards or enable recovery.
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
  pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncertainLexing {
  pub offset: usize,
}

fn splice(bytes: &[u8], offset: usize) -> Option<usize> {
  if bytes.get(offset) != Some(&b'\\') {
    return None;
  }
  if bytes.get(offset + 1) == Some(&b'\n') {
    return Some(offset + 2);
  }
  (bytes.get(offset + 1..offset + 3) == Some(b"\r\n")).then_some(offset + 3)
}

/// Inventory logical-line directives without changing the input. Unknown token
/// encodings and unterminated literals/comments fail closed. No grammar or
/// preprocessor state is inferred from the resulting spans.
pub fn audit(source: &str) -> Result<Vec<Directive>, UncertainLexing> {
  let bytes = source.as_bytes();
  if let Some(offset) = bytes.windows(2).position(|window| window == b"??") {
    return Err(UncertainLexing { offset });
  }
  if let Some(offset) = bytes
    .iter()
    .enumerate()
    .find_map(|(i, b)| (*b == b'\r' && bytes.get(i + 1) != Some(&b'\n')).then_some(i))
  {
    return Err(UncertainLexing { offset });
  }
  let mut directives = Vec::new();
  let mut offset = if bytes.starts_with(b"\xef\xbb\xbf") {
    3
  } else {
    0
  };
  let mut line_start = true;
  let mut active = None;
  while offset < bytes.len() {
    if let Some(next) = splice(bytes, offset) {
      // Outside a directive, phase-two joining may form a new comment/literal
      // delimiter. This inventory deliberately declines that unmodeled case.
      if active.is_none() || offset == 0 || !bytes[offset - 1].is_ascii_whitespace() {
        return Err(UncertainLexing { offset });
      }
      offset = next;
      continue;
    }
    if bytes[offset] == b'\n' {
      if let Some(start) = active.take() {
        directives.push(Directive {
          span: start..offset + 1,
        });
      }
      line_start = true;
      offset += 1;
      continue;
    }
    if bytes[offset].is_ascii_whitespace() {
      offset += 1;
      continue;
    }
    if bytes.get(offset..offset + 2) == Some(b"//") {
      offset += 2;
      while offset < bytes.len() && bytes[offset] != b'\n' {
        if let Some(next) = splice(bytes, offset) {
          offset = next;
        } else {
          offset += 1;
        }
      }
      continue;
    }
    if bytes.get(offset..offset + 2) == Some(b"/*") {
      let start = offset;
      offset += 2;
      while offset < bytes.len() && bytes.get(offset..offset + 2) != Some(b"*/") {
        if let Some(next) = splice(bytes, offset) {
          offset = next;
          continue;
        }
        // The whole comment becomes whitespace, including its newlines. It
        // neither ends an active directive nor starts a fresh logical line.
        offset += 1;
      }
      if offset == bytes.len() {
        return Err(UncertainLexing { offset: start });
      }
      offset += 2;
      continue;
    }
    let raw_prefix = [b"u8R\"".as_slice(), b"uR\"", b"UR\"", b"LR\"", b"R\""]
      .into_iter()
      .find(|prefix| bytes.get(offset..offset + prefix.len()) == Some(*prefix));
    if let Some(prefix) = raw_prefix {
      let start = offset;
      let delimiter_start = offset + prefix.len();
      offset = delimiter_start;
      while offset < bytes.len() && bytes[offset] != b'(' {
        if offset - delimiter_start >= 16
          || bytes[offset].is_ascii_whitespace()
          || matches!(bytes[offset], b'\\' | b')')
          || !bytes[offset].is_ascii()
        {
          return Err(UncertainLexing { offset: start });
        }
        offset += 1;
      }
      if offset == bytes.len() {
        return Err(UncertainLexing { offset: start });
      }
      let mut close = vec![b')'];
      close.extend_from_slice(&bytes[delimiter_start..offset]);
      close.push(b'"');
      offset += 1;
      let Some(relative) = bytes[offset..]
        .windows(close.len())
        .position(|window| window == close)
      else {
        return Err(UncertainLexing { offset: start });
      };
      offset += relative + close.len();
      line_start = false;
      continue;
    }
    if bytes[offset].is_ascii_digit() {
      offset += 1;
      while offset < bytes.len()
        && (bytes[offset].is_ascii_alphanumeric()
          || bytes[offset] == b'_'
          || bytes[offset] == b'.'
          || bytes[offset] == b'\'' && bytes.get(offset + 1).is_some_and(u8::is_ascii_alphanumeric))
      {
        offset += 1;
      }
      line_start = false;
      continue;
    }
    if matches!(bytes[offset], b'"' | b'\'') {
      let start = offset;
      let quote = bytes[offset];
      offset += 1;
      loop {
        if offset >= bytes.len() || bytes[offset] == b'\n' {
          return Err(UncertainLexing { offset: start });
        }
        if let Some(next) = splice(bytes, offset) {
          offset = next;
          continue;
        }
        if bytes[offset] == quote {
          offset += 1;
          break;
        }
        if bytes[offset] == b'\\' {
          offset += 1;
        }
        offset += 1;
      }
      line_start = false;
      continue;
    }
    if line_start && bytes[offset] == b'#' {
      active = Some(offset);
    }
    // Alternative preprocessing token spellings are not normalized by this audit.
    if bytes.get(offset..offset + 2) == Some(b"%:") {
      return Err(UncertainLexing { offset });
    }
    line_start = false;
    offset += 1;
  }
  if let Some(start) = active {
    directives.push(Directive {
      span: start..source.len(),
    });
  }
  Ok(directives)
}
