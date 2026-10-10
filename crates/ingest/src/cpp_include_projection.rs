//! Physical attribution and conservative product handoff for genuine textual includes.
//!
//! The public report exposes no bankable root. Internal physical products are
//! prepared only after validating the complete context. Local entity indices
//! refer to this report, never to dense KG IDs. Cross-file function bodies
//! retain their actual owner and complete definition pieces; they cannot become
//! unrelated file-level calls or a fabricated single-file definition.

use std::borrow::Cow;
use std::ops::Range;
use vorpal_outline::model::{
  DefinitionSourceContext, OutlineEntry, PhysicalSourceSpan, SourcePosition, SourceRange,
};

use crate::cpp_include_context::{ContextError, IncludeContext, PhysicalSpan};
use crate::outline_extractor::{ParsedRoot, product_from_parts};
use crate::product::{ExtractedParts, ProductRef, ReferenceSourceContext};

#[derive(Debug)]
pub struct ProjectionAudit {
  pub root: String,
  pub identity: String,
  pub definitions: Vec<DefinitionProjection>,
  pub references: Vec<ReferenceProjection>,
  /// All raw ERROR and MISSING nodes, including ambiguous boundary locations.
  pub diagnostics: Vec<DiagnosticProjection>,
  pub(crate) facts: crate::FileProduct,
}

#[derive(Debug)]
pub struct DefinitionProjection {
  pub entity_index: u32,
  pub parent_entity_index: Option<u32>,
  /// The exact existing KG layout discriminator, including overload signatures.
  pub entity_path: String,
  pub entry: OutlineEntry<'static>,
  pub is_import: bool,
  pub is_exported: bool,
}

#[derive(Debug)]
pub struct ReferenceProjection {
  /// Index into the common original layout, even when the owner is in another
  /// physical file. Zero denotes a genuine file-level occurrence.
  pub owner_entity_index: u32,
  pub physical: PhysicalSourceSpan,
  pub reference: ProductRef,
}

#[derive(Debug)]
pub struct DiagnosticProjection {
  pub kind: String,
  pub missing: bool,
  /// Multiple zero-width locations at a boundary express uncertainty, not
  /// multiple errors. Nonempty diagnostics retain all their original pieces.
  pub locations: Vec<PhysicalSourceSpan>,
}

fn fail(message: &str) -> ContextError {
  ContextError(message.into())
}

fn source_range(source: &str, bytes: Range<usize>) -> Result<SourceRange, ContextError> {
  let position = |offset| {
    let prefix = source
      .get(..offset)
      .ok_or_else(|| fail("range splits UTF-8 or exceeds physical input"))?;
    let line = prefix.bytes().filter(|&byte| byte == b'\n').count();
    let column = prefix
      .rsplit('\n')
      .next()
      .unwrap_or_default()
      .chars()
      .count();
    Ok::<_, ContextError>(SourcePosition { line, column })
  };
  if bytes.start > bytes.end {
    return Err(fail("reversed physical range"));
  }
  Ok(SourceRange {
    start: position(bytes.start)?,
    end: position(bytes.end)?,
    byte_offset: bytes,
  })
}

fn physical(
  context: &IncludeContext,
  span: PhysicalSpan,
) -> Result<PhysicalSourceSpan, ContextError> {
  let file = context
    .files
    .get(span.file)
    .ok_or_else(|| fail("unknown physical file"))?;
  Ok(PhysicalSourceSpan {
    path: file
      .path
      .to_str()
      .ok_or_else(|| fail("non-UTF-8 physical path"))?
      .into(),
    range: source_range(&file.source, span.bytes)?,
  })
}

fn portions(
  context: &IncludeContext,
  range: Range<usize>,
) -> Result<Vec<PhysicalSourceSpan>, ContextError> {
  context
    .physical_spans(range)
    .ok_or_else(|| fail("unmappable nonempty range"))?
    .into_iter()
    .map(|span| physical(context, span))
    .collect()
}

/// A trivial outline capture borrows the actual AST name token. Recovering its
/// byte offset from that borrow avoids guessing which repeated name is the
/// declaration and also rejects manufactured/transformed names conservatively.
fn name_range(entry: &OutlineEntry<'_>, source: &str) -> Result<Range<usize>, ContextError> {
  let Cow::Borrowed(name) = &entry.name else {
    return Err(fail("definition name is not an original AST capture"));
  };
  let start = (name.as_ptr() as usize)
    .checked_sub(source.as_ptr() as usize)
    .ok_or_else(|| fail("definition name is not in the composed source"))?;
  let end = start
    .checked_add(name.len())
    .ok_or_else(|| fail("name range overflow"))?;
  if name.is_empty()
    || source.get(start..end) != Some(*name)
    || start < entry.range.byte_offset.start
    || end > entry.range.byte_offset.end
  {
    return Err(fail("definition name escaped its original AST range"));
  }
  Ok(start..end)
}

/// Lower one common parse into physical products. No fabricated parent declarations
/// are emitted: a member whose parent is in another file currently declines this
/// handoff, rather than attributing it to a file or cloning a class stub.
pub(crate) fn physical_products(
  context: &IncludeContext,
  report: ProjectionAudit,
) -> Result<std::collections::HashMap<String, crate::FileProduct>, ContextError> {
  use std::collections::HashMap;
  use vorpal_outline::model::{OutlineItem, OutlineMember};
  if !report.facts.swallows.is_empty() {
    return Err(fail(
      "context swallow recovery requires original diagnostic attribution",
    ));
  }
  let mut products = HashMap::new();
  for file in &context.files {
    let path = file
      .path
      .to_str()
      .ok_or_else(|| fail("non-UTF-8 physical path"))?;
    let mut product = report.facts.clone();
    product.source_xxh3 = xxhash_rust::xxh3::xxh3_64(file.source.as_bytes());
    product.error_nodes = 0;
    product.error_bytes = 0;
    product.error_spans.clear();
    product.cuts.clear();
    product.entity_params.clear();
    product.returns.clear();
    product.signatures.clear();
    product.requests.clear();
    products.insert(path.to_owned(), product);
  }
  // original layout -> physical path, new file-local index, durable identity
  let mut owners: HashMap<u32, (String, u32, u128)> = HashMap::new();
  let mut item_positions: HashMap<u32, usize> = HashMap::new();
  let mut local_counts: HashMap<String, u32> = HashMap::new();
  let mut identities = std::collections::HashSet::new();
  for definition in report.definitions {
    let path = definition
      .entry
      .source_context
      .as_ref()
      .unwrap()
      .name
      .path
      .clone();
    let index = local_counts.entry(path.clone()).or_default();
    *index += 1;
    let external = vorpal_kg::external_entity_id(&path, &definition.entity_path);
    if !identities.insert(external) {
      return Err(fail("multiple original definitions require a shared durable-identity handoff"));
    }
    owners.insert(definition.entity_index, (path.clone(), *index, external));
    let product = products
      .get_mut(&path)
      .ok_or_else(|| fail("definition outside participating inputs"))?;
    match definition.parent_entity_index {
      Some(parent) => {
        if owners
          .get(&parent)
          .is_none_or(|(owner_path, _, _)| owner_path != &path)
        {
          return Err(fail(
            "cross-file member parent requires a foreign containment handoff",
          ));
        }
        let position = *item_positions
          .get(&parent)
          .ok_or_else(|| fail("missing original member parent"))?;
        product.items[position].members.push(OutlineMember {
          entry: definition.entry,
          is_public: definition.is_exported,
        });
      }
      None => {
        item_positions.insert(definition.entity_index, product.items.len());
        product.items.push(OutlineItem {
          entry: definition.entry,
          is_import: definition.is_import,
          is_exported: definition.is_exported,
          members: Vec::new(),
        });
      }
    }
  }
  for projection in report.references {
    let mut reference = projection.reference;
    reference.from_entity_index = 0;
    if projection.owner_entity_index != 0 {
      let (owner_path, local, external) = owners
        .get(&projection.owner_entity_index)
        .ok_or_else(|| fail("reference owner outside projected definitions"))?;
      if owner_path == &projection.physical.path {
        reference.from_entity_index = *local;
      } else {
        reference.source_context.as_mut().unwrap().owner_external = Some(*external);
      }
    }
    products
      .get_mut(&projection.physical.path)
      .ok_or_else(|| fail("reference outside physical products"))?
      .refs
      .push(reference);
  }
  // The composed parse does not contain selected include directives. Preserve
  // those actual file-to-file imports separately, at their authored directive
  // spans; this creates neither declaration stubs nor runtime calls.
  for include in &context.includes {
    let path = context.files[include.directive.file].path.to_str().unwrap();
    products.get_mut(path).unwrap().refs.push(ProductRef {
      from_entity_index: 0,
      source_context: None,
      name: include.spelling.clone(),
      kind: 2,
      start: include.directive.bytes.start as u32,
      end: include.directive.bytes.end as u32,
      alias: None,
      qualifier: None,
      form: 0,
      receiver: None,
      receiver_type: None,
      receiver_type_origin: 255,
      call_shape: 0,
      args: Vec::new(),
    });
  }
  for (entity, params) in report.facts.entity_params {
    let (path, local, _) = owners
      .get(&entity)
      .ok_or_else(|| fail("parameter owner outside projected definitions"))?;
    products
      .get_mut(path)
      .unwrap()
      .entity_params
      .push((*local, params));
  }
  for mut signature in report.facts.signatures {
    let (path, local, _) = owners
      .get(&signature.entity_index)
      .ok_or_else(|| fail("signature owner outside projected definitions"))?;
    signature.entity_index = *local;
    products.get_mut(path).unwrap().signatures.push(signature);
  }
  for (name, ret) in report.facts.returns {
    for product in products.values_mut() {
      if product.items.iter().any(|item| {
        item.entry.name == name || item.members.iter().any(|member| member.entry.name == name)
      }) {
        product.returns.push((name.clone(), ret.clone()));
      }
    }
  }
  for mut request in report.facts.requests {
    let locations = portions(context, request.start as usize..request.end as usize)?;
    if locations.len() != 1 {
      return Err(fail("request crosses an include boundary"));
    }
    let physical = &locations[0];
    if request.from_entity_index != 0 {
      let (path, local, _) = owners
        .get(&request.from_entity_index)
        .ok_or_else(|| fail("request has no projected owner"))?;
      if path != &physical.path {
        return Err(fail("request requires foreign owner handoff"));
      }
      request.from_entity_index = *local;
    }
    request.start = physical.range.byte_offset.start as u32;
    request.end = physical.range.byte_offset.end as u32;
    products
      .get_mut(&physical.path)
      .unwrap()
      .requests
      .push(request);
  }
  for diagnostic in report.diagnostics {
    let mut files = std::collections::HashSet::new();
    for location in diagnostic.locations {
      let product = products
        .get_mut(&location.path)
        .ok_or_else(|| fail("diagnostic outside physical inputs"))?;
      // One original diagnostic may cover several pieces in one physical file.
      if files.insert(location.path) {
        product.error_nodes += 1;
      }
      product.error_spans.push((
        location.range.byte_offset.start as u32,
        location.range.byte_offset.end as u32,
      ));
    }
  }
  for product in products.values_mut() {
    product.error_spans.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (start, end) in std::mem::take(&mut product.error_spans) {
      match merged.last_mut() {
        Some((_, prior_end)) if start <= *prior_end => *prior_end = (*prior_end).max(end),
        _ => merged.push((start, end)),
      }
    }
    product.error_bytes = merged
      .iter()
      .map(|(start, end)| u64::from(end - start))
      .sum();
    product.error_spans = merged.into_iter().take(8).collect();
  }
  Ok(products)
}

pub(crate) fn project(
  context: &IncludeContext,
  parsed: &ParsedRoot,
  parts: ExtractedParts<'_>,
) -> Result<ProjectionAudit, ContextError> {
  let root = context.files[0]
    .path
    .to_str()
    .ok_or_else(|| fail("non-UTF-8 root"))?
    .to_owned();
  let layout = vorpal_kg::layout_entity_paths(&parts.items);
  let mut declaration_ranges = std::collections::HashMap::new();
  for node in parsed.root().dfs() {
    let mut declaration = node.clone();
    while let Some(parent) = declaration.parent() {
      if parent.kind() != "template_declaration" {
        break;
      }
      declaration = parent;
    }
    declaration_ranges.insert(
      (
        node.range().start,
        node.range().end,
        node.kind().into_owned(),
      ),
      declaration.range(),
    );
  }
  let mut captures = Vec::new();
  let mut parent = 0u32;
  for item in &parts.items {
    parent += 1;
    captures.push((
      None,
      name_range(&item.entry, parsed.source())?,
      item.is_import,
      item.is_exported,
    ));
    for member in &item.members {
      captures.push((
        Some(parent),
        name_range(&member.entry, parsed.source())?,
        false,
        member.is_public,
      ));
    }
    parent += item.members.len() as u32;
  }
  let mut product = product_from_parts(parts);
  let entries = std::mem::take(&mut product.items)
    .into_iter()
    .flat_map(|item| {
      std::iter::once(item.entry).chain(item.members.into_iter().map(|member| member.entry))
    });
  let mut definitions = Vec::new();
  let mut template_prefix_owners = Vec::new();
  for (index, (mut entry, (parent_entity_index, capture, is_import, is_exported))) in
    entries.zip(captures).enumerate()
  {
    let name = portions(context, capture)?;
    if name.len() != 1 {
      return Err(fail("definition name crosses physical include boundary"));
    }
    let name = name.into_iter().next().unwrap();
    let key = (
      entry.range.byte_offset.start,
      entry.range.byte_offset.end,
      entry.ast_kind.to_string(),
    );
    let declaration = declaration_ranges
      .get(&key)
      .ok_or_else(|| fail("definition has no original AST node"))?;
    if declaration.start < entry.range.byte_offset.start {
      template_prefix_owners.push((
        declaration.start..entry.range.byte_offset.start,
        index as u32 + 1,
      ));
    }
    let original = portions(context, declaration.clone())?;
    let primary = original
      .iter()
      .find(|part| {
        part.path == name.path
          && part.range.byte_offset.start <= name.range.byte_offset.start
          && part.range.byte_offset.end >= name.range.byte_offset.end
      })
      .ok_or_else(|| fail("definition has no name-owning physical portion"))?;
    entry.range = primary.range.clone();
    entry.source_context = Some(Box::new(DefinitionSourceContext {
      root: root.clone(),
      identity: context.identity().to_hex().to_string(),
      inputs: context
        .files
        .iter()
        .map(|file| vorpal_outline::model::SourceContextInput {
          path: file.path.to_str().expect("physical UTF-8 path").into(),
          digest: xxhash_rust::xxh3::xxh3_64(file.source.as_bytes()),
        })
        .collect(),
      references: Vec::new(),
      name,
      parts: original,
    }));
    let entity_index = index as u32 + 1;
    definitions.push(DefinitionProjection {
      entity_index,
      parent_entity_index,
      entity_path: layout[entity_index as usize].clone(),
      entry,
      is_import,
      is_exported,
    });
  }
  let mut references = Vec::new();
  for mut reference in std::mem::take(&mut product.refs) {
    // Template parameters/defaults precede the matched function node. They
    // belong to that definition, not to a surrounding file/namespace owner.
    if let Some((_, owner)) = template_prefix_owners
      .iter()
      .filter(|(range, _)| {
        range.start <= reference.start as usize && range.end >= reference.end as usize
      })
      .max_by_key(|(range, _)| range.start)
    {
      reference.from_entity_index = *owner;
    }
    let locations = portions(context, reference.start as usize..reference.end as usize)?;
    if locations.len() != 1 {
      return Err(fail("reference evidence crosses physical include boundary"));
    }
    let physical = locations.into_iter().next().unwrap();
    let owner_entity_index = reference.from_entity_index;
    if owner_entity_index as usize >= layout.len() {
      return Err(fail("reference has unknown owner"));
    }
    reference.start = physical.range.byte_offset.start as u32;
    reference.end = physical.range.byte_offset.end as u32;
    // Still a report-layout owner, not a bankable per-file index. A production
    // handoff must lower foreign owners to durable identities before ingestion.
    reference.source_context = Some(Box::new(ReferenceSourceContext {
      root: root.clone(),
      identity: context.identity().to_hex().to_string(),
      owner_external: None,
    }));
    references.push(ReferenceProjection {
      owner_entity_index,
      physical,
      reference,
    });
  }
  let mut diagnostics = Vec::new();
  // Persist every occurrence, including same-file ones. Identical offsets in
  // different pieces may be ambiguous in the legacy fixed-width evidence row;
  // all matching real paths remain explicit, never guessed from the owner path.
  for projection in &references {
    if projection.owner_entity_index == 0 {
      continue;
    }
    let owner = definitions
      .get_mut(projection.owner_entity_index as usize - 1)
      .ok_or_else(|| fail("physical evidence owner outside definitions"))?;
    owner
      .entry
      .source_context
      .as_mut()
      .unwrap()
      .references
      .push(vorpal_outline::model::ContextReferenceSite {
        name_hash: xxhash_rust::xxh3::xxh3_64(projection.reference.name.as_bytes()) as u32,
        edge_type: crate::product::tag_refkind(projection.reference.kind)
          .edge()
          .0,
        physical: projection.physical.clone(),
      });
  }
  for node in parsed.root().dfs() {
    if node.kind() != "ERROR" && !node.is_missing() {
      continue;
    }
    let range = node.range();
    let locations = if range.is_empty() {
      let mut candidates = Vec::new();
      for piece in &context.pieces {
        if range.start >= piece.composed.start && range.start <= piece.composed.end {
          let offset = piece.original.bytes.start + range.start - piece.composed.start;
          let candidate = physical(
            context,
            PhysicalSpan {
              file: piece.original.file,
              bytes: offset..offset,
            },
          )?;
          if !candidates.contains(&candidate) {
            candidates.push(candidate);
          }
        }
      }
      if candidates.is_empty() {
        return Err(fail("missing token has no physical location"));
      }
      candidates
    } else {
      portions(context, range)?
    };
    diagnostics.push(DiagnosticProjection {
      kind: node.kind().into_owned(),
      missing: node.is_missing(),
      locations,
    });
  }
  Ok(ProjectionAudit {
    root,
    identity: context.identity().to_hex().to_string(),
    definitions,
    references,
    diagnostics,
    facts: product,
  })
}
