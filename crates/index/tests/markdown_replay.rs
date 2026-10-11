//! A lexer-only fix must invalidate both old products and the unchanged-tree fast path.
use std::fs;
use vorpal_index::build_index;
use vorpal_ingest::SgLang;
use vorpal_ingest::{
  Manifest, OutlineExtractor, PackFormat, PackMsg, PackReader, PackWriter, cache_file_name,
  encode_product_into, extraction_identity, save_product,
};
use vorpal_language::{LanguageExt, SupportLang, grammar_digest_of};

// Frozen pre-NUL-fix formula: builtin digests had only the structural surface. Reconstruct
// the registry and injection folds too, so the manifest represents a real previous binary.
fn legacy_digest(lang: SgLang) -> u64 {
  if lang == SgLang::Builtin(SupportLang::Markdown) {
    grammar_digest_of(&lang.get_ts_language())
  } else {
    match lang {
      SgLang::Builtin(builtin) => vorpal_language::grammar_digest(builtin),
      _ => grammar_digest_of(&lang.get_ts_language()),
    }
  }
}

fn legacy_generation(lang: SgLang) -> u64 {
  let bare = legacy_digest(lang);
  let mut entries: Vec<_> = lang
    .injectable_languages()
    .unwrap_or_default()
    .iter()
    .filter_map(|name| name.parse::<SgLang>().ok())
    .map(|sub| (sub.to_string(), legacy_digest(sub)))
    .collect();
  if entries.is_empty() {
    return bare;
  }
  entries.sort_unstable();
  entries.dedup();
  let mut h = xxhash_rust::xxh3::Xxh3::new();
  h.update(&bare.to_le_bytes());
  for (name, digest) in entries {
    h.update(name.as_bytes());
    h.update(&[0]);
    h.update(&digest.to_le_bytes());
  }
  h.digest()
}

fn legacy_stamp(rules: u64) -> u64 {
  let mut entries: Vec<_> = SgLang::all_langs()
    .into_iter()
    .filter(|lang| vorpal_ingest::grammar_generation_for(*lang).is_some())
    .map(|lang| (lang.to_string(), legacy_digest(lang)))
    .collect();
  entries.sort_unstable();
  let mut h = xxhash_rust::xxh3::Xxh3::new();
  h.update(b"vorpal-grammar-stamp/v2\n");
  h.update(&(entries.len() as u64).to_le_bytes());
  for (name, digest) in entries {
    h.update(name.as_bytes());
    h.update(&[0]);
    h.update(&digest.to_le_bytes());
  }
  let base = h.digest();
  let mut hosts: Vec<_> = SgLang::all_langs()
    .into_iter()
    .filter(|lang| vorpal_ingest::grammar_generation_for(*lang).is_some())
    .filter_map(|lang| {
      let generation = legacy_generation(lang);
      (generation != legacy_digest(lang)).then(|| (lang.to_string(), generation))
    })
    .collect();
  let grammar = if hosts.is_empty() {
    base
  } else {
    hosts.sort_unstable();
    let mut h = xxhash_rust::xxh3::Xxh3::new();
    h.update(&base.to_le_bytes());
    for (name, generation) in hosts {
      h.update(name.as_bytes());
      h.update(&[0]);
      h.update(&generation.to_le_bytes());
    }
    h.digest()
  };
  extraction_identity(grammar, rules)
}

fn migration(packed: bool) {
  let nonce = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_nanos();
  let root = std::env::temp_dir().join(format!(
    "vorpal-markdown-migration-{}-{nonce}-{packed}",
    std::process::id()
  ));
  let src = root.join("src");
  let out = root.join("index");
  fs::create_dir_all(&src).unwrap();
  fs::create_dir_all(out.join("products")).unwrap();
  let source = "# Diagram\n\n```text\nleft\0right\n```\n\n# Following\n";
  fs::write(src.join("guide.md"), source).unwrap();
  fs::write(src.join("lib.rs"), "pub fn intact() {}\n").unwrap();
  let src = src.canonicalize().unwrap();
  let extractor = OutlineExtractor::new().unwrap();
  let mut manifest = Manifest::scan(&src, |path| extractor.handles(path)).unwrap();
  let prior = if packed {
    let seed = root.join("seed");
    assert_eq!(build_index(&src, &seed).unwrap().indexed, 2);
    assert!(build_index(&src, &seed).unwrap().reused);
    // Seed an isolated legacy flat index root, not a mutated content-addressed generation.
    // A real old binary's generation would have a different content id.
    copy_tree(&vorpal_kg::resolve_index_dir(&seed), &out);
    out.clone()
  } else {
    out.clone()
  };
  let writer = packed.then(|| {
    let reader = PackReader::open_rooted(&prior, Some(&src.to_string_lossy())).unwrap();
    let format = if reader.is_bucketed() {
      PackFormat::Bucketed
    } else {
      PackFormat::Flat
    };
    PackWriter::new(
      &prior,
      None,
      Some(src.to_string_lossy().into_owned()),
      format,
    )
  });
  let sink = writer.as_ref().map(PackWriter::sink);
  for stat in manifest.entries() {
    let text = fs::read_to_string(&stat.path).unwrap();
    let mut product = extractor.extract_product(&stat.path, &text).unwrap();
    assert_eq!(product.error_nodes, 0);
    product.source_mtime_ns = stat.mtime_ns;
    product.source_size = stat.size;
    product.source_xxh3 = xxhash_rust::xxh3::xxh3_64(text.as_bytes());
    if stat.path.ends_with("guide.md") {
      product.grammar_digest = extraction_identity(
        legacy_generation(SgLang::Builtin(SupportLang::Markdown)),
        extractor.rules_digest(),
      );
      assert_ne!(
        product.grammar_digest,
        extractor.extraction_identity_for_path(&stat.path).unwrap()
      );
      // The old wildcard lexer diagnosed this valid physical NUL; seed the obsolete result.
      product.error_nodes = 1;
      product.error_bytes = 1;
    }
    if let Some(sink) = &sink {
      let mut body = Vec::new();
      encode_product_into(&product, &mut body);
      sink
        .send(PackMsg {
          path: stat.path.clone(),
          body,
        })
        .unwrap();
    } else {
      save_product(
        &out.join("products").join(cache_file_name(&stat.path)),
        &product,
      )
      .unwrap();
    }
  }
  drop(sink);
  if let Some(writer) = writer {
    writer
      .finish(manifest.entries().iter().map(|stat| stat.path.clone()))
      .unwrap();
  }
  let old_stamp = legacy_stamp(extractor.rules_digest());
  assert_ne!(
    old_stamp,
    extraction_identity(
      vorpal_ingest::global_grammar_stamp(),
      extractor.rules_digest()
    )
  );
  manifest.set_grammar_stamp(old_stamp);
  manifest.save(&prior.join("manifest.bin")).unwrap();
  let migrated = build_index(&src, &out).unwrap();
  assert!(!migrated.reused && !migrated.graph_reused, "{migrated:?}");
  assert_eq!(
    migrated.indexed, 1,
    "only obsolete Markdown reparses: {migrated:?}"
  );
  assert_eq!(migrated.skipped, 1);
  assert_eq!(migrated.error_nodes, 0);
  assert_eq!(migrated.error_bytes, 0);
  assert_eq!(fs::read(src.join("guide.md")).unwrap(), source.as_bytes());
  assert!(build_index(&src, &out).unwrap().reused);
  let scratch = root.join("scratch");
  build_index(&src, &scratch).unwrap();
  assert_eq!(
    fs::read(out.join("CURRENT")).unwrap(),
    fs::read(scratch.join("CURRENT")).unwrap()
  );
  fs::remove_dir_all(&root).unwrap();
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
  fs::create_dir_all(to).unwrap();
  for entry in fs::read_dir(from).unwrap() {
    let entry = entry.unwrap();
    let target = to.join(entry.file_name());
    if entry.file_type().unwrap().is_dir() {
      copy_tree(&entry.path(), &target);
    } else {
      fs::copy(entry.path(), target).unwrap();
    }
  }
}

#[test]
fn markdown_lexer_fix_migrates_loose_products() {
  migration(false);
}

#[test]
fn markdown_lexer_fix_migrates_packed_products_and_unchanged_tree() {
  migration(true);
}
