//! Proof-backed statement parsing and a read-only recovery report.
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use vorpal_core::tree_sitter::LanguageExt;
use vorpal_lang_registry::SgLang;
use vorpal_language::SupportLang;

#[derive(Debug)]
pub struct RecoveryAudit {
  pub has_error: bool,
  pub eligible_names: Vec<String>,
  pub macro_spans: Vec<Range<usize>>,
  pub calls: Vec<(String, Range<usize>)>,
  pub functions: Vec<String>,
  pub dependency_identity: u64,
  /// Proven statement replacements used where C++ requires an expression.
  pub context_errors: Vec<Range<usize>>,
}

/// Prove every invocation of a name before giving it to the offset-free scanner.
/// One unproven/incorrect-arity occurrence disables that name for the whole file.
/// This report does not write products or activate recovery in default extraction.
pub fn audit_recovery(path: &Path, source: &str, roots: &[PathBuf]) -> RecoveryAudit {
  vorpal_language::with_cpp_statement_macros(&[], || audit_without_context(path, source, roots))
}

#[derive(Debug, Default)]
pub(crate) struct ContextDiagnostics {
  pub errors: Vec<Range<usize>>,
  pub macro_calls: Vec<Range<usize>>,
}

// A proven if/try/compound replacement cannot occupy an expression. Keep this
// check separate from the unexpanded tree: no source or tree is rewritten.
fn macro_calls(
  parsed: &crate::ParsedRoot,
  evidence: &crate::cpp_macro_evidence::Evidence,
) -> Vec<(String, Range<usize>, bool)> {
  parsed
    .root()
    .dfs()
    .filter_map(|call| {
      if call.kind().as_ref() != "call_expression" {
        return None;
      }
      let function = call.field("function")?;
      let plain = function.kind().as_ref() == "identifier";
      let mut name = function;
      loop {
        name = match name.kind().as_ref() {
          "field_expression" => name.field("field")?,
          "qualified_identifier" => name.field("name")?,
          "identifier" | "field_identifier" => break,
          _ => return None,
        };
      }
      let text = name.text();
      evidence.at(&text, name.range().start)?;
      let expression = !plain
        || call
          .parent()
          .is_none_or(|p| p.kind().as_ref() != "expression_statement");
      Some((text.into_owned(), call.range(), expression))
    })
    .collect()
}

fn parse_without_context(
  path: &Path,
  source: &str,
  roots: &[PathBuf],
) -> (
  crate::ParsedRoot,
  Vec<String>,
  crate::cpp_macro_evidence::Evidence,
  ContextDiagnostics,
) {
  let lang = SgLang::Builtin(SupportLang::Cpp);
  let raw = lang.grep(source);
  let evidence = crate::cpp_macro_evidence::audit_with_roots(path, source, roots);
  let protected: Vec<_> = raw
    .root()
    .dfs()
    .filter_map(|n| {
      let kind = n.kind();
      matches!(
        kind.as_ref(),
        "preproc_def"
          | "preproc_function_def"
          | "comment"
          | "string_literal"
          | "raw_string_literal"
          | "char_literal"
      )
      .then(|| (n.range(), kind.as_ref() != "comment"))
    })
    .collect();
  let known: BTreeSet<_> = evidence
    .bindings
    .iter()
    .map(|b| b.definition.name.clone())
    .collect();
  let mut candidates: BTreeMap<String, BTreeMap<usize, usize>> = BTreeMap::new();
  let mut rejected = BTreeSet::new();
  let bytes = source.as_bytes();
  let mut i = 0;
  while i < bytes.len() {
    if let Some((range, _)) = protected.iter().find(|(range, _)| range.contains(&i)) {
      i = range.end;
      continue;
    }
    if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
      let start = i;
      i += 1;
      while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
      }
      let name = &source[start..i];
      if !known.contains(name) {
        continue;
      }
      let Some(next) = invocation_spacing(bytes, i) else {
        rejected.insert(name.to_owned());
        continue;
      };
      // Comments are whitespace between the name and '('; the offset-free
      // scanner must use exactly the same conservative spacing rules.
      if bytes.get(next) == Some(&b'(') {
        let proven = evidence.at(name, start).and_then(|definition| {
          let end = argument_end(source, next, &protected)?;
          if evidence.contains_expanding_tokens(&source[next..end]) {
            return None;
          }
          let arguments = validated_arguments(&source[next..end])?;
          let count = arguments.len();
          if definition.parameters != count {
            return None;
          }
          let replacement = definition
            .replacement
            .instantiate(&arguments.iter().map(String::as_str).collect::<Vec<_>>())?;
          if evidence.contains_expanding_tokens(&replacement)
            || !crate::cpp_macro_evidence::complete_statement(&replacement)
          {
            return None;
          }
          Some(count)
        });
        if let Some(count) = proven {
          candidates
            .entry(name.to_owned())
            .or_default()
            .insert(start, count);
        } else {
          rejected.insert(name.to_owned());
        }
      }
    } else {
      i += 1;
    }
  }
  let mut eligible_names: Vec<_> = candidates
    .into_keys()
    .filter(|name| name.len() <= 128 && name.is_ascii() && !rejected.contains(name))
    .collect();
  let mut parsed =
    vorpal_language::with_cpp_statement_macros(&eligible_names, || lang.grep(source));
  let uses = macro_calls(&parsed, &evidence);
  let invalid: BTreeSet<_> = uses
    .iter()
    .filter(|(_, _, expression)| *expression)
    .map(|(name, _, _)| name.clone())
    .collect();
  let errors = uses
    .into_iter()
    .filter(|(_, _, expression)| *expression)
    .map(|(_, span, _)| span)
    .collect();
  if eligible_names.iter().any(|name| invalid.contains(name)) {
    eligible_names.retain(|name| !invalid.contains(name));
    parsed = vorpal_language::with_cpp_statement_macros(&eligible_names, || lang.grep(source));
  }
  let macro_calls = macro_calls(&parsed, &evidence)
    .into_iter()
    .map(|(_, span, _)| span)
    .collect();
  (
    parsed,
    eligible_names,
    evidence,
    ContextDiagnostics {
      errors,
      macro_calls,
    },
  )
}

fn statement_space(byte: &u8) -> bool {
  matches!(*byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'\x0b')
}

fn invocation_spacing(bytes: &[u8], mut i: usize) -> Option<usize> {
  loop {
    while bytes.get(i).is_some_and(statement_space) {
      i += 1;
    }
    if bytes.get(i) != Some(&b'/') {
      return Some(i);
    }
    match bytes.get(i + 1) {
      Some(b'/') => {
        i += 2;
        while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
          if bytes[i] == b'\\' || bytes.get(i..i + 3) == Some(b"??/") {
            return None;
          }
          i += 1;
        }
      }
      Some(b'*') => {
        i += 2;
        loop {
          if bytes.get(i..i + 3) == Some(b"??/") {
            return None;
          }
          match bytes.get(i) {
            None | Some(b'\\') => return None,
            Some(b'*') if bytes.get(i + 1) == Some(&b'/') => {
              i += 2;
              break;
            }
            _ => i += 1,
          }
        }
      }
      _ => return None,
    }
  }
}

pub(crate) fn parse_recovery(
  path: &Path,
  source: &str,
  roots: &[PathBuf],
) -> (
  crate::ParsedRoot,
  crate::cpp_macro_evidence::Evidence,
  ContextDiagnostics,
) {
  vorpal_language::with_cpp_statement_macros(&[], || {
    let (parsed, _, evidence, diagnostics) = parse_without_context(path, source, roots);
    (parsed, evidence, diagnostics)
  })
}

fn audit_without_context(path: &Path, source: &str, roots: &[PathBuf]) -> RecoveryAudit {
  let (parsed, eligible_names, evidence, diagnostics) = parse_without_context(path, source, roots);
  let dependency_identity = evidence.dependency_identity();
  let root = parsed.root();
  let macro_spans = root
    .dfs()
    .filter(|n| n.kind().as_ref() == "macro_statement")
    .map(|n| n.range())
    .collect();
  let calls = root
    .dfs()
    .filter(|n| {
      n.kind().as_ref() == "call_expression" && !diagnostics.macro_calls.contains(&n.range())
    })
    .filter_map(|n| {
      let name = n.field("function")?;
      (name.kind().as_ref() == "identifier").then(|| (name.text().into_owned(), n.range()))
    })
    .collect();
  let functions = root
    .dfs()
    .filter(|n| n.kind().as_ref() == "function_definition")
    .filter_map(|n| {
      let name = n.field("declarator")?.field("declarator")?;
      (name.kind().as_ref() == "identifier").then(|| name.text().into_owned())
    })
    .collect();
  RecoveryAudit {
    has_error: root.has_error() || !diagnostics.errors.is_empty(),
    eligible_names,
    macro_spans,
    calls,
    functions,
    dependency_identity,
    context_errors: diagnostics.errors,
  }
}

fn arity(source: &str, range: Range<usize>, protected: &[(Range<usize>, bool)]) -> usize {
  let bytes = source.as_bytes();
  let mut i = range.start + 1;
  let mut nesting = 0;
  let mut commas = 0;
  let mut value = false;
  while i + 1 < range.end {
    if let Some((span, literal)) = protected.iter().find(|(span, _)| span.contains(&i)) {
      value |= *literal;
      i = span.end;
      continue;
    }
    match bytes[i] {
      b'(' => {
        nesting += 1;
        value = true;
      }
      b')' => {
        nesting -= 1;
      }
      b',' if nesting == 0 => {
        commas += 1;
      }
      b if statement_space(&b) => {}
      _ => {
        value = true;
      }
    }
    i += 1;
  }
  if value { commas + 1 } else { 0 }
}

fn argument_end(source: &str, start: usize, protected: &[(Range<usize>, bool)]) -> Option<usize> {
  let mut i = start;
  let mut nesting = 0;
  while i < source.len() {
    if let Some((span, _)) = protected.iter().find(|(span, _)| span.contains(&i)) {
      i = span.end;
      continue;
    }
    match source.as_bytes()[i] {
      b'(' => nesting += 1,
      b')' => {
        nesting -= 1;
        if nesting == 0 {
          return Some(i + 1);
        }
      }
      _ => {}
    }
    i += 1;
  }
  None
}

fn validated_arguments(arguments: &str) -> Option<Vec<String>> {
  let source = format!("void proof() {{ probe{arguments}; }}");
  let parsed = SupportLang::Cpp.grep(&source);
  let root = parsed.root();
  if root.has_error() {
    return None;
  }
  let call = root.dfs().find(|n| {
    n.kind().as_ref() == "call_expression"
      && n.field("function").is_some_and(|f| f.text() == "probe")
  })?;
  let arguments = call.field("arguments")?;
  let protected: Vec<_> = root
    .dfs()
    .filter_map(|n| {
      let kind = n.kind();
      matches!(
        kind.as_ref(),
        "comment" | "string_literal" | "raw_string_literal" | "char_literal"
      )
      .then(|| (n.range(), kind.as_ref() != "comment"))
    })
    .collect();
  let range = arguments.range();
  let count = arity(&source, range.clone(), &protected);
  if count == 0 {
    return Some(Vec::new());
  }
  // Preprocessing commas are protected by parentheses only, not C++ templates,
  // initializer braces or subscripts. Literal/comment commas remain protected.
  let mut result = Vec::new();
  let mut start = range.start + 1;
  let mut i = start;
  let mut nesting = 0;
  while i + 1 < range.end {
    if let Some((span, _)) = protected.iter().find(|(span, _)| span.contains(&i)) {
      i = span.end;
      continue;
    }
    match source.as_bytes()[i] {
      b'(' => nesting += 1,
      b')' => nesting -= 1,
      b',' if nesting == 0 => {
        result.push(source[start..i].to_owned());
        start = i + 1;
      }
      _ => {}
    }
    i += 1;
  }
  result.push(source[start..range.end - 1].to_owned());
  (result.len() == count).then_some(result)
}
