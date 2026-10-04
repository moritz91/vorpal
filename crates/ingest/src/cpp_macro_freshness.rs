//! Ephemeral freshness observations for a running proof-backed C++ server.
//! This stores input fingerprints, never bindings, parser contexts or products.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
  redirected: bool,
  digest: Option<u64>,
}

fn fingerprint(path: &Path) -> Fingerprint {
  let redirected = crate::cpp_macro_evidence::path_has_redirected_components(path);
  let digest = (|| {
    // Check the target type before opening: a named pipe could block a query.
    // Reading a regular redirected target is a freshness observation only;
    // macro evidence still rejects redirected paths independently.
    if !std::fs::metadata(path).ok()?.is_file() {
      return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
      return None;
    }
    let mut hash = xxhash_rust::xxh3::Xxh3::new();
    let mut buffer = [0; 8192];
    loop {
      let count = file.read(&mut buffer).ok()?;
      if count == 0 {
        return Some(hash.digest());
      }
      hash.update(&buffer[..count]);
    }
  })();
  Fingerprint { redirected, digest }
}

#[derive(Debug, Default)]
struct Observations {
  initialized: bool,
  conflicted: bool,
  inputs: BTreeMap<PathBuf, Fingerprint>,
}

/// Shared by extraction and replay within one running server. Quiet queries
/// check the exact consulted inputs, including missing include candidates and
/// redirected paths. This does not reuse proof or enable recovery by itself.
#[derive(Debug, Default)]
pub struct MacroFreshness {
  observations: Mutex<Observations>,
}

impl MacroFreshness {
  /// Start an observation epoch after prior build workers have been drained.
  pub fn begin_refresh(&self) {
    *self.observations.lock().unwrap_or_else(|p| p.into_inner()) = Observations::default();
  }

  /// Successful full revalidation also initializes an empty C++ input set.
  pub fn finish_refresh(&self) {
    self
      .observations
      .lock()
      .unwrap_or_else(|p| p.into_inner())
      .initialized = true;
  }

  #[cfg(feature = "builtin-parser")]
  pub(crate) fn observe(
    &self,
    path: &Path,
    source: &str,
    evidence: &crate::cpp_macro_evidence::Evidence,
  ) {
    let source_path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut inputs = vec![(
      source_path.clone(),
      Fingerprint {
        redirected: crate::cpp_macro_evidence::path_has_redirected_components(&source_path),
        digest: Some(xxhash_rust::xxh3::xxh3_64(source.as_bytes())),
      },
    )];
    for dependency in &evidence.dependencies {
      let path = std::path::absolute(&dependency.path).unwrap_or_else(|_| dependency.path.clone());
      // Read digests come from this exact audit. A declined/unavailable input
      // carries no proof digest; observe its current state only for freshness.
      let state = match dependency.digest {
        Some(digest) => Fingerprint {
          redirected: false,
          digest: Some(digest),
        },
        None => fingerprint(&path),
      };
      inputs.push((path, state));
    }
    let mut observations = self.observations.lock().unwrap_or_else(|p| p.into_inner());
    for (path, state) in inputs {
      if observations
        .inputs
        .get(&path)
        .is_some_and(|prior| prior != &state)
      {
        observations.conflicted = true;
      }
      observations.inputs.insert(path, state);
    }
  }

  /// Content hashes also detect same-length edits with restored timestamps.
  /// A mutation observed within a build cannot certify a consistent epoch.
  pub fn has_changed(&self) -> bool {
    let observations = self.observations.lock().unwrap_or_else(|p| p.into_inner());
    if !observations.initialized || observations.conflicted {
      return true;
    }
    observations
      .inputs
      .iter()
      .any(|(path, prior)| fingerprint(path) != *prior)
  }
}
