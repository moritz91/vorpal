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
    native_directives: Vec<String>,
    definitions: Vec<Buffer>,
    expansions: Vec<Site>,
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
      || !packet.native_directives.is_empty()
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
    let (evidence, ranges) =
      crate::cpp_macro_compiler_audit::prepare_evidence(path, source, &observation).ok()?;
    let bindings: std::collections::BTreeMap<_, _> = evidence
      .bindings
      .iter()
      .map(|b| (b.active.start, b.definition.clone()))
      .collect();
    let (root, _, _, mut diagnostics) =
      crate::cpp_macro_recovery::parse_compiler_evidence(source, evidence);
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
