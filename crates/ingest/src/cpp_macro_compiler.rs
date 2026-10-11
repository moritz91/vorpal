//! Fresh, explicitly configured native compiler observations. There is no replay
//! contract: callers must run the provider again before serving compiler products.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[cfg(feature = "builtin-parser")]
#[path = "cpp_macro_compiler_process.rs"]
mod process_tree;

/// Trusted preprocess-only provider. No shell is involved. The provider receives
/// file paths in VORPAL_CPP_COMPILER_REQUEST/RESPONSE. Relative paths are
/// resolved by the launcher against the project configuration directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompilerCommand {
  pub program: PathBuf,
  #[serde(default)]
  pub arguments: Vec<String>,
  pub directory: PathBuf,
  /// Explicit physical translation units; other files can use independent
  /// metadata recovery when include roots are configured.
  pub translation_units: Vec<PathBuf>,
  #[serde(default = "default_timeout")]
  pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
  60
}

impl CompilerCommand {
  pub fn validate(&self) -> Result<(), String> {
    if !cfg!(feature = "builtin-parser") {
      return Err("C++ compiler recovery requires builtin-parser".into());
    }
    if !self.program.is_absolute()
      || !self.directory.is_absolute()
      || !(1..=300).contains(&self.timeout_seconds)
      || self.translation_units.is_empty()
      || self.translation_units.len() > 16384
      || self.translation_units.iter().any(|p| !p.is_absolute())
      || self.arguments.len() > 128
      || self.arguments.iter().map(String::len).sum::<usize>() > 65536
    {
      return Err(
        "invalid C++ compiler command: absolute paths and a 1..300 second timeout are required"
          .into(),
      );
    }
    Ok(())
  }
}

#[cfg(feature = "builtin-parser")]
mod native {
  use super::*;
  use crate::cpp_macro_compiler_audit::{
    DefinitionAnchor, DefinitionBuffer, Expansion, Observation, TokenAgreement,
    compare_token_spellings,
  };
  use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    ops::Range,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
  };

  const LIMIT: u64 = 32 * 1024 * 1024;

  fn scratch_within_limits(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
      return false;
    };
    let mut total = 0u64;
    for (index, entry) in entries.enumerate() {
      let Ok(entry) = entry else {
        return false;
      };
      let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
        return false;
      };
      let name = entry.file_name();
      let limit = if name == "response.json" || name == "stderr.log" {
        LIMIT
      } else {
        4 * LIMIT
      };
      if index >= 128 || !metadata.is_file() || metadata.len() > limit {
        return false;
      }
      total += metadata.len();
      if total > 16 * LIMIT {
        return false;
      }
    }
    true
  }

  #[derive(Serialize)]
  #[serde(rename_all = "camelCase")]
  struct Request<'a> {
    version: u32,
    request_id: &'a str,
    path: &'a Path,
    source: &'a str,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "camelCase", deny_unknown_fields)]
  struct Buffer {
    path: PathBuf,
    source: String,
  }
  #[derive(Deserialize)]
  #[serde(rename_all = "camelCase", deny_unknown_fields)]
  struct Anchor {
    buffer: usize,
    name_offset: usize,
    end: usize,
    parameters: usize,
  }
  #[derive(Deserialize)]
  #[serde(rename_all = "camelCase", deny_unknown_fields)]
  struct Site {
    name: String,
    start: usize,
    end: usize,
    definition: Option<Anchor>,
  }
  #[derive(Deserialize)]
  #[serde(rename_all = "camelCase", deny_unknown_fields)]
  struct Callee {
    name: String,
    start: usize,
  }
  #[derive(Deserialize)]
  #[serde(rename_all = "camelCase", deny_unknown_fields)]
  struct TokenSite {
    offset: usize,
    from_macro: bool,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "camelCase", deny_unknown_fields)]
  struct Packet {
    version: u32,
    request_id: String,
    path: PathBuf,
    source: String,
    complete: bool,
    volatile_inputs: bool,
    native_context_before: String,
    native_context_after: String,
    observed_context: String,
    native_before: Vec<String>,
    native_after: Vec<String>,
    observed_tokens: Vec<String>,
    #[serde(default)]
    observed_token_sites: Vec<TokenSite>,
    native_directives: Vec<String>,
    #[serde(default)]
    pragma_offsets: Vec<usize>,
    definitions: Vec<Buffer>,
    expansions: Vec<Site>,
    #[serde(default)]
    literal_arguments: Vec<Site>,
    #[serde(default)]
    specifier_macros: Vec<Site>,
    #[serde(default)]
    control_macros: Vec<Site>,
    #[serde(default)]
    annotation_macros: Vec<Site>,
    expanded_names: BTreeSet<String>,
    callee_sites: Vec<Callee>,
  }

  pub(crate) struct CompilerParse {
    pub root: crate::ParsedRoot,
    pub diagnostics: crate::cpp_macro_recovery::ContextDiagnostics,
    pub dependency: u64,
    /// Definition names physically supplied by an expansion are not original
    /// source definitions. Exact positions avoid suppressing later ordinary names.
    pub definition_sites: Vec<Range<usize>>,
  }

  fn current(path: &Path, source: &str) -> bool {
    path.is_absolute()
      && !crate::cpp_macro_evidence::path_has_redirected_components(path)
      && fs::File::open(path).is_ok_and(|file| {
        let mut bytes = Vec::new();
        file
          .take(source.len() as u64 + 1)
          .read_to_end(&mut bytes)
          .is_ok()
          && bytes == source.as_bytes()
      })
  }

  fn validated_native_pragmas(packet: &Packet, source: &str) -> bool {
    use vorpal_core::tree_sitter::LanguageExt;
    use vorpal_language::SupportLang;
    fn compact(text: &str) -> String {
      let mut result = String::new();
      let mut quoted = false;
      let mut escaped = false;
      for c in text.chars() {
        if quoted || !c.is_ascii_whitespace() {
          result.push(c);
        }
        if escaped {
          escaped = false;
        } else if quoted && c == '\\' {
          escaped = true;
        } else if c == '"' {
          quoted = !quoted;
        }
      }
      result
    }
    fn approved(text: &str) -> bool {
      matches!(
        text,
        "#pragmawarning(push)"
          | "#pragmawarning(pop)"
          | "#pragmaoptimize(\"\",off)"
          | "#pragmaoptimize(\"\",on)"
      ) || text
        .strip_prefix("#pragmawarning(disable:")
        .and_then(|s| s.strip_suffix(')'))
        .is_some_and(|s| !s.is_empty() && s.len() <= 5 && s.bytes().all(|b| b.is_ascii_digit()))
    }
    if packet.pragma_offsets.len() != packet.native_directives.len()
      || packet.pragma_offsets.len() > 16384
      || packet.pragma_offsets.windows(2).any(|p| p[0] >= p[1])
    {
      return false;
    }
    if packet.native_directives.is_empty() {
      return true;
    }
    let parsed = SupportLang::Cpp.grep(source);
    let directives: std::collections::BTreeMap<_, _> = parsed
      .root()
      .dfs()
      .filter(|n| {
        n.kind().as_ref() == "preproc_call"
          && n.field("directive").is_some_and(|d| d.text() == "#pragma")
      })
      .map(|n| (n.range().start, n))
      .collect();
    packet
      .pragma_offsets
      .iter()
      .zip(&packet.native_directives)
      .all(|(offset, native)| {
        let Some(node) = directives.get(offset) else {
          return false;
        };
        let Some(argument) = node.field("argument") else {
          return false;
        };
        let text = argument.text();
        let Some(end) = crate::cpp_macro_evidence::replacement_token_end(&text) else {
          return false;
        };
        let authored = compact(&format!("#pragma{}", &text[..end]));
        approved(&authored) && authored == compact(native)
      })
  }

  fn capture(config: &CompilerCommand, path: &Path, source: &str) -> Option<(Packet, u64)> {
    config.validate().ok()?;
    if source.len() > 4 * 1024 * 1024 || !current(path, source) {
      return None;
    }
    let scratch = tempfile::Builder::new()
      .prefix("vorpal-cpp-compiler-")
      .tempdir()
      .ok()?;
    let request_id = scratch.path().file_name()?.to_str()?;
    let request = scratch.path().join("request.json");
    let response = scratch.path().join("response.json");
    let errors = scratch.path().join("stderr.log");
    let ready = scratch.path().join("ready");
    fs::write(
      &request,
      serde_json::to_vec(&Request {
        version: 1,
        request_id,
        path,
        source,
      })
      .ok()?,
    )
    .ok()?;
    let mut command = Command::new(&config.program);
    command
      .args(&config.arguments)
      .env("VORPAL_CPP_COMPILER_REQUEST", &request)
      .env("VORPAL_CPP_COMPILER_RESPONSE", &response)
      .env("VORPAL_CPP_COMPILER_READY", &ready)
      .current_dir(&config.directory)
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::from(fs::File::create(&errors).ok()?));
    #[cfg(windows)]
    {
      use std::os::windows::process::CommandExt;
      command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    #[cfg(unix)]
    {
      use std::os::unix::process::CommandExt;
      command.process_group(0);
    }
    let mut child = command.spawn().ok()?;
    let Some(tree) = super::process_tree::ProcessTree::attach(&child) else {
      let _ = child.kill();
      let _ = child.wait();
      return None;
    };
    // Providers wait for this before starting preprocessing children. The job/
    // process group owns them before any compiler is allowed to start.
    if fs::write(ready, b"ready").is_err() {
      drop(tree);
      let _ = child.kill();
      let _ = child.wait();
      return None;
    }
    let deadline = Instant::now() + Duration::from_secs(config.timeout_seconds);
    let status = loop {
      match child.try_wait() {
        Ok(Some(status)) => break Some(status),
        Ok(None) if Instant::now() < deadline && scratch_within_limits(scratch.path()) => {
          std::thread::sleep(Duration::from_millis(20));
        }
        _ => {
          let _ = child.kill();
          let _ = child.wait();
          break None;
        }
      }
    }?;
    drop(tree);
    if !status.success() || !scratch_within_limits(scratch.path()) {
      return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(response)
      .ok()?
      .take(LIMIT + 1)
      .read_to_end(&mut bytes)
      .ok()?;
    if bytes.len() as u64 > LIMIT || !current(path, source) {
      return None;
    }
    let packet: Packet = serde_json::from_slice(&bytes).ok()?;
    if packet.version != 1
      || packet.request_id != request_id
      || packet.path != path
      || packet.source != source
      || !packet.complete
      || packet.volatile_inputs
      || !validated_native_pragmas(&packet, source)
      || packet.native_context_before.is_empty()
      || packet.observed_context.is_empty()
      || packet.native_context_before != packet.native_context_after
      || packet.native_before != packet.native_after
      || packet.native_before.len() > 1_000_000
      || packet.observed_tokens.len() > 1_000_000
    {
      return None;
    }
    let native: Vec<_> = packet.native_before.iter().map(String::as_str).collect();
    let observed: Vec<_> = packet.observed_tokens.iter().map(String::as_str).collect();
    if compare_token_spellings(&native, &observed) == TokenAgreement::Different {
      return None;
    }
    // This identifies this exact capture, not reusable dependency completeness.
    let mut hash = xxhash_rust::xxh3::Xxh3::new();
    hash.update(b"vorpal-fresh-cpp-compiler-v1\0");
    hash.update(&serde_json::to_vec(config).ok()?);
    let mut stable: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    stable.as_object_mut()?.remove("requestId");
    hash.update(&serde_json::to_vec(&stable).ok()?);
    Some((packet, hash.digest()))
  }

  pub(crate) fn parse(
    config: &CompilerCommand,
    path: &Path,
    source: &str,
  ) -> Option<CompilerParse> {
    vorpal_language::with_cpp_statement_macros(&[], || parse_without_context(config, path, source))
  }

  fn add_native_generators(
    source: &str,
    packet: &Packet,
    evidence: &mut crate::cpp_macro_evidence::Evidence,
  ) -> Option<()> {
    use crate::cpp_macro_evidence::{Binding, StatementMacro, native_generator_statement};
    use vorpal_core::tree_sitter::LanguageExt;
    if packet.observed_token_sites.is_empty() {
      return Some(());
    }
    if packet.observed_token_sites.len() != packet.observed_tokens.len()
      || packet
        .observed_token_sites
        .iter()
        .any(|s| s.offset >= source.len() || !source.is_char_boundary(s.offset))
    {
      return None;
    }
    let raw = vorpal_language::SupportLang::Cpp.grep(source);
    let root = raw.root();
    let calls: std::collections::BTreeMap<_, _> = root
      .dfs()
      .filter(|n| n.kind().as_ref() == "call_expression")
      .filter_map(|n| Some((n.field("function")?.range().start, n)))
      .collect();
    let mut origins: std::collections::BTreeMap<usize, Vec<usize>> =
      std::collections::BTreeMap::new();
    for (i, site) in packet.observed_token_sites.iter().enumerate() {
      origins.entry(site.offset).or_default().push(i);
    }
    let mut definition_spans: std::collections::BTreeMap<(usize, usize), Option<Range<usize>>> =
      std::collections::BTreeMap::new();
    let proven: BTreeSet<_> = evidence.bindings.iter().map(|b| b.active.start).collect();
    for expansion in &packet.expansions {
      if proven.contains(&expansion.start) {
        continue;
      }
      let Some(anchor) = &expansion.definition else {
        continue;
      };
      let Some(call) = calls.get(&expansion.start).filter(|n| {
        n.field("function")
          .is_some_and(|f| f.text() == expansion.name)
          && n
            .field("arguments")
            .is_some_and(|a| a.range().end == expansion.end)
      }) else {
        continue;
      };
      // A statement list is admitted only at an original direct block position.
      // It cannot be substituted as one statement inside an if/loop/else arm.
      let mut position = call.clone();
      let mut direct = false;
      for _ in 0..64 {
        let Some(parent) = position.parent() else {
          break;
        };
        if parent.kind().as_ref() == "comma_expression"
          && parent.range().start == expansion.start
          && parent
            .children()
            .filter(|n| n.kind().as_ref() == ",")
            .all(|n| n.range().is_empty())
        {
          // Raw error recovery can insert a missing comma before the following
          // ordinary statement. Actual authored commas are expression contexts.
          position = parent;
          continue;
        }
        if parent.kind().as_ref() == "assignment_expression"
          && parent.range().start == expansion.start
          && parent.has_error()
          && parent
            .field("left")
            .is_some_and(|n| n.range() == position.range())
          && crate::cpp_macro_recovery::invocation_spacing(source.as_bytes(), expansion.end)
            .and_then(|next| source.as_bytes().get(next))
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
          // Raw recovery can absorb the following authored assignment into the
          // macro call's left operand. Require a separate identifier after the
          // exact invocation, never a real operator continuing that call.
          position = parent;
          continue;
        }
        direct = parent.kind().as_ref() == "expression_statement"
          && parent.range().start == expansion.start
          && parent
            .parent()
            .is_some_and(|b| b.kind().as_ref() == "compound_statement");
        break;
      }
      if !direct {
        continue;
      }
      let arguments = crate::cpp_macro_recovery::validated_arguments(
        &call.field("arguments")?.text(),
        anchor.parameters,
      )?;
      let callback_arguments = !arguments.is_empty()
        && arguments.iter().all(|a| {
          a.as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
            && a.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        && arguments.iter().any(|a| packet.expanded_names.contains(a));
      let argument_node = call.field("arguments")?;
      // Atom/array arguments contain no runtime calls or side effects to lose
      // when a native list ignores, repeats or reorders them. Expanding names
      // and all other expression forms require a separate evaluation proof.
      let array_arguments = !arguments.is_empty()
        && !argument_node.has_error()
        && argument_node.dfs().filter(|n| n.is_named()).all(|n| {
          matches!(
            n.kind().as_ref(),
            "argument_list"
              | "identifier"
              | "number_literal"
              | "subscript_expression"
              | "subscript_argument_list"
          ) && (n.kind().as_ref() != "identifier"
            || !packet.expanded_names.contains(n.text().as_ref()))
        });
      if !callback_arguments && !array_arguments {
        continue;
      }
      let mut positions = origins.range(expansion.start..expansion.end);
      let Some((&offset, indices)) = positions.next() else {
        continue;
      };
      if offset != expansion.start || positions.next().is_some() {
        return None;
      }
      let (Some(&first), Some(&last)) = (indices.first(), indices.last()) else {
        continue;
      };
      if last - first + 1 != indices.len()
        || indices.iter().any(|&i| {
          let s = &packet.observed_token_sites[i];
          !s.from_macro || s.offset != expansion.start
        })
      {
        return None;
      }
      let mut proof = String::from("{ ");
      for token in &packet.observed_tokens[first..=last] {
        if proof.len().checked_add(token.len())?.checked_add(3)? > 4 * 1024 * 1024 {
          return None;
        }
        proof.push_str(token);
        proof.push(' ');
      }
      proof.push('}');
      let Some(replacement) = native_generator_statement(proof, arguments) else {
        continue;
      };
      let buffer = packet.definitions.get(anchor.buffer)?;
      let definition_span = definition_spans
        .entry((anchor.buffer, anchor.name_offset))
        .or_insert_with(|| {
          let parsed = vorpal_language::SupportLang::Cpp.grep(&buffer.source);
          let root = parsed.root();
          let node = root
            .dfs()
            .find(|n| {
              n.kind().as_ref() == "preproc_function_def"
                && n
                  .field("name")
                  .is_some_and(|name| name.range().start == anchor.name_offset)
            });
          node.map(|n| n.range())
        })
        .as_ref()?
        .clone();
      evidence.bindings.push(Binding {
        active: expansion.start..expansion.end,
        definition: StatementMacro {
          name: expansion.name.clone(),
          parameters: anchor.parameters,
          definition_path: buffer.path.clone(),
          definition_span,
          replacement: std::sync::Arc::new(replacement),
        },
      });
    }
    Some(())
  }

  fn refine_native_literal_arguments(
    source: &str,
    packet: &Packet,
    evidence: &mut crate::cpp_macro_evidence::Evidence,
  ) -> Option<()> {
    use vorpal_core::tree_sitter::LanguageExt;
    use vorpal_language::SupportLang;
    if packet.literal_arguments.is_empty() {
      return Some(());
    }
    if packet.literal_arguments.len() > 16384 {
      return None;
    }
    let parsed = SupportLang::Cpp.grep(source);
    let root = parsed.root();
    let identifiers: BTreeSet<_> = root
      .dfs()
      .filter(|n| n.kind().as_ref() == "identifier")
      .map(|n| (n.range().start, n.range().end))
      .collect();
    let mut literals = std::collections::BTreeMap::new();
    let mut templates = std::collections::BTreeMap::new();
    for site in &packet.literal_arguments {
      let anchor = site.definition.as_ref()?;
      if anchor.parameters != 0
        || !identifiers.contains(&(site.start, site.end))
        || source.get(site.start..site.end)? != site.name
        || !packet.expanded_names.contains(&site.name)
        || !packet
          .callee_sites
          .iter()
          .any(|c| c.start == site.start && c.name == site.name)
      {
        return None;
      }
      let buffer = packet.definitions.get(anchor.buffer)?;
      let value = templates
        .entry((
          anchor.buffer,
          anchor.name_offset,
          anchor.end,
          site.name.clone(),
        ))
        .or_insert_with(|| {
          let parsed = SupportLang::Cpp.grep(&buffer.source);
          let definition = parsed.root().dfs().find(|n| {
            n.kind().as_ref() == "preproc_def"
              && n
                .field("name")
                .is_some_and(|n| n.range().start == anchor.name_offset && n.text() == site.name)
          })?;
          let value = definition.field("value")?;
          let text = value.text().trim().to_owned();
          if value.range().start.checked_add(text.len())? != anchor.end || text.len() > 128 {
            return None;
          }
          let proof = SupportLang::Cpp.grep(format!("void proof() {{ auto value = ({text}); }}"));
          let root = proof.root();
          let values: Vec<_> = root
            .dfs()
            .filter(|n| n.kind().as_ref() == "number_literal")
            .collect();
          (!root.has_error() && values.len() == 1 && values[0].text() == text).then_some(text)
        })
        .as_ref()?
        .clone();
      if literals.insert(site.start, (site.end, value)).is_some() {
        return None;
      }
    }
    if literals
      .iter()
      .zip(literals.iter().skip(1))
      .any(|((_, (end, _)), (start, _))| end > start)
    {
      return None;
    }
    let calls: std::collections::BTreeMap<_, _> = root
      .dfs()
      .filter(|n| n.kind().as_ref() == "call_expression")
      .filter_map(|n| Some((n.field("function")?.range().start, n)))
      .collect();
    // Work from the independently proven direct template. A numeric object-like
    // expansion cannot stringify/ignore calls or manufacture unevaluated syntax.
    // All other nested effects stay opaque, even if their final syntax is valid.
    for index in 0..evidence.bindings.len() {
      let binding = &evidence.bindings[index];
      if binding.definition.replacement.native_arguments.is_some() {
        continue;
      }
      let Some(call) = calls.get(&binding.active.start) else {
        continue;
      };
      let Some(arguments) = call
        .field("arguments")
        .filter(|n| n.range().end == binding.active.end)
      else {
        continue;
      };
      let range = arguments.range();
      let parts: Vec<_> = literals.range(range.clone()).collect();
      if parts.is_empty() {
        continue;
      }
      let mut proof_arguments = String::new();
      let mut cursor = range.start;
      for (&start, (end, value)) in parts {
        if *end > range.end || start < cursor {
          return None;
        }
        proof_arguments.push_str(source.get(cursor..start)?);
        proof_arguments.push_str(value);
        cursor = *end;
      }
      proof_arguments.push_str(source.get(cursor..range.end)?);
      if evidence.contains_expanding_tokens(&proof_arguments) {
        continue;
      }
      let original = crate::cpp_macro_recovery::validated_arguments(
        &arguments.text(),
        binding.definition.parameters,
      )?;
      let expanded = crate::cpp_macro_recovery::validated_arguments(
        &proof_arguments,
        binding.definition.parameters,
      )?;
      let Some(proof) = binding
        .definition
        .replacement
        .instantiate(&expanded.iter().map(String::as_str).collect::<Vec<_>>())
      else {
        continue;
      };
      if evidence.contains_expanding_tokens(&proof) {
        continue;
      }
      if let Some(replacement) = crate::cpp_macro_evidence::native_literal_statement(
        &binding.definition.replacement,
        proof,
        original,
      ) {
        evidence.bindings[index].definition.replacement = std::sync::Arc::new(replacement);
      }
    }
    Some(())
  }

  fn object_prefix_sites(
    packet: &Packet,
    source: &str,
  ) -> Option<Vec<vorpal_language::CppProvenMacroSite>> {
    use vorpal_core::tree_sitter::LanguageExt;
    use vorpal_language::{CppProvenMacroKind, CppProvenMacroSite, SupportLang};
    if packet
      .specifier_macros
      .len()
      .checked_add(packet.control_macros.len())?
      .checked_add(packet.annotation_macros.len())?
      > 16384
    {
      return None;
    }
    if packet.specifier_macros.is_empty()
      && packet.control_macros.is_empty()
      && packet.annotation_macros.is_empty()
    {
      return Some(Vec::new());
    }
    let mut origins: std::collections::BTreeMap<usize, Vec<usize>> =
      std::collections::BTreeMap::new();
    for (i, site) in packet.observed_token_sites.iter().enumerate() {
      origins.entry(site.offset).or_default().push(i);
    }
    let mut definitions = std::collections::BTreeMap::new();
    let mut sites = Vec::new();
    for (site, role) in packet
      .specifier_macros
      .iter()
      .map(|s| (s, 0))
      .chain(packet.control_macros.iter().map(|s| (s, 1)))
      .chain(packet.annotation_macros.iter().map(|s| (s, 2)))
    {
      let anchor = site.definition.as_ref()?;
      if anchor.parameters != 0
        || site.start.checked_add(site.name.len())? != site.end
        || source.get(site.start..site.end)? != site.name
        || !packet.expanded_names.contains(&site.name)
        || !packet
          .callee_sites
          .iter()
          .any(|c| c.start == site.start && c.name == site.name)
        || !packet.expansions.iter().any(|e| {
          e.start == site.start
            && e.end == site.end
            && e.name == site.name
            && e.definition.is_none()
        })
      {
        return None;
      }
      let buffer = packet.definitions.get(anchor.buffer)?;
      let value = definitions
        .entry((
          anchor.buffer,
          anchor.name_offset,
          anchor.end,
          site.name.clone(),
        ))
        .or_insert_with(|| {
          let parsed = SupportLang::Cpp.grep(&buffer.source);
          let definition = parsed.root().dfs().find(|n| {
            n.kind().as_ref() == "preproc_def"
              && n
                .field("name")
                .is_some_and(|n| n.range().start == anchor.name_offset && n.text() == site.name)
          })?;
          let Some(value) = definition.field("value") else {
            return (role == 2 && definition.field("name")?.range().end == anchor.end)
              .then(String::new);
          };
          let text = value.text();
          let length = crate::cpp_macro_evidence::replacement_token_end(&text)?;
          if value.range().start.checked_add(length)? != anchor.end {
            return None;
          }
          let text = text.get(..length)?.trim();
          Some(text.to_owned())
        })
        .as_ref()?;
      let (kind, tokens): (_, Vec<&str>) = if role == 2 {
        match value
          .chars()
          .filter(|c| !c.is_ascii_whitespace())
          .collect::<String>()
          .as_str()
        {
          "" => (CppProvenMacroKind::Annotation, vec![]),
          "__pragma(warning(push))" => (
            CppProvenMacroKind::Annotation,
            vec!["__pragma", "(", "warning", "(", "push", ")", ")"],
          ),
          "__pragma(warning(pop))" => (
            CppProvenMacroKind::Annotation,
            vec!["__pragma", "(", "warning", "(", "pop", ")", ")"],
          ),
          _ => return None,
        }
      } else if role == 1 {
        match value
          .chars()
          .filter(|c| !c.is_ascii_whitespace())
          .collect::<String>()
          .as_str()
        {
          "try" => (CppProvenMacroKind::TryPrefix, vec!["try"]),
          "catch(...)" => (
            CppProvenMacroKind::CatchAllPrefix,
            vec!["catch", "(", "...", ")"],
          ),
          _ => return None,
        }
      } else if matches!(value.as_str(), "inline" | "__forceinline" | "__inline") {
        (CppProvenMacroKind::InlineSpecifier, vec![value.as_str()])
      } else {
        return None;
      };
      if tokens.is_empty() {
        if origins.range(site.start..site.end).next().is_some() {
          return None;
        }
        sites.push(CppProvenMacroSite {
          offset: u32::try_from(site.start).ok()?,
          name: site.name.clone(),
          kind,
        });
        continue;
      }
      let mut offsets = origins.range(site.start..site.end);
      let (&offset, indices) = offsets.next()?;
      if offsets.next().is_some()
        || offset != site.start
        || indices.len() != tokens.len()
        || indices.last()?.checked_sub(indices[0])?.checked_add(1)? != indices.len()
        || indices.iter().zip(tokens).any(|(&i, token)| {
          !packet.observed_token_sites[i].from_macro
            || packet
              .observed_tokens
              .get(i)
              .is_none_or(|observed| observed != token)
        })
      {
        return None;
      }
      // Object prefixes consume only their original name; calls and authored
      // declaration/handler bodies remain ordinary syntax under an exact site.
      sites.push(CppProvenMacroSite {
        offset: u32::try_from(site.start).ok()?,
        name: site.name.clone(),
        kind,
      });
    }
    Some(sites)
  }

  fn typed_handler_sites(
    packet: &Packet,
    source: &str,
  ) -> Option<Vec<vorpal_language::CppProvenMacroSite>> {
    use vorpal_core::tree_sitter::LanguageExt;
    use vorpal_language::{CppProvenMacroKind, CppProvenMacroSite, SupportLang};
    let mut origins: std::collections::BTreeMap<usize, Vec<usize>> =
      std::collections::BTreeMap::new();
    for (i, site) in packet.observed_token_sites.iter().enumerate() {
      origins.entry(site.offset).or_default().push(i);
    }
    let mut result = Vec::new();
    for expansion in &packet.expansions {
      let Some(anchor) = &expansion.definition else {
        continue;
      };
      let Some(indices) = origins.get(&expansion.start) else {
        continue;
      };
      if packet
        .observed_tokens
        .get(indices[0])
        .is_none_or(|s| s != "catch")
      {
        continue;
      }
      let buffer = packet.definitions.get(anchor.buffer)?;
      let parsed = SupportLang::Cpp.grep(&buffer.source);
      let definition = parsed.root().dfs().find(|n| {
        n.kind().as_ref() == "preproc_function_def"
          && n
            .field("name")
            .is_some_and(|n| n.range().start == anchor.name_offset && n.text() == expansion.name)
      })?;
      let parameters = definition.field("parameters")?.text();
      let parameter = parameters.strip_prefix('(')?.strip_suffix(')')?.trim();
      if anchor.parameters != 1
        || parameter.is_empty()
        || !parameter
          .bytes()
          .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || !parameter.as_bytes()[0].is_ascii_alphabetic() && !parameter.starts_with('_')
      {
        return None;
      }
      let value = definition.field("value")?;
      let text = value.text();
      let end = crate::cpp_macro_evidence::replacement_token_end(&text)?;
      if value.range().start.checked_add(end)? != anchor.end
        || text[..end]
          .chars()
          .filter(|c| !c.is_ascii_whitespace())
          .collect::<String>()
          != format!("catch({parameter})")
      {
        return None;
      }
      let name_end = expansion.start.checked_add(expansion.name.len())?;
      if source.get(expansion.start..name_end)? != expansion.name {
        return None;
      }
      let argument_start =
        crate::cpp_macro_recovery::invocation_spacing(source.as_bytes(), name_end)?;
      let arguments = source.get(argument_start..expansion.end)?;
      // Parse only the original parameter spelling for proof. The production tree
      // always reads the unchanged source, never this ephemeral declaration.
      let proof_text = format!("void proof{arguments};");
      let proof = SupportLang::Cpp.grep(&proof_text);
      let parameters = proof
        .root()
        .dfs()
        .find(|n| n.kind().as_ref() == "parameter_list")?;
      let declarations: Vec<_> = parameters
        .children()
        .filter(|n| n.is_named() && n.kind().as_ref() != "comment")
        .collect();
      if proof.root().has_error()
        || declarations.len() != 1
        || declarations[0].kind().as_ref() != "parameter_declaration"
      {
        return None;
      }
      let mut tokens = vec!["catch".to_owned()];
      tokens.extend(
        parameters
          .dfs()
          .filter(|n| n.children().next().is_none() && n.kind().as_ref() != "comment")
          .map(|n| n.text().into_owned()),
      );
      if indices.len() != tokens.len()
        || indices.last()?.checked_sub(indices[0])?.checked_add(1)? != indices.len()
        || origins.range(name_end..expansion.end).next().is_some()
        || indices.iter().zip(tokens).any(|(&i, token)| {
          !packet.observed_token_sites[i].from_macro || packet.observed_tokens[i] != token
        })
      {
        return None;
      }
      result.push(CppProvenMacroSite {
        offset: u32::try_from(expansion.start).ok()?,
        name: expansion.name.clone(),
        kind: CppProvenMacroKind::CatchParameterPrefix,
      });
    }
    Some(result)
  }

  fn function_prefix_sites(
    packet: &Packet,
    root: &crate::ParsedRoot,
  ) -> Option<Vec<vorpal_language::CppProvenMacroSite>> {
    use vorpal_core::tree_sitter::LanguageExt;
    use vorpal_language::{CppProvenMacroKind, CppProvenMacroSite, SupportLang};
    if packet.observed_token_sites.is_empty() {
      return Some(Vec::new());
    }
    let node = root.root();
    let mut candidates = std::collections::BTreeMap::new();
    for owner in node
      .dfs()
      .filter(|n| n.kind().as_ref() == "function_definition")
    {
      if !owner
        .parent()
        .is_some_and(|n| matches!(n.kind().as_ref(), "translation_unit" | "declaration_list"))
      {
        continue;
      }
      let Some(declarator) = owner
        .field("declarator")
        .filter(|n| n.kind().as_ref() == "function_declarator")
      else {
        continue;
      };
      let Some(name) = declarator
        .field("declarator")
        .filter(|n| n.kind().as_ref() == "identifier" && n.range().start == owner.range().start)
      else {
        continue;
      };
      let Some(arguments) = declarator.field("parameters") else {
        continue;
      };
      if !owner
        .field("body")
        .is_some_and(|n| n.kind().as_ref() == "compound_statement")
      {
        continue;
      }
      candidates.insert(name.range().start, (name.text().into_owned(), arguments));
    }
    // A literal argument cannot be mistaken for a raw function parameter.
    // Preserve the exact top-level call and the immediately following authored
    // body; only the fresh native prefix proof can join them into a definition.
    for call in node
      .dfs()
      .filter(|n| n.kind().as_ref() == "call_expression")
    {
      let Some(error) = call
        .parent()
        .filter(|n| n.kind().as_ref() == "ERROR" && n.range() == call.range())
      else {
        continue;
      };
      let Some(parent) = error
        .parent()
        .filter(|n| matches!(n.kind().as_ref(), "translation_unit" | "declaration_list"))
      else {
        continue;
      };
      let next = parent.children().find(|n| {
        n.is_named() && n.kind().as_ref() != "comment" && n.range().start >= error.range().end
      });
      if !next.is_some_and(|n| n.kind().as_ref() == "compound_statement") {
        continue;
      }
      let Some(name) = call
        .field("function")
        .filter(|n| n.kind().as_ref() == "identifier")
      else {
        continue;
      };
      let Some(arguments) = call.field("arguments") else {
        continue;
      };
      candidates.insert(name.range().start, (name.text().into_owned(), arguments));
    }
    let mut origins: std::collections::BTreeMap<usize, Vec<usize>> =
      std::collections::BTreeMap::new();
    for (i, site) in packet.observed_token_sites.iter().enumerate() {
      origins.entry(site.offset).or_default().push(i);
    }
    let mut result = Vec::new();
    for expansion in &packet.expansions {
      let Some(anchor) = &expansion.definition else {
        continue;
      };
      let Some((name, arguments)) = candidates.get(&expansion.start) else {
        continue;
      };
      if name != &expansion.name || arguments.range().end != expansion.end {
        continue;
      }
      let mut positions = origins.range(expansion.start..expansion.end);
      let Some((&offset, indices)) = positions.next() else {
        continue;
      };
      // Native handler keywords cannot authorize a function-generator role.
      if packet
        .observed_tokens
        .get(indices[0])
        .is_some_and(|s| matches!(s.as_str(), "catch" | "try"))
      {
        continue;
      }
      if offset != expansion.start || positions.next().is_some() {
        return None;
      }
      let (Some(&first), Some(&last)) = (indices.first(), indices.last()) else {
        continue;
      };
      if last - first + 1 != indices.len()
        || indices.iter().any(|&i| {
          let s = &packet.observed_token_sites[i];
          !s.from_macro || s.offset != expansion.start
        })
      {
        return None;
      }
      let mut prefix = String::new();
      for token in &packet.observed_tokens[first..=last] {
        if prefix.len().checked_add(token.len())?.checked_add(4)? > 4 * 1024 * 1024 {
          return None;
        }
        prefix.push_str(token);
        prefix.push(' ');
      }
      prefix.push_str("{}");
      let parsed = SupportLang::Cpp.grep(&prefix);
      let proof = parsed.root();
      let children: Vec<_> = proof
        .children()
        .filter(|n| n.is_named() && n.kind().as_ref() != "comment")
        .collect();
      if proof.has_error()
        || children.len() != 1
        || children[0].kind().as_ref() != "function_definition"
        || proof
          .dfs()
          .filter(|n| n.kind().as_ref() == "function_definition")
          .count()
          != 1
      {
        continue;
      }
      // Typed handler arguments are not function-generator arguments. Validate
      // arity/syntax only after the independent token proof establishes this role.
      crate::cpp_macro_recovery::validated_arguments(&arguments.text(), anchor.parameters)?;
      result.push(CppProvenMacroSite {
        offset: u32::try_from(expansion.start).ok()?,
        name: expansion.name.clone(),
        kind: CppProvenMacroKind::FunctionPrefix,
      });
    }
    Some(result)
  }

  // Full declaration generators differ from function-head generators: every
  // body and name is expanded, so the source AST must own only the invocation.
  // Require the exact fresh contiguous native token run and an original direct
  // namespace/TU call. Never reinterpret a local/qualified/expression call.
  fn declaration_list_sites(
    packet: &Packet,
    root: &crate::ParsedRoot,
  ) -> Option<Vec<vorpal_language::CppProvenMacroSite>> {
    use vorpal_core::tree_sitter::LanguageExt;
    use vorpal_language::{CppProvenMacroKind, CppProvenMacroSite, SupportLang};
    if packet.observed_token_sites.is_empty() {
      return Some(Vec::new());
    }
    let mut candidates = std::collections::BTreeMap::new();
    for call in root
      .root()
      .dfs()
      .filter(|n| n.kind().as_ref() == "call_expression")
    {
      let Some(name) = call
        .field("function")
        .filter(|n| n.kind().as_ref() == "identifier")
      else {
        continue;
      };
      let mut context = call.parent();
      let mut direct = false;
      while let Some(node) = context {
        match node.kind().as_ref() {
          "expression_statement" | "ERROR" if node.range().start == call.range().start => {}
          "translation_unit" | "declaration_list" => {
            direct = node.kind().as_ref() == "translation_unit"
              || node
                .parent()
                .is_some_and(|p| p.kind().as_ref() == "namespace_definition");
            break;
          }
          _ => break,
        }
        context = node.parent();
      }
      if direct {
        candidates.insert(
          name.range().start,
          (name.text().into_owned(), call.field("arguments")?),
        );
      }
    }
    let mut origins: std::collections::BTreeMap<usize, Vec<usize>> =
      std::collections::BTreeMap::new();
    for (i, site) in packet.observed_token_sites.iter().enumerate() {
      origins.entry(site.offset).or_default().push(i);
    }
    let mut result = Vec::new();
    for expansion in &packet.expansions {
      let Some(anchor) = &expansion.definition else {
        continue;
      };
      let Some((name, arguments)) = candidates.get(&expansion.start) else {
        continue;
      };
      if name != &expansion.name || arguments.range().end != expansion.end {
        continue;
      }
      crate::cpp_macro_recovery::validated_arguments(&arguments.text(), anchor.parameters)?;
      let mut positions = origins.range(expansion.start..expansion.end);
      let Some((&offset, indices)) = positions.next() else {
        continue;
      };
      if offset != expansion.start || positions.next().is_some() {
        return None;
      }
      let (Some(&first), Some(&last)) = (indices.first(), indices.last()) else {
        continue;
      };
      if last - first + 1 != indices.len()
        || indices
          .iter()
          .any(|&i| !packet.observed_token_sites[i].from_macro)
      {
        return None;
      }
      let tokens = &packet.observed_tokens[first..=last];
      // Classify a literal native warning envelope without changing the fully
      // compared stream or any original source. Both operator ranges are part
      // of the proof, and unpaired/other operators remain unsupported.
      let push = ["__pragma", "(", "warning", "(", "push", ")", ")"];
      let pop = ["__pragma", "(", "warning", "(", "pop", ")", ")"];
      let wrapped = tokens.len() >= 14
        && tokens[..7].iter().map(String::as_str).eq(push)
        && tokens[tokens.len() - 7..]
          .iter()
          .map(String::as_str)
          .eq(pop);
      let declaration_tokens = if wrapped {
        &tokens[7..tokens.len() - 7]
      } else {
        tokens
      };
      let mut source = String::new();
      for token in declaration_tokens {
        if source.len().checked_add(token.len())?.checked_add(1)? > 4 * 1024 * 1024 {
          return None;
        }
        source.push_str(token);
        source.push(' ');
      }
      let parsed = SupportLang::Cpp.grep(&source);
      let proof = parsed.root();
      let children: Vec<_> = proof
        .children()
        .filter(|n| n.is_named() && n.kind().as_ref() != "comment")
        .collect();
      if proof.has_error()
        || children.is_empty()
        || children.iter().any(|n| !complete_native_declaration(n))
        // Non-function generators have no fabricated argument call edges.
        // Effectful authored arguments need a separate runtime-use proof.
        || !children.iter().all(|n| n.dfs().any(|n| n.kind().as_ref() == "function_declarator"))
          && arguments.dfs().any(|n| n.kind().as_ref() == "call_expression")
        || proof.dfs().any(|n| {
          matches!(
            n.kind().as_ref(),
            "sdk_call_modifier"
              | "sdk_parameter_annotation"
              | "macro_type_argument"
              | "macro_statement"
              | "macro_declaration"
          )
        })
      {
        continue;
      }
      result.push(CppProvenMacroSite {
        offset: u32::try_from(expansion.start).ok()?,
        name: expansion.name.clone(),
        kind: CppProvenMacroKind::DeclarationList,
      });
    }
    Some(result)
  }

  fn complete_native_declaration(
    node: &vorpal_core::Node<'_, vorpal_core::tree_sitter::StrDoc<vorpal_language::SupportLang>>,
  ) -> bool {
    match node.kind().as_ref() {
      "namespace_definition" => node.field("body").is_some_and(|body| {
        let nodes: Vec<_> = body
          .children()
          .filter(|n| n.is_named() && n.kind().as_ref() != "comment")
          .collect();
        !nodes.is_empty() && nodes.iter().all(complete_native_declaration)
      }),
      "template_declaration" => node
        .children()
        .filter(|n| n.is_named())
        .last()
        .is_some_and(|n| complete_native_declaration(&n)),
      "function_definition" => node
        .field("declarator")
        .is_some_and(|n| n.dfs().any(|n| n.kind().as_ref() == "function_declarator")),
      "declaration" => node.field("type").is_some() && node.field("declarator").is_some(),
      _ => false,
    }
  }

  // A case/for generator must be a direct switch-body invocation followed by
  // an original compound body. Native tokens prove only the generated prefix;
  // its generated label/loop expressions never become authored source nodes.
  fn case_loop_prefix_sites(
    packet: &Packet,
    root: &crate::ParsedRoot,
  ) -> Option<Vec<vorpal_language::CppProvenMacroSite>> {
    use vorpal_core::tree_sitter::LanguageExt;
    use vorpal_language::{CppProvenMacroKind, CppProvenMacroSite, SupportLang};
    if packet.observed_token_sites.is_empty() {
      return Some(Vec::new());
    }
    let mut candidates = std::collections::BTreeMap::new();
    for call in root
      .root()
      .dfs()
      .filter(|n| n.kind().as_ref() == "call_expression")
    {
      let Some(name) = call
        .field("function")
        .filter(|n| n.kind().as_ref() == "identifier")
      else {
        continue;
      };
      let mut context = call.parent();
      while let Some(node) = context {
        match node.kind().as_ref() {
          "expression_statement" | "ERROR" if node.range().start == call.range().start => {}
          "compound_statement"
            if node
              .parent()
              .is_some_and(|n| n.kind().as_ref() == "switch_statement") =>
          {
            let body = node.children().find(|n| {
              n.is_named() && n.kind().as_ref() != "comment" && n.range().start >= call.range().end
            });
            if body.is_some_and(|n| {
              n.kind().as_ref() == "compound_statement"
                && crate::cpp_macro_recovery::invocation_spacing(
                  packet.source.as_bytes(),
                  call.range().end,
                ) == Some(n.range().start)
            }) {
              candidates.insert(
                name.range().start,
                (name.text().into_owned(), call.field("arguments")?),
              );
            }
            break;
          }
          _ => break,
        }
        context = node.parent();
      }
    }
    let mut origins: std::collections::BTreeMap<usize, Vec<usize>> =
      std::collections::BTreeMap::new();
    for (i, site) in packet.observed_token_sites.iter().enumerate() {
      origins.entry(site.offset).or_default().push(i);
    }
    let mut result = Vec::new();
    for expansion in &packet.expansions {
      let Some(anchor) = &expansion.definition else {
        continue;
      };
      let Some((name, arguments)) = candidates.get(&expansion.start) else {
        continue;
      };
      if name != &expansion.name || arguments.range().end != expansion.end {
        continue;
      }
      crate::cpp_macro_recovery::validated_arguments(&arguments.text(), anchor.parameters)?;
      let mut positions = origins.range(expansion.start..expansion.end);
      let Some((&offset, indices)) = positions.next() else {
        continue;
      };
      if offset != expansion.start || positions.next().is_some() {
        return None;
      }
      let (Some(&first), Some(&last)) = (indices.first(), indices.last()) else {
        continue;
      };
      if last - first + 1 != indices.len()
        || indices
          .iter()
          .any(|&i| !packet.observed_token_sites[i].from_macro)
      {
        return None;
      }
      let prefix = "void proof() { switch(0) { ";
      let mut source = prefix.to_owned();
      for token in &packet.observed_tokens[first..=last] {
        if source.len().checked_add(token.len())?.checked_add(8)? > 4 * 1024 * 1024 {
          return None;
        }
        source.push_str(token);
        source.push(' ');
      }
      let body_start = source.len();
      source.push_str("{} } }");
      let parsed = SupportLang::Cpp.grep(&source);
      let proof = parsed.root();
      let Some(body) = proof
        .children()
        .find(|n| n.kind().as_ref() == "function_definition")
        .and_then(|n| n.field("body"))
      else {
        continue;
      };
      let statements: Vec<_> = body.children().filter(|n| n.is_named()).collect();
      if proof.has_error()
        || statements.len() != 1
        || statements[0].kind().as_ref() != "switch_statement"
      {
        continue;
      }
      let Some(switch_body) = statements[0].field("body") else {
        continue;
      };
      let cases: Vec<_> = switch_body.children().filter(|n| n.is_named()).collect();
      if cases.len() != 1
        || cases[0].kind().as_ref() != "case_statement"
        || cases[0].range().start != prefix.len()
      {
        continue;
      }
      let children: Vec<_> = cases[0].children().filter(|n| n.is_named()).collect();
      let Some(loop_node) = children
        .last()
        .filter(|n| n.kind().as_ref() == "for_statement")
      else {
        continue;
      };
      if children.len() != 2
        || !loop_node.field("body").is_some_and(|n| {
          n.kind().as_ref() == "compound_statement"
            && n.range().start == body_start
            && n.text() == "{}"
        })
        || proof
          .dfs()
          .filter(|n| n.kind().as_ref() == "for_statement")
          .count()
          != 1
        || proof
          .dfs()
          .filter(|n| n.kind().as_ref() == "case_statement")
          .count()
          != 1
        || proof.dfs().any(|n| {
          matches!(
            n.kind().as_ref(),
            "sdk_call_modifier"
              | "sdk_parameter_annotation"
              | "macro_type_argument"
              | "macro_statement"
              | "macro_declaration"
          )
        })
      {
        continue;
      }
      result.push(CppProvenMacroSite {
        offset: u32::try_from(expansion.start).ok()?,
        name: expansion.name.clone(),
        kind: CppProvenMacroKind::CaseLoopPrefix,
      });
    }
    Some(result)
  }

  fn parse_without_context(
    config: &CompilerCommand,
    path: &Path,
    source: &str,
  ) -> Option<CompilerParse> {
    let (packet, dependency) = capture(config, path, source)?;
    let buffers: Vec<_> = packet
      .definitions
      .iter()
      .map(|b| DefinitionBuffer {
        path: &b.path,
        source: &b.source,
      })
      .collect();
    let expansions: Vec<_> = packet
      .expansions
      .iter()
      .map(|e| Expansion {
        name: &e.name,
        invocation: e.start..e.end,
        definition: e.definition.as_ref().map(|d| DefinitionAnchor {
          buffer: d.buffer,
          name_offset: d.name_offset,
          end: d.end,
          parameters: d.parameters,
        }),
      })
      .collect();
    let observation = Observation {
      path,
      source,
      definitions: &buffers,
      expansions: &expansions,
      expanded_names: &packet.expanded_names,
      complete: packet.complete,
      volatile_inputs: packet.volatile_inputs,
    };
    let (mut evidence, ranges) =
      crate::cpp_macro_compiler_audit::prepare_evidence(path, source, &observation).ok()?;
    add_native_generators(source, &packet, &mut evidence)?;
    refine_native_literal_arguments(source, &packet, &mut evidence)?;
    let bindings: std::collections::BTreeMap<_, _> = evidence
      .bindings
      .iter()
      .map(|b| (b.active.start, b.definition.clone()))
      .collect();
    let (mut root, _, _, mut diagnostics) =
      crate::cpp_macro_recovery::parse_compiler_evidence(source, evidence);
    let mut sites = function_prefix_sites(&packet, &root)?;
    sites.extend(declaration_list_sites(&packet, &root)?);
    sites.extend(case_loop_prefix_sites(&packet, &root)?);
    sites.extend(object_prefix_sites(&packet, source)?);
    sites.extend(typed_handler_sites(&packet, source)?);
    if !sites.is_empty() {
      use vorpal_core::tree_sitter::LanguageExt;
      use vorpal_language::{CppProvenMacroKind, CppProvenMacroSite};
      for node in root
        .root()
        .dfs()
        .filter(|n| n.kind().as_ref() == "macro_statement")
      {
        let name = node.field("name")?;
        let binding = bindings.get(&name.range().start)?;
        let arguments = crate::cpp_macro_recovery::validated_arguments(
          &node.field("arguments")?.text(),
          binding.parameters,
        )?;
        let proof = binding
          .replacement
          .instantiate(&arguments.iter().map(String::as_str).collect::<Vec<_>>())?;
        sites.push(CppProvenMacroSite {
          offset: u32::try_from(name.range().start).ok()?,
          name: name.text().into_owned(),
          kind: if crate::cpp_macro_evidence::statement_accepts_else(&proof) {
            CppProvenMacroKind::OpenIf
          } else {
            CppProvenMacroKind::Statement
          },
        });
      }
      sites.sort_by_key(|s| s.offset);
      if sites.windows(2).any(|s| s[0].offset >= s[1].offset) {
        return None;
      }
      if sites.iter().any(|s| {
        s.name.is_empty()
          || s.name.len() > 128
          || !s
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
          || !s.name.as_bytes()[0].is_ascii_alphabetic() && !s.name.starts_with('_')
      }) {
        return None;
      }
      root = vorpal_language::with_cpp_proven_macro_sites(&sites, || {
        vorpal_lang_registry::SgLang::Builtin(vorpal_language::SupportLang::Cpp).grep(source)
      });
    }
    let mut definition_sites = Vec::new();
    if packet.callee_sites.len() > 16384 {
      return None;
    }
    let mut starts = BTreeSet::new();
    for callee in &packet.callee_sites {
      if callee.name.is_empty()
        || callee.name.len() > 128
        || !callee
          .name
          .bytes()
          .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || !callee.name.as_bytes()[0].is_ascii_alphabetic() && !callee.name.starts_with('_')
        || source
          .get(..callee.start)?
          .chars()
          .next_back()
          .is_some_and(|c| c.is_alphanumeric() || c == '_')
        || !packet.expanded_names.contains(&callee.name)
        || !starts.insert(callee.start)
        || source.get(callee.start..callee.start.checked_add(callee.name.len())?)? != callee.name
        || source
          .get(callee.start + callee.name.len()..)?
          .chars()
          .next()
          .is_some_and(|c| c.is_alphanumeric() || c == '_')
      {
        return None;
      }
    }
    if packet.expansions.iter().any(|e| {
      !packet
        .callee_sites
        .iter()
        .any(|c| c.start == e.start && c.name == e.name)
    }) {
      return None;
    }
    let mut omitted = Vec::new();
    let mut runtime_masks = std::collections::BTreeMap::new();
    for node in root.root().dfs() {
      if node.kind().as_ref() == "macro_declaration"
        || node.kind().as_ref() == "case_statement" && node.field("name").is_some()
      {
        let span = node.field("name")?.range().start..node.field("arguments")?.range().end;
        if !ranges.contains(&span) {
          return None;
        }
        omitted.push(node.field("arguments")?.range());
      }
      if node.kind().as_ref() == "function_definition"
        && node
          .field("name")
          .is_some_and(|n| starts.contains(&n.range().start))
      {
        definition_sites.push(node.range());
        omitted.push(node.field("arguments")?.range());
      }
      if node.kind().as_ref() == "macro_statement" {
        let span = node.field("name")?.range().start..node.field("arguments")?.range().end;
        if !ranges.contains(&span) {
          return None;
        }
        let definition = bindings.get(&span.start)?;
        let mask = runtime_masks
          .entry(std::sync::Arc::as_ptr(&definition.replacement) as usize)
          .or_insert_with(|| {
            definition
              .replacement
              .runtime_parameters(definition.parameters)
          });
        let mask = mask.as_ref()?;
        let arguments: Vec<_> = node
          .field("arguments")?
          .children()
          .filter(|n| n.is_named() && n.kind().as_ref() != "comment")
          .collect();
        if arguments.len() != definition.parameters {
          return None;
        }
        omitted.extend(
          arguments
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !mask.contains(i))
            .map(|(_, n)| n.range()),
        );
      }
      if node.kind().as_ref() == "call_expression"
        && let Some(function) = node.field("function")
      {
        if starts.range(function.range()).next().is_some() {
          // An unsupported expansion can ignore/stringify arguments or make
          // them unevaluated. Its raw argument syntax is not runtime evidence.
          omitted.push(node.range());
        }
      }
      if node.kind().as_ref() == "function_declarator"
        && let Some(declarator) = node.field("declarator")
        && starts.range(declarator.range()).next().is_some()
      {
        if let Some(owner) = node.ancestors().find(|n| {
          matches!(
            n.kind().as_ref(),
            "function_definition" | "declaration" | "field_declaration"
          )
        }) {
          definition_sites.push(owner.range());
        }
      }
    }
    for node in root.root().dfs() {
      if matches!(
        node.kind().as_ref(),
        "class_specifier"
          | "struct_specifier"
          | "enum_specifier"
          | "namespace_definition"
          | "alias_declaration"
      ) && let Some(name) = node.field("name")
        && starts.range(name.range()).next().is_some()
      {
        definition_sites.push(node.range());
      }
    }
    omitted.sort_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in omitted {
      if let Some(last) = merged.last_mut()
        && range.start <= last.end
      {
        last.end = last.end.max(range.end);
      } else {
        merged.push(range);
      }
    }
    for call in root
      .root()
      .dfs()
      .filter(|n| n.kind().as_ref() == "call_expression")
    {
      let range = call.range();
      let index = merged.partition_point(|r| r.start <= range.start);
      if index > 0 && range.end <= merged[index - 1].end {
        diagnostics.macro_calls.push(range);
      }
    }
    if !current(path, source)
      || packet
        .definitions
        .iter()
        .any(|b| !current(&b.path, &b.source))
    {
      return None;
    }
    Some(CompilerParse {
      root,
      diagnostics,
      dependency,
      definition_sites,
    })
  }
}

#[cfg(feature = "builtin-parser")]
pub(crate) use native::parse;
