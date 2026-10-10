//! Report-only recovery from physical compiler expansion observations.
//!
//! This validates the connection between captured buffers, definition anchors and
//! original invocations, then uses the same independent argument/context proof as
//! normal recovery. It does not run a compiler, validate its environment or file
//! queries, or produce roots/products/cache identities. A compiler observer and a
//! complete invalidation contract remain prerequisites for production use.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use vorpal_core::tree_sitter::LanguageExt;
use vorpal_language::SupportLang;

use crate::cpp_macro_evidence::{Binding, Evidence, StatementMacro, native_statement_replacement};

/// A spelling comparison for a report adapter, not a compiler/cache proof.
/// Native MSVC preprocessing can retain CRLF in a raw literal's spelling while
/// LLVM's spelling contains LF; native runtime controls agree on the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAgreement {
  Exact,
  RawLiteralLineEndings,
  Different,
}

/// Compare independently lexed preprocessing tokens. Only CRLF inside a complete
/// raw string literal may differ. Prefix, delimiter, punctuation, ordinary string
/// escapes and all other token bytes/order must agree. Original buffers/anchors
/// are untouched. A caller must still handle native directives and dependencies;
/// lines beginning with '#' inside a raw literal are payload, not directives.
pub fn compare_token_spellings(native: &[&str], observed: &[&str]) -> TokenAgreement {
  if native.len() != observed.len() {
    return TokenAgreement::Different;
  }
  let mut result = TokenAgreement::Exact;
  for (&a, &b) in native.iter().zip(observed) {
    if a == b {
      continue;
    }
    let (Some((a_head, a_payload, a_end)), Some((b_head, b_payload, b_end))) =
      (raw_literal_parts(a), raw_literal_parts(b))
    else {
      return TokenAgreement::Different;
    };
    if a_head != b_head || a_end != b_end || !raw_newlines(a_payload).eq(raw_newlines(b_payload)) {
      return TokenAgreement::Different;
    }
    result = TokenAgreement::RawLiteralLineEndings;
  }
  result
}

fn raw_literal_parts(token: &str) -> Option<(&str, &str, &str)> {
  let prefix = ["u8R\"", "uR\"", "UR\"", "LR\"", "R\""]
    .into_iter()
    .find(|prefix| token.starts_with(prefix))?;
  let opening = token[prefix.len()..].find('(')? + prefix.len();
  let delimiter = &token[prefix.len()..opening];
  if delimiter.len() > 16
    || !delimiter
      .bytes()
      .all(|b| (b'!'..=b'~').contains(&b) && !b"()\\".contains(&b))
  {
    return None;
  }
  let closing_len = delimiter.len() + 2;
  let payload_end = token.len().checked_sub(closing_len)?;
  let closing = token.get(payload_end..)?;
  if !closing.starts_with(')')
    || !closing.ends_with('"')
    || &closing[1..closing_len - 1] != delimiter
  {
    return None;
  }
  let payload = token.get(opening + 1..payload_end)?;
  if payload.contains(closing) {
    return None;
  }
  Some((&token[..opening + 1], payload, closing))
}

fn raw_newlines(payload: &str) -> impl Iterator<Item = u8> + '_ {
  let mut bytes = payload.bytes().peekable();
  std::iter::from_fn(move || {
    let byte = bytes.next()?;
    if byte == b'\r' && bytes.peek() == Some(&b'\n') {
      bytes.next()
    } else {
      Some(byte)
    }
  })
}

/// Original-source diagnostics/spans only. There is deliberately no dependency
/// or extraction identity: a finished compiler run is not a cache contract.
#[derive(Debug)]
pub struct CompilerRecoveryAudit {
  pub has_error: bool,
  pub eligible_names: Vec<String>,
  pub macro_spans: Vec<Range<usize>>,
  pub calls: Vec<(String, Range<usize>)>,
  pub member_calls: Vec<(String, Range<usize>)>,
  pub functions: Vec<String>,
  pub context_errors: Vec<Range<usize>>,
}

/// An original physical header buffer captured by the compiler observer.
/// The adapter must verify that these are the bytes associated with its records;
/// reading a newer header after compilation is not a captured buffer.
#[derive(Debug)]
pub struct DefinitionBuffer<'a> {
  pub path: &'a Path,
  pub source: &'a str,
}

/// A definition anchor in one captured physical buffer. Both offsets are byte
/// offsets: `name_offset` starts the name, `end` follows its final original token.
#[derive(Debug, Clone)]
pub struct DefinitionAnchor {
  pub buffer: usize,
  pub name_offset: usize,
  pub end: usize,
  pub parameters: usize,
}

/// A direct expansion in the original translation unit. Nested expansion names
/// belong in `expanded_names`; their spelling locations are not invocation sites.
#[derive(Debug, Clone)]
pub struct Expansion<'a> {
  pub name: &'a str,
  pub invocation: Range<usize>,
  /// None for command-line/builtin/object-like/otherwise unanchored definitions.
  pub definition: Option<DefinitionAnchor>,
}

/// One preprocess-only observation. This is deliberately not an ExtractionEnv
/// input or a reusable proof. No claim about compiler equivalence or dependency
/// completeness is inferred from `complete` (which only means the run finished).
#[derive(Debug)]
pub struct Observation<'a> {
  pub path: &'a Path,
  pub source: &'a str,
  pub definitions: &'a [DefinitionBuffer<'a>],
  pub expansions: &'a [Expansion<'a>],
  /// Every expansion rooted in the main file, including nested replacements and
  /// argument expansions. Do not filter this set to candidate statement names.
  pub expanded_names: &'a BTreeSet<String>,
  pub complete: bool,
  pub volatile_inputs: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declined {
  Incomplete,
  VolatileInputs,
  SourceMismatch,
  Limit,
  ConflictingBuffers,
  InvalidExpansion,
  InvalidDefinition,
}

/// Audit physical observations without exposing a recovered root or product.
/// Invalid/stale observations return a decline; unsupported replacements remain
/// ordinary raw syntax. Genuine argument/semicolon/context failures are retained.
pub fn audit(
  path: &Path,
  source: &str,
  observation: &Observation<'_>,
) -> Result<CompilerRecoveryAudit, Declined> {
  // Definition/argument proof parses must not inherit an outer scanner context.
  vorpal_language::with_cpp_statement_macros(&[], || {
    audit_without_context(path, source, observation)
  })
}

pub(crate) fn prepare_evidence(
  path: &Path,
  source: &str,
  observation: &Observation<'_>,
) -> Result<(Evidence, Vec<Range<usize>>), Declined> {
  if !observation.complete {
    return Err(Declined::Incomplete);
  }
  if observation.volatile_inputs {
    return Err(Declined::VolatileInputs);
  }
  if observation.path != path || observation.source != source {
    return Err(Declined::SourceMismatch);
  }
  if source.len() > 4 * 1024 * 1024
    || observation.definitions.len() > 128
    || observation.expansions.len() > 16384
    || observation.expanded_names.len() > 16384
  {
    return Err(Declined::Limit);
  }
  let mut name_bytes = 1024 * 1024usize;
  for name in observation.expanded_names {
    name_bytes = name_bytes.checked_sub(name.len()).ok_or(Declined::Limit)?;
  }
  let mut remaining = 4 * 1024 * 1024usize;
  let mut buffers = BTreeMap::from([(observation.path, observation.source)]);
  let mut parsed = Vec::with_capacity(observation.definitions.len());
  for buffer in observation.definitions {
    remaining = remaining
      .checked_sub(buffer.source.len())
      .ok_or(Declined::Limit)?;
    if let Some(previous) = buffers.insert(buffer.path, buffer.source)
      && previous != buffer.source
    {
      return Err(Declined::ConflictingBuffers);
    }
    parsed.push(SupportLang::Cpp.grep(buffer.source));
  }
  let roots: Vec<_> = parsed.iter().map(|parsed| parsed.root()).collect();
  let definition_nodes: BTreeMap<_, _> = roots
    .iter()
    .enumerate()
    .flat_map(|(buffer, root)| {
      root
        .dfs()
        .filter(|n| n.kind().as_ref() == "preproc_function_def")
        .filter_map(move |node| Some(((buffer, node.field("name")?.range().start), node)))
    })
    .collect();
  let mut templates = BTreeMap::new();
  let mut replacement_ends = BTreeMap::new();
  let mut evidence = Evidence {
    macro_names: observation.expanded_names.clone(),
    ..Evidence::default()
  };
  // Preprocessor operators are not necessarily MacroExpands callbacks. Their
  // argument/replacement effects are outside the independent statement proof.
  evidence
    .macro_names
    .extend(["_Pragma", "__pragma", "__VA_ARGS__", "__VA_OPT__"].map(str::to_owned));
  let mut ranges = Vec::with_capacity(observation.expansions.len());
  for expansion in observation.expansions {
    let span = &expansion.invocation;
    let name = expansion.name;
    if name.is_empty()
      || name.len() > 128
      || !name.as_bytes()[0].is_ascii_alphabetic() && !name.starts_with('_')
      || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
      || !evidence.macro_names.contains(name)
      || span.start >= span.end
      || source
        .get(span.clone())
        .is_none_or(|text| !text.starts_with(name))
      || source
        .as_bytes()
        .get(span.start + name.len())
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
      return Err(Declined::InvalidExpansion);
    }
    ranges.push(span.clone());
    let Some(anchor) = &expansion.definition else {
      continue;
    };
    let buffer = observation
      .definitions
      .get(anchor.buffer)
      .ok_or(Declined::InvalidDefinition)?;
    let definition = definition_nodes
      .get(&(anchor.buffer, anchor.name_offset))
      .ok_or(Declined::InvalidDefinition)?;
    if definition.field("name").is_none_or(|n| n.text() != name) {
      return Err(Declined::InvalidDefinition);
    }
    let values: Vec<_> = definition
      .children()
      .filter(|n| n.kind().as_ref() == "preproc_arg")
      .collect();
    let Some(first) = values.first() else {
      // An empty macro cannot be a complete statement.
      continue;
    };
    let last = values.last().ok_or(Declined::InvalidDefinition)?;
    let end = last.range().end;
    // Native MacroInfo ends at the last replacement token, not a trailing
    // comment included by preproc_arg. Independently tokenize the physical
    // replacement outside its opaque directive node; never trim comment-like
    // spelling inside literals or accept an endpoint in the middle of a token.
    let value = &buffer.source[first.range().start..end];
    let token_end = replacement_ends
      .entry((anchor.buffer, anchor.name_offset))
      .or_insert_with(|| {
        crate::cpp_macro_evidence::replacement_token_end(value)
          .map(|end| first.range().start + end)
      });
    let Some(token_end) = *token_end else {
      continue;
    };
    if anchor.end <= first.range().start || anchor.end != token_end {
      return Err(Declined::InvalidDefinition);
    }
    let parameter_node = definition
      .field("parameters")
      .ok_or(Declined::InvalidDefinition)?;
    let parameters: Vec<_> = parameter_node
      .children()
      .filter(|n| n.is_named())
      .map(|n| n.text().into_owned())
      .collect();
    // MacroInfo includes an implicit variadic parameter that has no named
    // identifier in the original AST. Unsupported signatures decline only this
    // definition; do not mistake their different count for a malformed anchor.
    if definition.has_error()
      || !parameter_node
        .children()
        .all(|n| n.kind().as_ref() == "identifier" || matches!(n.text().as_ref(), "(" | ")" | ","))
      || parameters.iter().collect::<BTreeSet<_>>().len() != parameters.len()
    {
      continue;
    }
    if parameters.len() != anchor.parameters {
      return Err(Declined::InvalidDefinition);
    }
    let template = templates
      .entry((anchor.buffer, anchor.name_offset))
      .or_insert_with(|| {
        let replacement = native_statement_replacement(value, &parameters)?;
        Some(StatementMacro {
          name: name.to_owned(),
          parameters: parameters.len(),
          definition_path: buffer.path.to_path_buf(),
          definition_span: definition.range(),
          replacement: Arc::new(replacement),
        })
      });
    let Some(template) = template else {
      continue;
    };
    evidence.bindings.push(Binding {
      definition: template.clone(),
      active: span.clone(),
    });
  }
  ranges.sort_by_key(|span| (span.start, span.end));
  if ranges.windows(2).any(|pair| pair[0].end > pair[1].start) {
    return Err(Declined::InvalidExpansion);
  }
  Ok((evidence, ranges))
}

fn audit_without_context(path: &Path, source: &str, observation: &Observation<'_>) -> Result<CompilerRecoveryAudit, Declined> {
  let (evidence, ranges) = prepare_evidence(path, source, observation)?;
  let (report, member_calls) = crate::cpp_macro_recovery::audit_compiler_evidence(source, evidence);
  // The site-scoped scanner must recover exactly captured original invocations,
  // not another occurrence hidden inside an overlong or malformed record range.
  if report.macro_spans.iter().any(|span| {
    ranges
      .binary_search_by_key(&(span.start, span.end), |s| (s.start, s.end))
      .is_err()
  }) {
    return Err(Declined::InvalidExpansion);
  }
  let expansion_starts: BTreeMap<_, _> = observation
    .expansions
    .iter()
    .map(|expansion| (expansion.invocation.start, expansion.name))
    .collect();
  Ok(CompilerRecoveryAudit {
    has_error: report.has_error,
    eligible_names: report.eligible_names,
    macro_spans: report.macro_spans,
    // Unsupported macros still expand. Suppress only a callee whose original
    // site is observed, retaining argument calls and ordinary same-name calls
    // after undef. No expansion or source masking is used to invent a target.
    calls: report
      .calls
      .into_iter()
      .filter(|(name, span)| {
        expansion_starts
          .get(&span.start)
          .is_none_or(|expanded| *expanded != name)
      })
      .collect(),
    member_calls: member_calls
      .into_iter()
      .filter(|(callee, span)| {
        let Some(end) = span.start.checked_add(callee.len()) else {
          return false;
        };
        expansion_starts.range(span.start..end).next().is_none()
      })
      .collect(),
    functions: report
      .functions
      .into_iter()
      .filter(|name| !observation.expanded_names.contains(name))
      .collect(),
    context_errors: report.context_errors,
  })
}
