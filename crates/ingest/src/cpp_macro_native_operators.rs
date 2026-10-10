//! Ephemeral syntax proof for native-observed #/## replacements. Generated
//! text never becomes the indexed source, an original owner or a runtime edge.
use super::*;

pub(super) fn prepare(raw: &str, parameters: &[String]) -> Option<StatementReplacement> {
  if raw.len() > 4 * 1024 * 1024 {
    return None;
  }
  if !parameters.iter().all(|p| canonical_identifier(p)) {
    return None;
  }
  let source = raw.replace("\\\r\n", "").replace("\\\n", "");
  let parsed = SupportLang::Cpp.grep(&source);
  let protected = ProtectedRanges::new(parsed.root().dfs().filter_map(|n| {
    matches!(
      n.kind().as_ref(),
      "comment"
        | "string_literal"
        | "raw_string_literal"
        | "char_literal"
        | "number_literal"
        | "user_defined_literal"
    )
    .then(|| n.range())
  }));
  let mut tokens = Vec::new();
  let bytes = source.as_bytes();
  let mut i = 0;
  while i < bytes.len() {
    if let Some(span) = protected.covering(i) {
      if !source[span.clone()].starts_with("//") && !source[span.clone()].starts_with("/*") {
        tokens.push(span.clone());
      }
      i = span.end;
      continue;
    }
    if bytes[i].is_ascii_whitespace() {
      i += 1;
      continue;
    }
    if bytes[i] == b'\\' {
      return None;
    }
    let start = i;
    if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
      i += 1;
      while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
      }
    } else if bytes.get(i..i + 2) == Some(b"##") {
      i += 2;
    } else {
      i += source[i..].chars().next()?.len_utf8();
    }
    tokens.push(start..i);
  }
  let text = |index: usize| source.get(tokens.get(index)?.clone());
  let piece = |s: &str| {
    if !canonical_identifier(s) {
      return None;
    }
    Some(
      parameters
        .iter()
        .position(|p| p == s)
        .map(NativePiece::Parameter)
        .unwrap_or_else(|| NativePiece::Identifier(s.to_owned())),
    )
  };
  let mut substitutions = Vec::new();
  let mut operators = Vec::new();
  let mut i = 0;
  while i < tokens.len() {
    let value = text(i)?;
    if matches!(value, "_Pragma" | "__pragma") {
      return None;
    }
    if text(i + 1) == Some("##") {
      let start = i;
      let mut parts = vec![piece(value)?];
      while text(i + 1) == Some("##") {
        parts.push(piece(text(i + 2)?)?);
        i += 2;
      }
      operators.push(NativeOperator::Paste(
        tokens[start].start..tokens[i].end,
        parts,
      ));
    } else if value == "#" {
      let next = text(i + 1)?;
      let parameter = parameters.iter().position(|p| p == next)?;
      operators.push(NativeOperator::Stringify(
        tokens[i].start..tokens[i + 1].end,
        parameter,
      ));
      i += 1;
    } else if value == "##" {
      return None;
    } else if let Some(parameter) = parameters.iter().position(|p| p == value) {
      substitutions.push((tokens[i].clone(), parameter));
    }
    i += 1;
  }
  if operators.is_empty() {
    return None;
  }
  let mut result = StatementReplacement {
    source,
    substitutions,
    native_operators: operators,
    native_arguments: None,
    native_runtime_parameters: None,
    requires_semicolon: false,
  };
  let arguments: Vec<_> = parameters.iter().map(String::as_str).collect();
  let proof = result.instantiate(&arguments)?;
  if !complete_statement(&proof) {
    // Only standard do/while wrappers may borrow an original semicolon.
    if !complete_statement(&format!("{proof};")) {
      return None;
    }
    let parsed = SupportLang::Cpp.grep(format!("void proof() {{ {proof}; }}"));
    let statement = parsed
      .root()
      .children()
      .find(|n| n.kind().as_ref() == "function_definition")?
      .field("body")?
      .children()
      .find(|n| n.is_named() && n.kind().as_ref() != "comment")?;
    if statement.kind().as_ref() != "do_statement" {
      return None;
    }
    result.source.push_str("\n;");
    result.requires_semicolon = true;
  }
  Some(result)
}

pub(super) fn instantiate(proof: &StatementReplacement, arguments: &[&str]) -> Option<String> {
  const LIMIT: usize = 4 * 1024 * 1024;
  if proof.source.len() > LIMIT || arguments.iter().any(|a| a.len() > LIMIT) {
    return None;
  }
  let mut edits = Vec::new();
  let mut edited_bytes = proof.source.len();
  let mut add_edit = |span: Range<usize>, value: String| -> Option<()> {
    edited_bytes = edited_bytes
      .checked_sub(span.len())?
      .checked_add(value.len())?
      .checked_add(2)?;
    if edited_bytes > LIMIT {
      return None;
    }
    edits.push((span, value));
    Some(())
  };
  for (span, parameter) in &proof.substitutions {
    let boundary = |b: u8| b.is_ascii_whitespace() || b"()[]{},;".contains(&b);
    if (span.start > 0 && !boundary(proof.source.as_bytes()[span.start - 1]))
      || (span.end < proof.source.len() && !boundary(proof.source.as_bytes()[span.end]))
    {
      return None;
    }
    add_edit(span.clone(), arguments.get(*parameter)?.to_string())?;
  }
  for operator in &proof.native_operators {
    let (span, value) = match operator {
      NativeOperator::Stringify(span, parameter) => (
        span.clone(),
        serde_json::to_string(arguments.get(*parameter)?).ok()?,
      ),
      NativeOperator::Paste(span, parts) => {
        let mut value = String::new();
        for part in parts {
          let token = match part {
            NativePiece::Parameter(p) => *arguments.get(*p)?,
            NativePiece::Identifier(s) => s.as_str(),
          };
          if !canonical_identifier(token) {
            return None;
          }
          if value.len().checked_add(token.len())? > LIMIT {
            return None;
          }
          value.push_str(token);
        }
        if !canonical_identifier(&value) || matches!(value.as_str(), "_Pragma" | "__pragma") {
          return None;
        }
        (span.clone(), value)
      }
    };
    add_edit(span, value)?;
  }
  edits.sort_by_key(|(span, _)| span.start);
  let mut cursor = 0;
  let mut result = String::new();
  for (span, value) in edits {
    if span.start < cursor {
      return None;
    }
    result.push_str(proof.source.get(cursor..span.start)?);
    result.push(' ');
    result.push_str(&value);
    result.push(' ');
    if result.len() > 4 * 1024 * 1024 {
      return None;
    }
    cursor = span.end;
  }
  result.push_str(proof.source.get(cursor..)?);
  (result.len() <= 4 * 1024 * 1024).then_some(result)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn parameters() -> Vec<String> {
    vec!["x".into()]
  }

  #[test]
  fn chained_paste_comments_and_literal_hashes_are_distinct() {
    let proof = prepare(
      r##"{ log("#x ## x"); before_ /* gap */ ## x ## _after(); sink(x); }"##,
      &parameters(),
    )
    .unwrap();
    let expanded = proof.instantiate(&["value"]).unwrap();
    assert!(expanded.contains("before_value_after"));
    assert!(expanded.contains("\"#x ## x\""));
    assert_eq!(proof.runtime_parameters(1).unwrap(), BTreeSet::from([0]));
    let stringified = prepare("{ log(#x); }", &parameters()).unwrap();
    assert!(stringified.runtime_parameters(1).unwrap().is_empty());
    let pasted = prepare("{ test_##x(); }", &parameters()).unwrap();
    assert!(pasted.runtime_parameters(1).unwrap().is_empty());
    assert!(pasted.instantiate(&["value()"]).is_none());
  }

  #[test]
  fn invalid_operators_directives_and_generated_pragma_decline() {
    for source in [
      "{ log(#unknown); }",
      "{ ## x; }",
      "{ x ## ; }",
      "{ x ## 1; }",
      "{ __pragma(x); log(#x); }",
      "{ _Pragma(#x); }",
      "void test_##x() {}",
    ] {
      assert!(prepare(source, &parameters()).is_none(), "{source}");
    }
    let proof = prepare("{ _ ## x(); }", &parameters()).unwrap();
    assert!(proof.instantiate(&["Pragma"]).is_none());
  }

  #[test]
  fn only_outer_do_wrappers_borrow_original_semicolons() {
    let proof = prepare("do { log(#x); } while (false)", &parameters()).unwrap();
    assert!(proof.requires_semicolon);
    assert!(complete_statement(&proof.instantiate(&["value"]).unwrap()));
    assert!(prepare("if (ready) do { log(#x); } while (false)", &parameters()).is_none());
  }

  #[test]
  fn repeated_parameters_are_bounded_before_output_allocation() {
    let proof = prepare("{ log(#x); sink(x); sink(x); }", &parameters()).unwrap();
    let large = "a".repeat(2 * 1024 * 1024);
    assert!(proof.instantiate(&[&large]).is_none());
  }
}
