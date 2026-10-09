use vorpal_core::{Language, tree_sitter::LanguageExt};
use vorpal_ingest::OutlineExtractor;
use vorpal_lang_registry::SgLang;

#[test]
fn native_assignments_and_bare_word_arrays_keep_argument_and_variable_spans() {
  let language = SgLang::from_path("commands.ps1").unwrap();
  for word in [
    "--headless-frames=$Frames",
    "--output=$env:VALUE",
    "--format='%H %ad'",
    "--date=short",
    "--empty=",
  ] {
    let source = format!("tool {word}\nfunction Following {{ Write-Output 'done' }}\n");
    let parsed = language.grep(&source);
    assert!(!parsed.root().has_error(), "{word}");
    assert!(
      parsed
        .root()
        .dfs()
        .any(|n| n.kind().as_ref() == "generic_token" && n.text() == word)
    );
    if let Some(offset) = word.find('$') {
      assert!(
        parsed
          .root()
          .dfs()
          .any(|n| n.kind().as_ref() == "variable" && n.text() == word[offset..])
      );
    }
    let product = OutlineExtractor::new()
      .unwrap()
      .extract_product("commands.ps1", &source)
      .unwrap();
    assert_eq!(product.error_nodes, 0);
    assert!(product.items.iter().any(|i| i.entry.name == "Following"));
  }
  let arguments = "name, ownership, status, desired, actual";
  let source = format!("Format-Table {arguments} -Wrap\n");
  let parsed = language.grep(&source);
  assert!(!parsed.root().has_error());
  let array = parsed
    .root()
    .dfs()
    .find(|n| n.kind().as_ref() == "array_literal_expression" && n.text() == arguments)
    .unwrap();
  assert_eq!(&source[array.range()], arguments);
  assert_eq!(
    array
      .children()
      .filter(|n| n.kind().as_ref() == "generic_token")
      .map(|n| n.text().into_owned())
      .collect::<Vec<_>>(),
    ["name", "ownership", "status", "desired", "actual"]
  );
  for invalid in [
    "tool --frames=$env:\n",
    "tool --format='unfinished\n",
    "Format-Table name,\n",
    "Format-Table name,,status\n",
  ] {
    assert!(language.grep(invalid).root().has_error(), "{invalid}");
  }
}

#[test]
fn assignment_prefix_lookahead_is_invalidated_by_incremental_variable_edits() {
  use vorpal_core::tree_sitter::StrDoc;
  let language = SgLang::from_path("commands.ps1").unwrap();
  let source = "tool --frames=$Frames\n";
  for (offset, insertion, has_error) in [
    (source.find('$').unwrap(), " ", false),
    (source.find('\n').unwrap(), ":", true),
  ] {
    let old = StrDoc::new(source, language);
    assert!(!old.tree.root_node().has_error());
    let changed = format!("{}{}{}", &source[..offset], insertion, &source[offset..]);
    let incremental =
      StrDoc::try_new_incremental(&changed, language, Some((source, &old.tree))).unwrap();
    let fresh = StrDoc::new(&changed, language);
    assert_eq!(
      incremental.tree.root_node().has_error(),
      has_error,
      "{changed}"
    );
    assert_eq!(
      incremental.tree.root_node().to_sexp(),
      fresh.tree.root_node().to_sexp()
    );
  }
}

#[test]
fn native_argument_separator_does_not_capture_decrement_expressions_or_strings() {
  let language = SgLang::from_path("commands.ps1").unwrap();
  let command = "& wsl.exe -d $distro -- wslpath -a $archive.Replace('\\', '/')";
  for source in [format!("{command}\n"), format!("{command}\r\n")] {
    let parsed = language.grep(&source);
    assert!(!parsed.root().has_error());
    let separator = parsed
      .root()
      .dfs()
      .find(|n| n.kind().as_ref() == "command_parameter" && n.text() == "--")
      .unwrap();
    assert_eq!(&source[separator.range()], "--");
    assert_eq!(
      parsed
        .root()
        .dfs()
        .filter(|n| n.kind().as_ref() == "command")
        .count(),
      1
    );
    assert!(
      !parsed
        .root()
        .dfs()
        .any(|n| n.kind().as_ref() == "pre_decrement_expression")
    );
    assert!(
      parsed
        .root()
        .dfs()
        .any(|n| n.kind().as_ref() == "invokation_expression"
          && n.text() == "$archive.Replace('\\', '/')")
    );
  }
  for source in [
    "--$value\n",
    "$value--\n",
    "$text = '--'\n",
    "Write-Output $value--\n",
    "Write-Output --$value\n",
  ] {
    let parsed = language.grep(source);
    assert!(!parsed.root().has_error(), "{source}");
    assert!(
      !parsed
        .root()
        .dfs()
        .any(|n| n.kind().as_ref() == "command_parameter" && n.text() == "--")
    );
  }
  for invalid in [
    "Write-Output -- (1 +)\n",
    "Write-Output -- $value.Method(\n",
    "Write-Output \n --\n",
  ] {
    assert!(language.grep(invalid).root().has_error(), "{invalid}");
  }
}

#[test]
fn command_arguments_keep_complete_member_invocations() {
  let language = SgLang::from_path("commands.ps1").unwrap();
  for expression in [
    "$task.GetAwaiter().GetResult()",
    "$spec.root.Replace('\\', '/')",
  ] {
    let lf = format!("Write-Output {expression}\nfunction Following {{ Write-Output 'done' }}\n");
    for source in [lf.clone(), lf.replace('\n', "\r\n")] {
      let parsed = language.grep(&source);
      assert!(!parsed.root().has_error(), "{expression}");
      assert!(parsed.root().dfs().any(|n| n.kind().as_ref() == "invokation_expression"
        && &source[n.range()] == expression));
      let product = OutlineExtractor::new()
        .unwrap()
        .extract_product("commands.ps1", &source)
        .unwrap();
      assert_eq!(product.error_nodes, 0);
      assert!(product.items.iter().any(|i| i.entry.name == "Following"));
    }
  }
  for invalid in [
    "Write-Output $task.GetAwaiter().GetResult(",
    "Write-Output $task.GetAwaiter().GetResult(,)",
  ] {
    assert!(language.grep(invalid).root().has_error(), "{invalid}");
  }
}

#[test]
fn numeric_multipliers_preserve_literal_spans_and_following_functions() {
  let language = SgLang::from_path("sizes.ps1").unwrap();
  let extractor = OutlineExtractor::new().unwrap();
  for number in [
    "1MB", "2Kb", "3gB", "4TB", "5Pb", "0x10MB", "1.5MB", ".5KB", "1e2GB",
  ] {
    let lf = format!("$size = {number}\nfunction Following {{ Write-Output $size }}\n");
    for source in [lf.clone(), lf.replace('\n', "\r\n")] {
      let parsed = language.grep(&source);
      assert!(!parsed.root().has_error(), "{number}");
      assert!(parsed.root().dfs().any(|n| {
        matches!(
          n.kind().as_ref(),
          "decimal_integer_literal" | "hexadecimal_integer_literal" | "real_literal"
        ) && &source[n.range()] == number
      }));
      let product = extractor.extract_product("sizes.ps1", &source).unwrap();
      assert_eq!(product.error_nodes, 0, "{number}");
      assert!(product.items.iter().any(|i| i.entry.name == "Following"));
    }
  }
  for invalid in [
    "$size = (1MB",
    "$size = 1MB +",
    "function Broken { $size = 1MB",
  ] {
    assert!(language.grep(invalid).root().has_error(), "{invalid}");
  }
}

#[test]
fn parameter_names_with_digits_keep_full_spans_and_numeric_arguments() {
  let lf = "Invoke-Tool -Sha256 $hash -Port18765:42 -Name2 value | Out-Null\n$number = -256\nInvoke-Tool -256\n--$counter\nfunction Following { Write-Output 'done' }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let parsed = SgLang::from_path("parameters.ps1").unwrap().grep(&source);
    assert!(!parsed.root().has_error(), "{source}");
    let parameters: Vec<_> = parsed
      .root()
      .dfs()
      .filter(|n| n.kind() == "command_parameter")
      .map(|n| n.text().to_string())
      .collect();
    for name in ["-Sha256", "-Port18765", "-Name2"] {
      assert!(parameters.iter().any(|n| n == name), "{parameters:?}");
    }
    assert!(
      !parameters
        .iter()
        .any(|n| n.contains("256") && n != "-Sha256")
    );
    for node in parsed
      .root()
      .dfs()
      .filter(|n| n.kind() == "command_parameter")
    {
      assert_eq!(&source[node.range()], node.text());
    }
    let product = OutlineExtractor::new()
      .unwrap()
      .extract_product("parameters.ps1", &source)
      .unwrap();
    assert_eq!(product.error_nodes, 0);
    assert!(product.items.iter().any(|i| i.entry.name == "Following"));
  }
  let bad = "Invoke-Tool -Sha256 (1 + )\n";
  assert!(
    SgLang::from_path("parameters.ps1")
      .unwrap()
      .grep(bad)
      .root()
      .has_error()
  );
}
