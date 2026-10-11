//! Grammar fingerprint (invariant I2 — extraction parity).
//!
//! Agent-mode matches are trustworthy only if the node's tree-sitter grammars behave exactly like
//! the coordinator's. The fingerprint is a `blake3` digest over each language's observable grammar
//! surface — name, ABI version, the full node-kind table (with namedness), and the full field
//! table and builtin behavior revisions — plus the crate version and wire protocol. Two builds that disagree on any of those can
//! produce different trees, so the coordinator refuses (or demotes to streaming) on mismatch;
//! matching fingerprints plus the exact-version gate make silent divergence structurally hard.
//!
//! Computed over `SgLang::all_langs()` **at call time**: custom languages register per job, so the
//! handshake carries the built-in fingerprint and the job carries the post-`LangEnv` one.

use crate::lang::SgLang;
use vorpal_language::{GrammarSurfaceEvent, LanguageExt, grammar_behavior_revision, grammar_surface};

/// Fingerprint of every currently registered language (builtins + any registered customs).
pub fn grammar_fingerprint() -> [u8; 32] {
  fingerprint_langs(SgLang::all_langs())
}

/// Fingerprint of the built-in grammars only. This is what an agent advertises in `Welcome`
/// (custom languages register later, per job), so it is what the coordinator can check at
/// handshake time; the post-`LangEnv` fingerprint is re-verified agent-side against the job.
pub fn builtin_fingerprint() -> [u8; 32] {
  fingerprint_langs(
    SgLang::all_langs()
      .into_iter()
      .filter(|lang| matches!(lang, SgLang::Builtin(_)))
      .collect(),
  )
}

fn fingerprint_langs(mut langs: Vec<SgLang>) -> [u8; 32] {
  // Sort by display name so registration order cannot perturb the digest.
  langs.sort_by_key(|l| l.to_string());
  let mut hasher = blake3::Hasher::new();
  hasher.update(b"vorpal-grammar-fingerprint/v1\n");
  hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
  hasher.update(&[vorpal_wire::PROTOCOL_VERSION]);
  for lang in langs {
    let ts = lang.get_ts_language();
    hasher.update(b"\nlang\n");
    hasher.update(lang.to_string().as_bytes());
    // The shared surface enumeration (F-M0): the same walk that produces the product-cache
    // digest feeds this fingerprint, so the two identities can never observe different
    // surfaces. The v1 byte framing is preserved exactly — it writes the FIELD COUNT
    // between the kind and field loops, so that write is deferred until the first field
    // event (or the walk's end for zero-field grammars). Metadata/parse-state events are
    // unused here, as in v1. Unpatched languages retain that historical framing.
    let mut pending_field_count: Option<u64> = None;
    grammar_surface(&ts, |event| match event {
      GrammarSurfaceEvent::Abi(abi) => {
        hasher.update(&abi.to_le_bytes());
      }
      GrammarSurfaceEvent::Counts {
        node_kinds, fields, ..
      } => {
        hasher.update(&(node_kinds as u64).to_le_bytes());
        pending_field_count = Some(fields as u64);
      }
      GrammarSurfaceEvent::NodeKind { name, named } => {
        if let Some(kind) = name {
          hasher.update(kind.as_bytes());
        }
        hasher.update(&[0, u8::from(named)]);
      }
      GrammarSurfaceEvent::Field(name) => {
        if let Some(count) = pending_field_count.take() {
          hasher.update(&count.to_le_bytes());
        }
        if let Some(field) = name {
          hasher.update(field.as_bytes());
        }
        hasher.update(&[0]);
      }
      GrammarSurfaceEvent::Name(_) | GrammarSurfaceEvent::Metadata(_) => {}
    });
    // Zero-field grammars fire no Field event; v1 still wrote their (zero) count.
    if let Some(count) = pending_field_count {
      hasher.update(&count.to_le_bytes());
    }
    if let SgLang::Builtin(builtin) = lang
      && let Some(revision) = grammar_behavior_revision(builtin)
    {
      hasher.update(b"\nbehavior/v1\n");
      hasher.update(&(revision.len() as u64).to_le_bytes());
      hasher.update(revision);
    }
  }
  *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The pre-patch v1 loop: structural framing remains unchanged, but a builtin behavior
  /// patch must make a different binary refuse an otherwise identical fleet handshake.
  fn legacy_fingerprint_langs(mut langs: Vec<SgLang>) -> [u8; 32] {
    langs.sort_by_key(|l| l.to_string());
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"vorpal-grammar-fingerprint/v1\n");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(&[vorpal_wire::PROTOCOL_VERSION]);
    for lang in langs {
      let ts = lang.get_ts_language();
      hasher.update(b"\nlang\n");
      hasher.update(lang.to_string().as_bytes());
      hasher.update(&(ts.abi_version() as u64).to_le_bytes());
      let kinds = ts.node_kind_count();
      hasher.update(&(kinds as u64).to_le_bytes());
      for id in 0..kinds {
        let id = id as u16;
        if let Some(kind) = ts.node_kind_for_id(id) {
          hasher.update(kind.as_bytes());
        }
        hasher.update(&[0, u8::from(ts.node_kind_is_named(id))]);
      }
      let fields = ts.field_count();
      hasher.update(&(fields as u64).to_le_bytes());
      for id in 1..=fields {
        if let Some(field) = ts.field_name_for_id(id as u16) {
          hasher.update(field.as_bytes());
        }
        hasher.update(&[0]);
      }
    }
    *hasher.finalize().as_bytes()
  }

  #[test]
  fn unaffected_fingerprints_stay_v1_and_markdown_behavior_invalidates_parity() {
    let unaffected: Vec<_> = SgLang::all_langs().into_iter()
      .filter(|lang| !matches!(lang, SgLang::Builtin(vorpal_language::SupportLang::Markdown)))
      .collect();
    assert_eq!(
      fingerprint_langs(unaffected.clone()),
      legacy_fingerprint_langs(unaffected),
      "unrelated grammar fingerprints moved"
    );
    assert_ne!(fingerprint_langs(SgLang::all_langs()), legacy_fingerprint_langs(SgLang::all_langs()),
      "identical structural tables cannot hide different Markdown lexer behavior");
  }

  #[test]
  fn fingerprint_is_deterministic_and_registration_order_independent() {
    let a = grammar_fingerprint();
    let b = grammar_fingerprint();
    assert_eq!(a, b);
    // Reversed input order must not change the digest (sorted internally).
    let mut langs = SgLang::all_langs();
    langs.reverse();
    assert_eq!(fingerprint_langs(langs), a);
  }

  #[test]
  fn fingerprint_is_not_trivial() {
    assert_ne!(grammar_fingerprint(), [0u8; 32]);
  }
}
