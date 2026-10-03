//! Conservative, read-only evidence for complete C++ statement macros.
//!
//! This is an audit seam, not parser recovery. Definitions are replayed in source
//! order, quoted includes are resolved relative to their including file, and
//! uncertain directives invalidate evidence. No preprocessor condition is evaluated.
//! A consumer must incorporate `dependencies` into product replay identity before
//! using these bindings to recover a parse.

use std::collections::BTreeMap;
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};

use vorpal_core::tree_sitter::LanguageExt;
use vorpal_language::SupportLang;

/// A definition whose replacement is exactly one complete statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementMacro {
  pub name: String,
  pub parameters: usize,
  pub definition_path: PathBuf,
  pub definition_span: Range<usize>,
}

/// An interval in the original translation unit where a definition is evidenced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
  pub definition: StatementMacro,
  pub active: Range<usize>,
}

/// Includes that were actually consulted, including unreadable/missing paths.
/// Consumers must also watch missing paths: creating a header changes evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
  pub path: PathBuf,
  pub digest: Option<u64>,
}

#[derive(Debug, Default)]
pub struct Evidence {
  pub bindings: Vec<Binding>,
  pub dependencies: Vec<Dependency>,
  /// Ordered search roots; changes in search order must also invalidate evidence.
  pub include_roots: Vec<PathBuf>,
}

impl Evidence {
  /// Dependency/search identity for a future recovery consumer. The consumer must
  /// separately validate the translation unit's exact source bytes and grammar.
  /// This does not install invalidation in the index or its product caches.
  pub fn dependency_identity(&self) -> u64 {
    let mut hash = xxhash_rust::xxh3::Xxh3::new();
    hash.update(b"vorpal-cpp-macro-evidence-v1\0");
    hash.update(&(self.include_roots.len() as u64).to_le_bytes());
    for root in &self.include_roots {
      let text = root.as_os_str().as_encoded_bytes();
      hash.update(&(text.len() as u64).to_le_bytes());
      hash.update(text);
    }
    hash.update(&(self.dependencies.len() as u64).to_le_bytes());
    for dependency in &self.dependencies {
      let text = dependency.path.as_os_str().as_encoded_bytes();
      hash.update(&(text.len() as u64).to_le_bytes());
      hash.update(text);
      hash.update(&[u8::from(dependency.digest.is_some())]);
      hash.update(&dependency.digest.unwrap_or_default().to_le_bytes());
    }
    hash.digest()
  }

  pub fn at(&self, name: &str, offset: usize) -> Option<&StatementMacro> {
    self
      .bindings
      .iter()
      .find(|b| b.definition.name == name && b.active.contains(&offset))
      .map(|b| &b.definition)
  }
}

struct Audit {
  include_roots: Vec<PathBuf>,
  dependencies: BTreeMap<PathBuf, Option<u64>>,
  stack: Vec<PathBuf>,
  remaining_bytes: usize,
  remaining_files: usize,
}

/// Audit without modifying source, evaluating conditions, or caching header reads.
/// The limits bound pathological include trees; exhausting a limit clears evidence.
pub fn audit(path: &Path, source: &str) -> Evidence {
  audit_with_roots(path, source, &[])
}

/// Search quoted includes locally first, then in the supplied root order.
/// Angle includes search only the supplied roots. Never infer a search root.
pub fn audit_with_roots(path: &Path, source: &str, include_roots: &[PathBuf]) -> Evidence {
  let mut audit = Audit {
    include_roots: include_roots
      .iter()
      .map(|p| canonical_or_absolute(p))
      .collect(),
    dependencies: BTreeMap::new(),
    stack: Vec::new(),
    remaining_bytes: 4 * 1024 * 1024,
    remaining_files: 128,
  };
  let path = canonical_or_absolute(path);
  let mut environment = BTreeMap::new();
  let mut bindings = Vec::new();
  audit.visit(&path, source, &mut environment, Some(&mut bindings));
  Evidence {
    include_roots: audit.include_roots,
    bindings: bindings
      .into_iter()
      .filter(|b| !b.active.is_empty())
      .collect(),
    dependencies: audit
      .dependencies
      .into_iter()
      .map(|(path, digest)| Dependency { path, digest })
      .collect(),
  }
}

fn canonical_or_absolute(path: &Path) -> PathBuf {
  path
    .canonicalize()
    .or_else(|_| std::path::absolute(path))
    .unwrap_or_else(|_| path.to_path_buf())
}

impl Audit {
  fn resolve_include(
    &mut self,
    including: &Path,
    text: &str,
    environment: &mut BTreeMap<String, StatementMacro>,
  ) {
    let (relative, quoted) =
      if let Some(relative) = text.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        (relative, true)
      } else if let Some(relative) = text.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        (relative, false)
      } else {
        environment.clear();
        return;
      };
    let mut candidates = Vec::new();
    if quoted {
      candidates.push(including.parent().unwrap_or(Path::new(".")).join(relative));
    }
    candidates.extend(self.include_roots.iter().map(|root| root.join(relative)));
    for path in candidates {
      match path.try_exists() {
        Ok(true) => {
          self.include(&path, environment);
          return;
        }
        Ok(false) => {
          self.dependencies.insert(path, None);
        }
        Err(_) => {
          self.dependencies.insert(path, None);
          environment.clear();
          return;
        }
      }
    }
    environment.clear();
  }

  fn include(&mut self, path: &Path, environment: &mut BTreeMap<String, StatementMacro>) {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if self.remaining_files == 0
      || self.stack.len() >= 16
      || self.stack.contains(&path)
      || std::fs::metadata(&path).is_ok_and(|m| m.len() > self.remaining_bytes as u64)
    {
      self.dependencies.insert(path, None);
      environment.clear();
      return;
    }
    let bytes = std::fs::File::open(&path).ok().and_then(|file| {
      let mut bytes = Vec::new();
      file
        .take(self.remaining_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
      (bytes.len() <= self.remaining_bytes).then_some(bytes)
    });
    self.dependencies.insert(
      path.clone(),
      bytes.as_ref().map(|b| xxhash_rust::xxh3::xxh3_64(b)),
    );
    let Some(bytes) = bytes else {
      environment.clear();
      return;
    };
    if self.stack.len() >= 16
      || self.stack.contains(&path)
      || self.remaining_files == 0
      || bytes.len() > self.remaining_bytes
    {
      environment.clear();
      return;
    }
    self.remaining_files -= 1;
    self.remaining_bytes -= bytes.len();
    let Ok(source) = std::str::from_utf8(&bytes) else {
      environment.clear();
      return;
    };
    self.visit(&path, source, environment, None);
  }

  fn visit(
    &mut self,
    path: &Path,
    source: &str,
    environment: &mut BTreeMap<String, StatementMacro>,
    mut bindings: Option<&mut Vec<Binding>>,
  ) {
    self.stack.push(path.to_path_buf());
    let parsed = SupportLang::Cpp.grep(source);
    let root = parsed.root();
    for node in root.children() {
      let previous = environment.clone();
      match node.kind().as_ref() {
        "preproc_function_def" => {
          if let Some(name) = node.field("name") {
            let name = name.text().into_owned();
            environment.remove(&name);
            let values: Vec<_> = node
              .children()
              .filter(|n| n.kind().as_ref() == "preproc_arg")
              .collect();
            let parameters = node.field("parameters");
            if !node.has_error()
              && let (Some(first), Some(last), Some(parameters)) =
                (values.first(), values.last(), parameters)
            {
              let simple_parameters = parameters.children().all(|n| {
                n.kind().as_ref() == "identifier" || matches!(n.text().as_ref(), "(" | ")" | ",")
              });
              let parameters: Vec<_> = parameters.children().filter(|n| n.is_named()).collect();
              let unique: std::collections::BTreeSet<_> =
                parameters.iter().map(|n| n.text().into_owned()).collect();
              if simple_parameters
                && unique.len() == parameters.len()
                && complete_statement(&source[first.range().start..last.range().end])
              {
                environment.insert(
                  name.clone(),
                  StatementMacro {
                    name,
                    parameters: parameters.len(),
                    definition_path: path.to_path_buf(),
                    definition_span: node.range(),
                  },
                );
              }
            }
          } else {
            environment.clear();
          }
        }
        "preproc_def" => {
          if let Some(name) = node.field("name") {
            environment.remove(name.text().as_ref());
          } else {
            environment.clear();
          }
        }
        "preproc_include" => {
          if !node.has_error()
            && let Some(include) = node.field("path")
          {
            self.resolve_include(path, &include.text(), environment);
          } else {
            environment.clear();
          }
        }
        "preproc_call" => {
          let directive = node
            .field("directive")
            .map(|n| n.text().into_owned())
            .unwrap_or_default();
          let argument = node
            .field("argument")
            .map(|n| n.text().into_owned())
            .unwrap_or_default();
          if directive == "#undef"
            && !argument.trim().is_empty()
            && argument
              .trim()
              .bytes()
              .all(|b| b.is_ascii_alphanumeric() || b == b'_')
          {
            environment.remove(argument.trim());
          } else if !(directive == "#pragma" && argument.trim() == "once") {
            environment.clear();
          }
        }
        "comment" => {}
        _ => {
          // Conditional groups, malformed directives and directives nested in ordinary
          // syntax are uncertain. Never pretend to execute their branches.
          if node
            .dfs()
            .any(|n| n.kind().starts_with("preproc_") || n.is_error() && n.text().contains('#'))
          {
            environment.clear();
          }
        }
      }
      if let Some(bindings) = bindings.as_deref_mut() {
        for (name, old) in &previous {
          if environment.get(name) != Some(old) {
            if let Some(binding) = bindings
              .iter_mut()
              .rev()
              .find(|b| b.definition == *old && b.active.end == source.len())
            {
              binding.active.end = node.range().start;
            }
          }
        }
        for (name, definition) in environment.iter() {
          if previous.get(name) != Some(definition) {
            bindings.push(Binding {
              definition: definition.clone(),
              active: node.range().end..source.len(),
            });
          }
        }
      }
    }
    self.stack.pop();
  }
}

fn complete_statement(replacement: &str) -> bool {
  // Only the proof fixture joins continuation lines; original input is never rewritten.
  let replacement = replacement.replace("\\\r\n", "").replace("\\\n", "");
  let parsed = SupportLang::Cpp.grep(format!("void proof() {{ {replacement}\n }}"));
  let root = parsed.root();
  if root.has_error() {
    return false;
  }
  let Some(function) = root
    .children()
    .find(|n| n.kind().as_ref() == "function_definition")
  else {
    return false;
  };
  let Some(body) = function.field("body") else {
    return false;
  };
  let statements: Vec<_> = body
    .children()
    .filter(|n| n.is_named() && n.kind().as_ref() != "comment")
    .collect();
  statements.len() == 1
    && matches!(
      statements[0].kind().as_ref(),
      "if_statement" | "try_statement" | "compound_statement"
    )
}
