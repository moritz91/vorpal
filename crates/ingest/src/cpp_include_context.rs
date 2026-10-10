//! Read-only composition of explicitly selected textual C++ includes.
//!
//! This is a source-map audit, not a preprocessor or a bankable extraction root.
//! It substitutes actual include contents without adding delimiters. Unselected
//! directives remain intact; conditions and macros are never evaluated.
use std::collections::BTreeSet;
use std::io::Read;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

const MAX_DEPTH: usize = 16;
const MAX_FILES: usize = 128;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct PhysicalFile {
  pub path: PathBuf,
  pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalSpan {
  pub file: usize,
  pub bytes: Range<usize>,
}

#[derive(Debug)]
pub struct SourcePiece {
  pub composed: Range<usize>,
  pub original: PhysicalSpan,
}

#[derive(Debug)]
pub struct IncludeEdge {
  /// The real directive, retained as provenance rather than parser text.
  pub directive: PhysicalSpan,
  pub included_file: usize,
}

#[derive(Debug)]
pub struct IncludeContext {
  pub source: String,
  pub files: Vec<PhysicalFile>,
  pub pieces: Vec<SourcePiece>,
  pub includes: Vec<IncludeEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextError(pub String);

impl std::fmt::Display for ContextError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(&self.0)
  }
}
impl std::error::Error for ContextError {}

impl IncludeContext {
  /// Decompose an AST range into its real physical pieces. A cross-file
  /// definition must keep every piece; it cannot become a single-file range.
  /// Empty/missing-token ranges have ambiguous boundary ownership and decline.
  pub fn physical_spans(&self, range: Range<usize>) -> Option<Vec<PhysicalSpan>> {
    if range.start >= range.end || range.end > self.source.len() {
      return None;
    }
    let mut spans = Vec::new();
    for piece in &self.pieces {
      let start = range.start.max(piece.composed.start);
      let end = range.end.min(piece.composed.end);
      if start < end {
        let physical_start = piece.original.bytes.start + start - piece.composed.start;
        spans.push(PhysicalSpan {
          file: piece.original.file,
          bytes: physical_start..physical_start + end - start,
        });
      }
    }
    Some(spans)
  }

  /// Re-read every participating input. This audit owns no persistent cache.
  pub fn is_current(&self) -> bool {
    self.files.iter().all(|file| {
      physical_path(&file.path).is_ok()
        && std::fs::File::open(&file.path).is_ok_and(|input| {
          let length = file.source.len() as u64;
          if !input
            .metadata()
            .is_ok_and(|metadata| metadata.len() == length)
          {
            return false;
          }
          let mut bytes = Vec::new();
          input.take(length + 1).read_to_end(&mut bytes).is_ok() && bytes == file.source.as_bytes()
        })
    })
  }

  /// Versioned identity includes exact physical inputs and their include order.
  pub fn identity(&self) -> blake3::Hash {
    let mut hash = blake3::Hasher::new();
    hash.update(b"vorpal-cpp-include-context-audit-v1\0");
    for file in &self.files {
      #[cfg(windows)]
      let path_bytes: Vec<u8> = {
        use std::os::windows::ffi::OsStrExt;
        file
          .path
          .as_os_str()
          .encode_wide()
          .flat_map(u16::to_le_bytes)
          .collect()
      };
      #[cfg(unix)]
      let path_bytes = {
        use std::os::unix::ffi::OsStrExt;
        file.path.as_os_str().as_bytes().to_vec()
      };
      #[cfg(not(any(windows, unix)))]
      let path_bytes = file.path.as_os_str().as_encoded_bytes().to_vec();
      hash.update(&(path_bytes.len() as u64).to_le_bytes());
      hash.update(&path_bytes);
      hash.update(blake3::hash(file.source.as_bytes()).as_bytes());
    }
    for edge in &self.includes {
      for value in [
        edge.directive.file,
        edge.directive.bytes.start,
        edge.directive.bytes.end,
        edge.included_file,
      ] {
        hash.update(&(value as u64).to_le_bytes());
      }
    }
    hash.finalize()
  }
}

/// Compose only the requested local quoted includes, in real source order.
/// Selected includes inside conditional groups, repeated files, file/directory
/// symlinks, uncertain lexing and unused selections all fail closed. This API
/// makes no claim that retained headers/macros represent a compiler's active TU.
pub fn audit_context(root: &Path, selected: &[PathBuf]) -> Result<IncludeContext, ContextError> {
  if selected.len() >= MAX_FILES {
    return Err(ContextError("include selection file limit".into()));
  }
  let root = physical_path(root)?;
  let mut allowed = BTreeSet::new();
  for path in selected {
    let path = physical_path(path)?;
    if path == root || !allowed.insert(path) {
      return Err(ContextError("duplicate include selection".into()));
    }
  }
  let mut builder = Builder {
    context: IncludeContext {
      source: String::new(),
      files: Vec::new(),
      pieces: Vec::new(),
      includes: Vec::new(),
    },
    allowed,
    visited: BTreeSet::new(),
    bytes: 0,
  };
  builder.visit(root, 0)?;
  if !builder.allowed.is_subset(&builder.visited) {
    return Err(ContextError(
      "selected file was not reached by an unconditional literal include".into(),
    ));
  }
  if !builder.context.is_current() {
    return Err(ContextError("input changed during include audit".into()));
  }
  Ok(builder.context)
}

fn physical_path(path: &Path) -> Result<PathBuf, ContextError> {
  let absolute = if path.is_absolute() {
    path.to_owned()
  } else {
    std::env::current_dir()
      .map_err(|e| ContextError(e.to_string()))?
      .join(path)
  };
  let mut prefix = PathBuf::new();
  for component in absolute.components() {
    prefix.push(component.as_os_str());
    if matches!(component, Component::Normal(_)) {
      let metadata = std::fs::symlink_metadata(&prefix)
        .map_err(|e| ContextError(format!("{}: {e}", prefix.display())))?;
      #[cfg(windows)]
      let redirected = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
      };
      #[cfg(not(windows))]
      let redirected = metadata.file_type().is_symlink();
      if redirected {
        return Err(ContextError(format!(
          "redirected include path: {}",
          prefix.display()
        )));
      }
    }
  }
  std::fs::canonicalize(absolute).map_err(|e| ContextError(e.to_string()))
}

struct Builder {
  context: IncludeContext,
  allowed: BTreeSet<PathBuf>,
  visited: BTreeSet<PathBuf>,
  bytes: usize,
}

impl Builder {
  fn append(&mut self, file: usize, bytes: Range<usize>) {
    if bytes.is_empty() {
      return;
    }
    let start = self.context.source.len();
    self
      .context
      .source
      .push_str(&self.context.files[file].source[bytes.clone()]);
    self.context.pieces.push(SourcePiece {
      composed: start..self.context.source.len(),
      original: PhysicalSpan { file, bytes },
    });
  }

  fn visit(&mut self, path: PathBuf, depth: usize) -> Result<usize, ContextError> {
    if depth > MAX_DEPTH
      || self.context.files.len() >= MAX_FILES
      || !self.visited.insert(path.clone())
    {
      return Err(ContextError(
        "include depth/file limit or repeated physical input".into(),
      ));
    }
    let mut bytes = Vec::new();
    let remaining = MAX_BYTES - self.bytes;
    std::fs::File::open(&path)
      .and_then(|file| file.take(remaining as u64 + 1).read_to_end(&mut bytes))
      .map_err(|e| ContextError(format!("{}: {e}", path.display())))?;
    if bytes.len() > remaining {
      return Err(ContextError("include byte limit".into()));
    }
    let source = String::from_utf8(bytes).map_err(|e| ContextError(e.to_string()))?;
    // A compiler may supply an implicit final newline. The source-map audit
    // has no physical byte for it and must not silently fuse boundary tokens.
    if depth > 0 && !source.is_empty() && !source.ends_with('\n') {
      return Err(ContextError(
        "included file has no physical final newline".into(),
      ));
    }
    self.bytes = self
      .bytes
      .checked_add(source.len())
      .ok_or_else(|| ContextError("include byte limit".into()))?;
    if self.bytes > MAX_BYTES {
      return Err(ContextError("include byte limit".into()));
    }
    let directives = crate::cpp_directive_audit::audit(&source).map_err(|e| {
      ContextError(format!(
        "uncertain include lexing at {}:{}",
        path.display(),
        e.offset
      ))
    })?;
    let groups = crate::cpp_directive_audit::audit_groups(&source).map_err(|e| {
      ContextError(format!(
        "uncertain include guards at {}:{}",
        path.display(),
        e.offset
      ))
    })?;
    let file = self.context.files.len();
    self.context.files.push(PhysicalFile {
      path: path.clone(),
      source,
    });
    let mut cursor = 0;
    for directive in directives {
      let source = &self.context.files[file].source;
      // Deliberately narrow literal syntax. Escapes, continuations, comments,
      // angle/macro includes remain metadata, never guessed selections.
      let text = source[directive.span.clone()].trim_end_matches(['\r', '\n']);
      let Some(tail) = text
        .strip_prefix('#')
        .map(str::trim_start)
        .and_then(|s| s.strip_prefix("include"))
      else {
        continue;
      };
      if !tail.starts_with([' ', '\t']) {
        continue;
      }
      let literal = tail.trim();
      let Some(name) = literal.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        continue;
      };
      if name.is_empty() || name.contains(['"', '\\', '\r', '\n']) {
        continue;
      }
      let candidate = path.parent().expect("absolute file parent").join(name);
      // Unselected includes are untouched, including unavailable SDK headers.
      let Ok(candidate) = physical_path(&candidate) else {
        continue;
      };
      if !self.allowed.contains(&candidate) {
        continue;
      }
      if groups
        .iter()
        .any(|group| group.span.contains(&directive.span.start))
      {
        return Err(ContextError("selected include is conditional".into()));
      }
      self.append(file, cursor..directive.span.start);
      let included_file = self.visit(candidate, depth + 1)?;
      self.context.includes.push(IncludeEdge {
        directive: PhysicalSpan {
          file,
          bytes: directive.span.clone(),
        },
        included_file,
      });
      cursor = directive.span.end;
    }
    self.append(file, cursor..self.context.files[file].source.len());
    Ok(file)
  }
}
