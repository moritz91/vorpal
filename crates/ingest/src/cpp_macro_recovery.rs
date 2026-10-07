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
  /// Independently incompatible macro replacement/context syntax.
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

// Diagnose only a bounded, independently parsed replacement/context proof.
// Surrounding macro tokens can change the syntactic slot, so they decline the
// diagnostic. These ephemeral fixtures never replace the input or cached tree.
fn macro_calls(
  parsed: &crate::ParsedRoot,
  evidence: &crate::cpp_macro_evidence::Evidence,
) -> Vec<(String, Range<usize>, bool)> {
  parsed
    .root()
    .dfs()
    .filter_map(|call| {
      if call.kind().as_ref() == "macro_statement" {
        let name = call.field("name")?;
        let text = name.text();
        evidence.at(&text, name.range().start)?;
        let mut parent = call.parent();
        while let Some(context) = parent {
          match context.kind().as_ref() {
            "function_definition" | "conditional_function_body" | "lambda_expression" => return None,
            "namespace_definition"
            | "linkage_specification"
            | "class_specifier"
            | "struct_specifier"
            | "union_specifier" => {
              let source = parsed.root().text();
              let range = context.range();
              let span = call.range();
              // A surrounding macro can manufacture a function or change the
              // declaration context. Such forms remain outside this proof.
              // Other scanner-proven complete statements cannot open an
              // enclosing function or alter the surrounding declaration scope.
              // Inspect original segments between them; no input is rewritten.
              let mut statements: Vec<_> = context
                .dfs()
                .filter(|n| n.kind().as_ref() == "macro_statement")
                .filter_map(|n| {
                  Some(n.field("name")?.range().start..n.field("arguments")?.range().end)
                })
                .collect();
              statements.sort_by_key(|s| s.start);
              let mut cursor = range.start;
              let mut expanding = false;
              for statement in statements {
                if statement.start < cursor {
                  continue;
                }
                expanding |= evidence.contains_expanding_tokens(&source[cursor..statement.start]);
                cursor = statement.end;
              }
              expanding |= evidence.contains_expanding_tokens(&source[cursor..range.end]);
              let incompatible = !expanding;
              return Some((text.into_owned(), span, incompatible));
            }
            _ => parent = context.parent(),
          }
        }
        return None;
      }
      if call.kind().as_ref() != "call_expression" {
        return None;
      }
      let function = call.field("function")?;
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
      let definition = evidence.at(&text, name.range().start)?;
      let expression = (|| {
        let mut context = call.clone();
        loop {
          if matches!(
            context.kind().as_ref(),
            "expression_statement"
              | "return_statement"
              | "declaration"
              | "throw_statement"
              | "if_statement"
              | "for_statement"
          ) {
            break;
          }
          context = context.parent()?;
          if matches!(
            context.kind().as_ref(),
            "compound_statement" | "conditional_function_body" | "translation_unit" | "function_definition"
          ) {
            return None;
          }
        }
        let range = context.range();
        let source = parsed.root().text();
        let start = name.range().start;
        let end = call.range().end;
        let prefix = &source[range.start..start];
        let suffix = &source[end..range.end];
        let arguments = call.field("arguments")?.text();
        if evidence.contains_expanding_tokens(prefix)
          || evidence.contains_expanding_tokens(suffix)
          || evidence.contains_expanding_tokens(&arguments)
        {
          return None;
        }
        let arguments = validated_arguments(&arguments, definition.parameters)?;
        if arguments.len() != definition.parameters {
          return None;
        }
        let replacement = definition
          .replacement
          .instantiate(&arguments.iter().map(String::as_str).collect::<Vec<_>>())?;
        if evidence.contains_expanding_tokens(&replacement)
          || prefix.len() + suffix.len() + replacement.len() > 4 * 1024 * 1024
        {
          return None;
        }
        let fixture = format!("void proof() {{ {prefix}{replacement}{suffix}\n }}");
        // An already malformed surrounding statement is not evidence that this
        // invocation caused its error. Preserve that tree's ordinary diagnostics.
        let original = format!(
          "void proof() {{ {prefix}{}{suffix}\n }}",
          &source[start..end]
        );
        if SupportLang::Cpp.grep(&original).root().has_error() {
          return None;
        }
        let proof = SupportLang::Cpp.grep(&fixture);
        let root = proof.root();
        // The grammar permits compound statements in argument lists for SDK
        // recovery. Native GNU statement expressions require enclosing ().
        let bare_compound = root.dfs().any(|n| {
          n.kind().as_ref() == "compound_statement"
            && n
              .parent()
              .is_some_and(|p| p.kind().as_ref() == "argument_list")
        });
        Some(root.has_error() || bare_compound)
      })()
      .unwrap_or(false);
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
  let mut invocation_kinds: BTreeMap<String, BTreeMap<usize, (usize, bool)>> = BTreeMap::new();
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
          if definition.replacement.requires_semicolon
            && bytes.get(invocation_spacing(bytes, end)?) != Some(&b';')
          {
            return None;
          }
          if evidence.contains_expanding_tokens(&source[next..end]) {
            return None;
          }
          let arguments = validated_arguments(&source[next..end], definition.parameters)?;
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
          Some((
            count,
            end,
            crate::cpp_macro_evidence::statement_accepts_else(&replacement),
          ))
        });
        if let Some((count, end, open_if)) = proven {
          invocation_kinds
            .entry(name.to_owned())
            .or_default()
            .insert(start, (end, open_if));
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
  // Offset-free scanner framing cannot change syntax class in the middle of a file.
  for (name, sites) in &invocation_kinds {
    let classes: BTreeSet<_> = sites.values().map(|(_, open_if)| *open_if).collect();
    if classes.len() != 1 {
      rejected.insert(name.clone());
    }
  }
  let open_if_names: Vec<_> = invocation_kinds
    .iter()
    .filter(|(_, sites)| sites.values().all(|(_, open_if)| *open_if))
    .map(|(name, _)| name.clone())
    .collect();
  let mut eligible_names: Vec<_> = candidates
    .into_keys()
    .filter(|name| name.len() <= 128 && name.is_ascii() && !rejected.contains(name))
    .collect();
  let parse = |names: &[String]| {
    let open: Vec<_> = open_if_names
      .iter()
      .filter(|name| names.contains(name))
      .cloned()
      .collect();
    vorpal_language::with_cpp_statement_macro_kinds(names, &open, || lang.grep(source))
  };
  let mut parsed = parse(&eligible_names);
  let uses = macro_calls(&parsed, &evidence);
  let invalid: BTreeSet<_> = uses
    .iter()
    .filter(|(_, _, expression)| *expression)
    .map(|(name, _, _)| name.clone())
    .collect();
  let mut errors: Vec<_> = uses
    .into_iter()
    .filter(|(_, _, expression)| *expression)
    .map(|(_, span, _)| span)
    .collect();
  if eligible_names.iter().any(|name| invalid.contains(name)) {
    eligible_names.retain(|name| !invalid.contains(name));
    parsed = parse(&eligible_names);
  }
  errors.extend(incompatible_else_spans(
    &parsed,
    &evidence,
    &invocation_kinds,
  ));
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

// The upstream grammar can read an orphan `else after();` as a declaration
// whose type is the identifier "else". Diagnose only scanner-proven invocations
// and their adjacent literal else; unrelated calls and opaque expansions decline.
fn incompatible_else_spans(
  parsed: &crate::ParsedRoot,
  evidence: &crate::cpp_macro_evidence::Evidence,
  sites: &BTreeMap<String, BTreeMap<usize, (usize, bool)>>,
) -> Vec<Range<usize>> {
  let root = parsed.root();
  let source = root.text();
  let bytes = source.as_bytes();
  let mut errors = Vec::new();
  let mut statements = BTreeMap::new();
  let mut enclosing_if = BTreeMap::new();
  for node in root.dfs() {
    if node.kind().as_ref() == "macro_statement" {
      statements.insert(node.range().start, node);
    } else if node.kind().as_ref() == "if_statement" {
      if let (Some(alternative), Some(consequence)) =
        (node.field("alternative"), node.field("consequence"))
      {
        enclosing_if.insert(alternative.range().start, consequence.range());
      }
    }
  }
  for (name, invocations) in sites {
    for (&start, &(end, open_if)) in invocations {
      let Some(statement) = statements.get(&start) else {
        continue;
      };
      let Some(mut next) = invocation_spacing(bytes, end) else {
        continue;
      };
      let mut required_terminator = evidence
        .at(name, start)
        .is_some_and(|definition| definition.replacement.requires_semicolon);
      let mut semicolon = false;
      while bytes.get(next) == Some(&b';') {
        // The first source semicolon completes an unterminated do/while
        // replacement; only subsequent semicolons are empty statements.
        if required_terminator {
          required_terminator = false;
        } else {
          semicolon = true;
        }
        let Some(after) = invocation_spacing(bytes, next + 1) else {
          break;
        };
        next = after;
      }
      if bytes.get(next..next + 4) != Some(b"else")
        || bytes
          .get(next + 4)
          .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        || evidence.contains_expanding_tokens("else")
      {
        continue;
      }
      let mut parent = statement.parent();
      let mut body_start = None;
      while let Some(node) = parent {
        if matches!(
          node.kind().as_ref(),
          "compound_statement" | "conditional_function_body"
        ) {
          body_start = Some(node.range().start);
          break;
        }
        parent = node.parent();
      }
      let Some(mut cursor) = body_start else {
        continue;
      };
      // An opaque prefix macro may manufacture the enclosing if. Independently
      // proven statement invocations cannot manufacture a containing function;
      // inspect original gaps between them and decline expanding prefixes.
      let mut expanding = false;
      for (&prior_start, prior) in &statements {
        if prior_start < cursor || prior_start >= start {
          continue;
        }
        let Some(arguments) = prior.field("arguments") else {
          continue;
        };
        if arguments.range().end > start {
          continue;
        }
        expanding |= evidence.contains_expanding_tokens(&source[cursor..prior_start]);
        cursor = arguments.range().end;
      }
      expanding |= evidence.contains_expanding_tokens(&source[cursor..start]);
      if expanding {
        continue;
      }
      // A closed statement is a valid consequence of an enclosing ordinary if.
      // An extra source semicolon breaks both the enclosing and internal if.
      let attached = !semicolon
        && (open_if
          || enclosing_if
            .get(&next)
            .is_some_and(|body| body.start <= start && body.end >= end));
      if !attached {
        // Keep original invocation/else bytes; no fabricated token or source mask.
        errors.push(start..next + 4);
      }
    }
  }
  errors
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
    .filter(|n| matches!(n.kind().as_ref(), "function_definition" | "conditional_function_prefix"))
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

fn validated_arguments(arguments: &str, parameters: usize) -> Option<Vec<String>> {
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
    // F() supplies one empty preprocessing argument to a one-parameter macro,
    // but no arguments to a zero-parameter macro. The replacement/context proof
    // decides whether that empty token sequence is syntactically usable.
    return Some(if parameters == 1 {
      vec![String::new()]
    } else {
      Vec::new()
    });
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
