//! Ephemeral freshness observations for a running proof-backed C++ server.
//! This stores input fingerprints, never bindings, parser contexts or products.
use rayon::prelude::*;
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
  revision: u64,
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
    let mut observations = self.observations.lock().unwrap_or_else(|p| p.into_inner());
    let revision = observations.revision.saturating_add(1);
    *observations = Observations {
      revision,
      ..Observations::default()
    };
  }

  /// Successful full revalidation also initializes an empty C++ input set.
  pub fn finish_refresh(&self) {
    let mut observations = self.observations.lock().unwrap_or_else(|p| p.into_inner());
    if !observations.initialized {
      observations.initialized = true;
      observations.revision = observations.revision.saturating_add(1);
    }
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
      let prior = observations.inputs.get(&path);
      if prior != Some(&state) {
        if prior.is_some() {
          observations.conflicted = true;
        }
        observations.revision = observations.revision.saturating_add(1);
        observations.inputs.insert(path, state);
      }
    }
  }

  /// Content hashes also detect same-length edits with restored timestamps.
  /// A mutation observed within a build cannot certify a consistent epoch.
  pub fn has_changed(&self) -> bool {
    self.has_changed_with(fingerprint)
  }

  fn has_changed_with(&self, read: impl Fn(&Path) -> Fingerprint + Sync + Send) -> bool {
    let (revision, inputs) = {
      let observations = self.observations.lock().unwrap_or_else(|p| p.into_inner());
      if !observations.initialized || observations.conflicted || observations.revision == u64::MAX {
        return true;
      }
      (
        observations.revision,
        observations
          .inputs
          .iter()
          .map(|(path, state)| (path.clone(), state.clone()))
          .collect::<Vec<_>>(),
      )
    };
    // Do not hold the observations lock across Rayon work: extraction workers
    // can publish observations on the same pool. Every query still reads every
    // consulted input unless a changed input already requires rebuilding.
    if inputs.par_iter().any(|(path, prior)| read(path) != *prior) {
      return true;
    }
    let observations = self.observations.lock().unwrap_or_else(|p| p.into_inner());
    !observations.initialized || observations.conflicted || observations.revision != revision
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn initialized() -> MacroFreshness {
    MacroFreshness {
      observations: Mutex::new(Observations {
        initialized: true,
        revision: 1,
        inputs: BTreeMap::from([(PathBuf::from("observed.h"), state(7))]),
        ..Observations::default()
      }),
    }
  }

  fn state(digest: u64) -> Fingerprint {
    Fingerprint {
      redirected: false,
      digest: Some(digest),
    }
  }

  #[test]
  fn validates_unchanged_content_and_rejects_content_or_redirect_changes() {
    let freshness = initialized();
    assert!(!freshness.has_changed_with(|_| state(7)));
    assert!(freshness.has_changed_with(|_| state(8)));
    assert!(freshness.has_changed_with(|_| Fingerprint {
      redirected: true,
      digest: Some(7)
    }));
    assert!(freshness.has_changed_with(|_| Fingerprint {
      redirected: false,
      digest: None
    }));
  }

  #[test]
  fn refresh_during_validation_declines_even_when_the_same_inputs_return() {
    let freshness = initialized();
    assert!(freshness.has_changed_with(|_| {
      freshness.begin_refresh();
      {
        let mut observations = freshness.observations.lock().unwrap();
        observations
          .inputs
          .insert(PathBuf::from("observed.h"), state(7));
      }
      freshness.finish_refresh();
      state(7)
    }));
    assert!(!freshness.has_changed_with(|_| state(7)));
  }

  #[test]
  fn conflicting_or_new_observations_during_validation_decline() {
    for conflict in [false, true] {
      let freshness = initialized();
      assert!(freshness.has_changed_with(|_| {
        let mut observations = freshness.observations.lock().unwrap();
        observations.revision += 1;
        observations.conflicted = conflict;
        observations.inputs.insert(PathBuf::from("new.h"), state(7));
        state(7)
      }));
    }
  }

  #[cfg(feature = "builtin-parser")]
  #[test]
  fn publishing_new_inputs_changes_revision_but_identical_inputs_do_not() {
    let freshness = MacroFreshness::default();
    let evidence = crate::cpp_macro_evidence::Evidence::default();
    let source = "void run() {}";
    freshness.begin_refresh();
    freshness.observe(Path::new("original.cc"), source, &evidence);
    freshness.finish_refresh();
    let revision = freshness.observations.lock().unwrap().revision;
    assert!(!freshness.has_changed_with(|_| {
      freshness.observe(Path::new("original.cc"), source, &evidence);
      Fingerprint {
        redirected: false,
        digest: Some(xxhash_rust::xxh3::xxh3_64(source.as_bytes())),
      }
    }));
    assert_eq!(freshness.observations.lock().unwrap().revision, revision);
    assert!(freshness.has_changed_with(|_| {
      freshness.observe(Path::new("new.cc"), source, &evidence);
      Fingerprint {
        redirected: false,
        digest: Some(xxhash_rust::xxh3::xxh3_64(source.as_bytes())),
      }
    }));
    assert!(freshness.observations.lock().unwrap().revision > revision);
    freshness.observe(Path::new("original.cc"), "void changed() {}", &evidence);
    assert!(
      freshness.has_changed_with(|_| panic!("conflicting observations decline before reading"))
    );
  }

  #[test]
  fn incomplete_epochs_and_revision_exhaustion_decline() {
    let freshness = initialized();
    freshness.begin_refresh();
    assert!(freshness.has_changed_with(|_| panic!("unfinished epochs do not read inputs")));
    freshness.finish_refresh();
    assert!(!freshness.has_changed_with(|_| panic!("empty completed epochs have no inputs")));
    freshness.observations.lock().unwrap().revision = u64::MAX;
    freshness.begin_refresh();
    freshness.finish_refresh();
    assert!(freshness.has_changed_with(|_| panic!("revision exhaustion stays conservative")));
  }
}
