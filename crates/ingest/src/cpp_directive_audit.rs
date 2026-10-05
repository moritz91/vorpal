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

/// One textual branch. Its expression is not evaluated or macro-expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
  pub header: Range<usize>,
  pub body: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalGroup {
  pub span: Range<usize>,
  pub branches: Vec<Branch>,
  pub close: Range<usize>,
}

struct OpenGroup {
  start: usize,
  branches: Vec<Branch>,
  has_else: bool,
}

fn keyword(source: &str, span: &Range<usize>) -> Result<String, UncertainLexing> {
  let bytes = source.as_bytes();
  let mut offset = span.start + 1;
  loop {
    if let Some(next) = splice(bytes, offset) {
      offset = next;
      continue;
    }
    if offset >= span.end {
      return Ok(String::new());
    }
    if bytes[offset].is_ascii_whitespace() {
      offset += 1;
      continue;
    }
    if bytes.get(offset..offset + 2) == Some(b"/*") {
      let Some(end) = bytes[offset + 2..span.end]
        .windows(2)
        .position(|p| p == b"*/")
      else {
        return Err(UncertainLexing { offset });
      };
      offset += end + 4;
      continue;
    }
    let start = offset;
    while offset < span.end && (bytes[offset].is_ascii_alphabetic() || bytes[offset] == b'_') {
      offset += 1;
    }
    if offset == start {
      return Err(UncertainLexing { offset });
    }
    return Ok(source[start..offset].to_owned());
  }
}

/// Match complete textual guard groups. This checks delimiter order only, not
/// expression legality, macro effects, includes or compiler configuration.
pub fn audit_groups(source: &str) -> Result<Vec<ConditionalGroup>, UncertainLexing> {
  let directives = audit(source)?;
  let mut pending: Vec<OpenGroup> = Vec::new();
  let mut groups = Vec::new();
  for directive in directives {
    let span = directive.span;
    match keyword(source, &span)?.as_str() {
      "if" | "ifdef" | "ifndef" => pending.push(OpenGroup {
        start: span.start,
        branches: vec![Branch {
          header: span.clone(),
          body: span.end..span.end,
        }],
        has_else: false,
      }),
      "else" | "elif" | "elifdef" | "elifndef" => {
        let Some(group) = pending.last_mut() else {
          return Err(UncertainLexing { offset: span.start });
        };
        if group.has_else {
          return Err(UncertainLexing { offset: span.start });
        }
        group.has_else = keyword(source, &span)? == "else";
        group.branches.last_mut().unwrap().body.end = span.start;
        group.branches.push(Branch {
          header: span.clone(),
          body: span.end..span.end,
        });
      }
      "endif" => {
        let Some(mut group) = pending.pop() else {
          return Err(UncertainLexing { offset: span.start });
        };
        group.branches.last_mut().unwrap().body.end = span.start;
        groups.push(ConditionalGroup {
          span: group.start..span.end,
          branches: group.branches,
          close: span,
        });
      }
      _ => {}
    }
  }
  if let Some(group) = pending.first() {
    return Err(UncertainLexing {
      offset: group.start,
    });
  }
  groups.sort_by_key(|group| group.span.start);
  Ok(groups)
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
