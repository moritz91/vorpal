#![cfg(feature = "builtin-parser")]
use std::collections::BTreeSet;
use std::path::Path;

use vorpal_ingest::cpp_macro_compiler_audit::{
  Declined, DefinitionAnchor, DefinitionBuffer, Expansion, Observation, TokenAgreement, audit,
  compare_token_spellings,
};
use vorpal_ingest::cpp_macro_recovery::audit_recovery;

const HEADER: &str = "#define CHECK(x) if (!(x)) { throw Failure(x); }\n";

#[test]
fn native_replacement_end_ignores_real_trailing_comments_but_not_literal_payload() {
  for newline in ["\n", "\r\n"] {
    for replacement in [
      "{ sink(x); }",
      "{ sink(\"// payload /* literal */\"); sink(x); }",
    ] {
      for suffix in [
        "  // ordinary comment",
        " /* comment */ /* after */",
        " // fabricated()",
      ] {
        let header = format!("#define CHECK(x) {replacement}{suffix}{newline}");
        let source = format!("void run() {{ CHECK(value()) }}{newline}");
        let definitions = [DefinitionBuffer {
          path: Path::new("proof.h"),
          source: &header,
        }];
        let mut expansions = [expansion(&source, "CHECK(value())", &header, "CHECK")];
        let end = "#define CHECK(x) ".len() + replacement.len();
        expansions[0].definition.as_mut().unwrap().end = end;
        let names = BTreeSet::from(["CHECK".to_owned()]);
        let obs = Observation {
          path: Path::new("main.cc"),
          source: &source,
          definitions: &definitions,
          expansions: &expansions,
          expanded_names: &names,
          complete: true,
          volatile_inputs: false,
        };
        let report = audit(obs.path, &source, &obs).unwrap();
        assert!(!report.has_error, "{header}");
        for wrong in [
          end - 1,
          header.trim_end().len(),
          header.find("//").unwrap_or(end),
        ] {
          if wrong == end {
            continue;
          }
          let mut invalid = expansions.clone();
          invalid[0].definition.as_mut().unwrap().end = wrong;
          let invalid_obs = Observation {
            expansions: &invalid,
            ..obs
          };
          assert_eq!(
            audit(obs.path, &source, &invalid_obs).unwrap_err(),
            Declined::InvalidDefinition,
            "{wrong}: {header}"
          );
        }
      }
    }
  }
}

#[test]
fn native_raw_literal_line_endings_compare_without_rewriting_source_or_directives() {
  for prefix in ["R", "u8R", "uR", "UR", "LR"] {
    for delimiter in ["", "tag", "abcdefghijklmnop"] {
      let native = format!(
        "{prefix}\"{delimiter}(Grüße 日本語\r\n#pragma external_header(push)\r\n#line 1 \"other.h\"\r\n#count == 2){delimiter}\""
      );
      let observed = native.replace("\r\n", "\n");
      let original = native.clone();
      assert_eq!(
        compare_token_spellings(&["const", &native, ";"], &["const", &observed, ";"]),
        TokenAgreement::RawLiteralLineEndings
      );
      assert_eq!(native, original);
      assert_eq!(
        compare_token_spellings(&[&native], &[&native]),
        TokenAgreement::Exact
      );
    }
  }
  assert_eq!(
    compare_token_spellings(&["#", "pragma", "external_header", "(", "push", ")"], &[]),
    TokenAgreement::Different,
    "real directive tokens cannot disappear as raw-literal line endings"
  );
}

#[test]
fn token_comparison_retains_semantic_and_malformed_literal_differences() {
  for (native, observed) in [
    ("\"a\\r\\nb\"", "\"a\\nb\""),
    ("R\"(a\rb)\"", "R\"(a\nb)\""),
    ("R\"(a\r\nb)\"", "u8R\"(a\nb)\""),
    ("R\"one(a\r\nb)one\"", "R\"two(a\nb)two\""),
    ("R\"(a\r\nb)\"_tag", "R\"(a\nb)\"_tag"),
    (
      "R\"abcdefghijklmnopq(a\r\nb)abcdefghijklmnopq\"",
      "R\"abcdefghijklmnopq(a\nb)abcdefghijklmnopq\"",
    ),
    ("R\"bad tag(a\r\nb)bad tag\"", "R\"bad tag(a\nb)bad tag\""),
    ("R\"(a)\"\r\nb)\"", "R\"(a)\"\nb)\""),
    ("R\"(a\r\nb)", "R\"(a\nb)"),
    ("R\"(a\r\nb)\"", "R\"(different\nb)\""),
    ("first\r\nsecond", "first\nsecond"),
    ("1", "2"),
  ] {
    assert_eq!(
      compare_token_spellings(&[native], &[observed]),
      TokenAgreement::Different
    );
  }
  assert_eq!(
    compare_token_spellings(&["first", "+", "second"], &["second", "+", "first"]),
    TokenAgreement::Different
  );
  assert_eq!(
    compare_token_spellings(&["first", "+", "second"], &["first", "+second"]),
    TokenAgreement::Different
  );
}

fn expansion<'a>(source: &str, invocation: &str, header: &str, name: &'a str) -> Expansion<'a> {
  let start = source.find(invocation).unwrap();
  Expansion {
    name,
    invocation: start..start + invocation.len(),
    definition: Some(DefinitionAnchor {
      buffer: 0,
      name_offset: header.find(name).unwrap(),
      end: header.trim_end().len(),
      parameters: 1,
    }),
  }
}

#[test]
fn compiler_anchors_recover_restored_definitions_and_keep_original_calls() {
  for newline in ["\n", "\r\n"] {
    let source = "#include \"opaque-sdk.h\"\nvoid run() {\nCHECK(value())\nCHECK(next())\nCHECK(finalValue())\n}\nvoid following() { after(); }\n".replace('\n', newline);
    let first = HEADER.replace('\n', newline);
    let second = "#define CHECK(x) if (x) { sink(x); }\n".replace('\n', newline);
    let definitions = [
      DefinitionBuffer {
        path: Path::new("first.h"),
        source: &first,
      },
      DefinitionBuffer {
        path: Path::new("second.h"),
        source: &second,
      },
    ];
    let mut expansions = [
      expansion(&source, "CHECK(value())", &first, "CHECK"),
      expansion(&source, "CHECK(next())", &second, "CHECK"),
      expansion(&source, "CHECK(finalValue())", &first, "CHECK"),
    ];
    expansions[1].definition.as_mut().unwrap().buffer = 1;
    let names = BTreeSet::from(["CHECK".to_owned()]);
    let observation = Observation {
      path: Path::new("sample.cc"),
      source: &source,
      definitions: &definitions,
      expansions: &expansions,
      expanded_names: &names,
      complete: true,
      volatile_inputs: false,
    };
    assert!(audit_recovery(observation.path, &source, &[]).has_error);
    let report = audit(observation.path, &source, &observation).unwrap();
    assert!(!report.has_error, "{report:?}");
    assert_eq!(report.eligible_names, ["CHECK"]);
    assert_eq!(
      report.macro_spans,
      expansions
        .iter()
        .map(|e| e.invocation.clone())
        .collect::<Vec<_>>()
    );
    assert_eq!(report.functions, ["run", "following"]);
    for name in ["value", "next", "finalValue", "after"] {
      let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
      assert_eq!(&source[span.clone()], format!("{name}()"));
    }
    assert!(
      !report
        .calls
        .iter()
        .any(|(name, _)| ["CHECK", "sink", "Failure"].contains(&name.as_str()))
    );
    // Nothing escapes into the ambient/default parser after report generation.
    assert!(audit_recovery(observation.path, &source, &[]).has_error);
  }
}

#[test]
fn stale_source_partial_observations_and_volatile_inputs_decline() {
  let source = "void run() { CHECK(value()) }";
  let definitions = [DefinitionBuffer {
    path: Path::new("proof.h"),
    source: HEADER,
  }];
  let expansions = [expansion(source, "CHECK(value())", HEADER, "CHECK")];
  let names = BTreeSet::from(["CHECK".to_owned()]);
  let mut observation = Observation {
    path: Path::new("sample.cc"),
    source,
    definitions: &definitions,
    expansions: &expansions,
    expanded_names: &names,
    complete: true,
    volatile_inputs: false,
  };
  assert_eq!(
    audit(
      observation.path,
      "void run() { CHECK(other()) }",
      &observation
    )
    .unwrap_err(),
    Declined::SourceMismatch
  );
  assert_eq!(
    audit(Path::new("another.cc"), source, &observation).unwrap_err(),
    Declined::SourceMismatch
  );
  observation.complete = false;
  assert_eq!(
    audit(observation.path, source, &observation).unwrap_err(),
    Declined::Incomplete
  );
  observation.complete = true;
  observation.volatile_inputs = true;
  assert_eq!(
    audit(observation.path, source, &observation).unwrap_err(),
    Declined::VolatileInputs
  );
}

#[test]
fn missing_ordinary_semicolons_and_argument_failures_are_not_hidden() {
  for (source, invocation, eligible) in [
    (
      "void run() { CHECK(value()) ordinary() }",
      "CHECK(value())",
      true,
    ),
    (
      "void run() { CHECK(first(), second()) }",
      "CHECK(first(), second())",
      false,
    ),
    ("void run() { CHECK() }", "CHECK()", false),
    (
      "void run() { auto value = CHECK(next()); }",
      "CHECK(next())",
      false,
    ),
  ] {
    let definitions = [DefinitionBuffer {
      path: Path::new("proof.h"),
      source: HEADER,
    }];
    let expansions = [expansion(source, invocation, HEADER, "CHECK")];
    let names = BTreeSet::from(["CHECK".to_owned()]);
    let observation = Observation {
      path: Path::new("sample.cc"),
      source,
      definitions: &definitions,
      expansions: &expansions,
      expanded_names: &names,
      complete: true,
      volatile_inputs: false,
    };
    let report = audit(observation.path, source, &observation).unwrap();
    assert!(report.has_error, "{source}: {report:?}");
    assert_eq!(
      !report.eligible_names.is_empty(),
      eligible,
      "{source}: {report:?}"
    );
  }
}

#[test]
fn one_unobserved_use_remains_raw_without_disabling_an_independent_site() {
  for source in [
    "void run() { CHECK(first()) CHECK(second()) }",
    "void run() { CHECK(first()) }\n#undef CHECK\nvoid later() { CHECK(second()) }",
    "#if 0\nvoid inactive() { CHECK(second()) }\n#endif\nvoid run() { CHECK(first()) }",
  ] {
    let definitions = [DefinitionBuffer {
      path: Path::new("proof.h"),
      source: HEADER,
    }];
    let expansions = [expansion(source, "CHECK(first())", HEADER, "CHECK")];
    let names = BTreeSet::from(["CHECK".to_owned()]);
    let observation = Observation {
      path: Path::new("sample.cc"),
      source,
      definitions: &definitions,
      expansions: &expansions,
      expanded_names: &names,
      complete: true,
      volatile_inputs: false,
    };
    let report = audit(observation.path, source, &observation).unwrap();
    assert_eq!(report.eligible_names, ["CHECK"]);
    assert_eq!(report.macro_spans, [expansions[0].invocation.clone()]);
    assert!(report.has_error);
  }
}

#[test]
fn nested_argument_and_replacement_effects_decline_statement_proof() {
  for (source, header, nested) in [
    ("void run() { CHECK(NESTED()) }", HEADER, "NESTED"),
    (
      "void run() { CHECK(value()) }",
      "#define CHECK(x) { NESTED(x); }\n",
      "NESTED",
    ),
    (
      "void run() { CHECK(__pragma(warning(push))) }",
      HEADER,
      "CHECK",
    ),
  ] {
    let definitions = [DefinitionBuffer {
      path: Path::new("proof.h"),
      source: header,
    }];
    let invocation = if source.contains("__pragma") {
      "CHECK(__pragma(warning(push)))"
    } else if source.contains("NESTED()") {
      "CHECK(NESTED())"
    } else {
      "CHECK(value())"
    };
    let expansions = [expansion(source, invocation, header, "CHECK")];
    let names = BTreeSet::from(["CHECK".to_owned(), nested.to_owned()]);
    let observation = Observation {
      path: Path::new("sample.cc"),
      source,
      definitions: &definitions,
      expansions: &expansions,
      expanded_names: &names,
      complete: true,
      volatile_inputs: false,
    };
    let report = audit(observation.path, source, &observation).unwrap();
    assert!(report.eligible_names.is_empty());
    assert!(report.has_error);
  }
}

#[test]
fn invalid_anchors_overlaps_and_conflicting_physical_buffers_are_rejected() {
  let source = "void run() { CHECK(value()) }";
  let definitions = [DefinitionBuffer {
    path: Path::new("proof.h"),
    source: HEADER,
  }];
  let valid = expansion(source, "CHECK(value())", HEADER, "CHECK");
  let names = BTreeSet::from(["CHECK".to_owned()]);
  for (bad, expected) in [
    (
      {
        let mut e = valid.clone();
        e.definition.as_mut().unwrap().name_offset += 1;
        e
      },
      Declined::InvalidDefinition,
    ),
    (
      {
        let mut e = valid.clone();
        e.definition.as_mut().unwrap().buffer = 1;
        e
      },
      Declined::InvalidDefinition,
    ),
    (
      {
        let mut e = valid.clone();
        e.definition.as_mut().unwrap().end -= 1;
        e
      },
      Declined::InvalidDefinition,
    ),
    (
      {
        let mut e = valid.clone();
        e.invocation.end = source.len();
        e
      },
      Declined::InvalidExpansion,
    ),
  ] {
    let expansions = [bad];
    let observation = Observation {
      path: Path::new("sample.cc"),
      source,
      definitions: &definitions,
      expansions: &expansions,
      expanded_names: &names,
      complete: true,
      volatile_inputs: false,
    };
    assert_eq!(
      audit(observation.path, source, &observation).unwrap_err(),
      expected
    );
  }
  let expansions = [valid.clone(), valid];
  let mut observation = Observation {
    path: Path::new("sample.cc"),
    source,
    definitions: &definitions,
    expansions: &expansions,
    expanded_names: &names,
    complete: true,
    volatile_inputs: false,
  };
  assert_eq!(
    audit(observation.path, source, &observation).unwrap_err(),
    Declined::InvalidExpansion
  );
  let conflicting = [
    DefinitionBuffer {
      path: Path::new("proof.h"),
      source: HEADER,
    },
    DefinitionBuffer {
      path: Path::new("proof.h"),
      source: "#define CHECK(x) expression(x)\n",
    },
  ];
  observation.definitions = &conflicting;
  assert_eq!(
    audit(observation.path, source, &observation).unwrap_err(),
    Declined::ConflictingBuffers
  );
  let conflicting_source = [DefinitionBuffer {
    path: observation.path,
    source: HEADER,
  }];
  observation.definitions = &conflicting_source;
  assert_eq!(
    audit(observation.path, source, &observation).unwrap_err(),
    Declined::ConflictingBuffers
  );
}

#[test]
fn unsupported_definitions_cannot_turn_into_complete_statements() {
  for (header, parameters) in [
    ("#define CHECK(x) expression(x)\n", 1),
    ("#define CHECK(x) void generated() { sink(x); }\n", 1),
    // LLVM's MacroInfo count includes the implicit variadic parameter; the
    // original parser has only the one named identifier here.
    ("#define CHECK(x, ...) { sink(x); }\n", 2),
    ("#define CHECK(x, x) { sink(x); }\n", 2),
    ("#define CHECK(x) do { sink(x); } while(0)\n", 1),
  ] {
    let source = "void run() { CHECK(value()) }";
    let definitions = [DefinitionBuffer {
      path: Path::new("proof.h"),
      source: header,
    }];
    let mut e = expansion(source, "CHECK(value())", header, "CHECK");
    e.definition.as_mut().unwrap().parameters = parameters;
    let expansions = [e];
    let names = BTreeSet::from(["CHECK".to_owned()]);
    let observation = Observation {
      path: Path::new("sample.cc"),
      source,
      definitions: &definitions,
      expansions: &expansions,
      expanded_names: &names,
      complete: true,
      volatile_inputs: false,
    };
    let report = audit(observation.path, source, &observation).unwrap();
    assert!(report.eligible_names.is_empty(), "{header}: {report:?}");
    assert!(report.has_error);
  }
}

#[test]
fn member_sites_and_nested_thread_scanner_contexts_remain_original() {
  use vorpal_core::tree_sitter::LanguageExt;
  use vorpal_language::SupportLang;
  let source = "void run() { CHECK(first.empty()) after(); }";
  let run = || {
    let definitions = [DefinitionBuffer {
      path: Path::new("proof.h"),
      source: HEADER,
    }];
    let expansions = [expansion(source, "CHECK(first.empty())", HEADER, "CHECK")];
    let names = BTreeSet::from(["CHECK".to_owned()]);
    let observation = Observation {
      path: Path::new("sample.cc"),
      source,
      definitions: &definitions,
      expansions: &expansions,
      expanded_names: &names,
      complete: true,
      volatile_inputs: false,
    };
    let report = audit(observation.path, source, &observation).unwrap();
    assert!(!report.has_error, "{report:?}");
    let (name, span) = &report.member_calls[0];
    assert_eq!(name, "first.empty");
    assert_eq!(&source[span.clone()], "first.empty()");
    assert_eq!(span.start, source.find("first.empty()").unwrap());
  };
  std::thread::scope(|scope| {
    for _ in 0..4 {
      scope.spawn(|| {
        vorpal_language::with_cpp_statement_macros(&["OUTER".to_owned()], || {
          run();
          let outer = SupportLang::Cpp.grep("void run() { OUTER(value()) }");
          assert!(!outer.root().has_error());
        });
        assert!(SupportLang::Cpp.grep(source).root().has_error());
      });
    }
  });
}

#[test]
fn oversized_observations_decline_before_cloning_or_parsing() {
  let source = "void run() { CHECK(value()) }";
  let names = BTreeSet::from(["X".repeat(1024 * 1024 + 1)]);
  let observation = Observation {
    path: Path::new("sample.cc"),
    source,
    definitions: &[],
    expansions: &[],
    expanded_names: &names,
    complete: true,
    volatile_inputs: false,
  };
  assert_eq!(
    audit(observation.path, source, &observation).unwrap_err(),
    Declined::Limit
  );
  let names = BTreeSet::new();
  let header = " ".repeat(4 * 1024 * 1024 + 1);
  let definitions = [DefinitionBuffer {
    path: Path::new("large.h"),
    source: &header,
  }];
  let observation = Observation {
    definitions: &definitions,
    expanded_names: &names,
    ..observation
  };
  assert_eq!(
    audit(observation.path, source, &observation).unwrap_err(),
    Declined::Limit
  );
}

#[test]
fn function_generating_macros_do_not_become_original_function_names() {
  let source = "GENERATE(Case) { CHECK(value()) }\nvoid following() { after(); }";
  let generated = "#define GENERATE(name) void test_##name()\n";
  let definitions = [
    DefinitionBuffer {
      path: Path::new("proof.h"),
      source: HEADER,
    },
    DefinitionBuffer {
      path: Path::new("generate.h"),
      source: generated,
    },
  ];
  let mut expansions = [
    expansion(source, "CHECK(value())", HEADER, "CHECK"),
    expansion(source, "GENERATE(Case)", generated, "GENERATE"),
  ];
  expansions[1].definition.as_mut().unwrap().buffer = 1;
  let names = BTreeSet::from(["CHECK".to_owned(), "GENERATE".to_owned()]);
  let observation = Observation {
    path: Path::new("sample.cc"),
    source,
    definitions: &definitions,
    expansions: &expansions,
    expanded_names: &names,
    complete: true,
    volatile_inputs: false,
  };
  let report = audit(observation.path, source, &observation).unwrap();
  assert_eq!(report.eligible_names, ["CHECK"]);
  assert_eq!(report.functions, ["following"]);
  assert_eq!(report.macro_spans.len(), 1);
  assert!(
    !report
      .calls
      .iter()
      .any(|(name, _)| name == "GENERATE" || name == "CHECK")
  );
}

#[test]
fn an_expanding_member_name_does_not_become_a_member_call() {
  let source = "void run() { object.CHECK(value()); }";
  let definitions = [DefinitionBuffer {
    path: Path::new("proof.h"),
    source: HEADER,
  }];
  let expansions = [expansion(source, "CHECK(value())", HEADER, "CHECK")];
  let names = BTreeSet::from(["CHECK".to_owned()]);
  let observation = Observation {
    path: Path::new("sample.cc"),
    source,
    definitions: &definitions,
    expansions: &expansions,
    expanded_names: &names,
    complete: true,
    volatile_inputs: false,
  };
  let report = audit(observation.path, source, &observation).unwrap();
  assert!(report.has_error, "{report:?}");
  assert!(report.member_calls.is_empty(), "{report:?}");
  let (_, span) = report
    .calls
    .iter()
    .find(|(name, _)| name == "value")
    .unwrap();
  assert_eq!(&source[span.clone()], "value()");
}

#[test]
fn unsupported_variadic_anchors_keep_independent_proof_and_ordinary_same_name_calls() {
  let source = "void run() { CHECK(value()) object.LOG(\"cat\", \"member\"); LOG(\"cat\", \"macro\"); }\n#undef LOG\nvoid other() { LOG(\"cat\", \"ordinary\"); }";
  let variadic = "#define LOG(category, ...) dispatch(category, __VA_ARGS__)\n";
  let definitions = [
    DefinitionBuffer {
      path: Path::new("proof.h"),
      source: HEADER,
    },
    DefinitionBuffer {
      path: Path::new("log.h"),
      source: variadic,
    },
  ];
  let mut expansions = [
    expansion(source, "CHECK(value())", HEADER, "CHECK"),
    expansion(source, "LOG(\"cat\", \"member\")", variadic, "LOG"),
    expansion(source, "LOG(\"cat\", \"macro\")", variadic, "LOG"),
  ];
  for e in &mut expansions[1..] {
    let anchor = e.definition.as_mut().unwrap();
    anchor.buffer = 1;
    anchor.parameters = 2;
  }
  let names = BTreeSet::from(["CHECK".to_owned(), "LOG".to_owned()]);
  let observation = Observation {
    path: Path::new("sample.cc"),
    source,
    definitions: &definitions,
    expansions: &expansions,
    expanded_names: &names,
    complete: true,
    volatile_inputs: false,
  };
  let report = audit(observation.path, source, &observation).unwrap();
  assert_eq!(report.eligible_names, ["CHECK"]);
  assert_eq!(report.macro_spans.len(), 1);
  assert!(report.member_calls.is_empty(), "{report:?}");
  let ordinary = source.find("LOG(\"cat\", \"ordinary\")").unwrap();
  let calls: Vec<_> = report
    .calls
    .iter()
    .filter(|(name, _)| name == "LOG")
    .collect();
  assert_eq!(calls.len(), 1, "{report:?}");
  assert_eq!(calls[0].1.start, ordinary);
  let (_, span) = report
    .calls
    .iter()
    .find(|(name, _)| name == "value")
    .unwrap();
  assert_eq!(&source[span.clone()], "value()");
}

#[test]
fn independent_sites_keep_mixed_replacement_classes_and_decline_only_invalid_arguments() {
  let source = "void run() { CHECK(first()) if (flag) CHECK(second()) else after(); CHECK() }";
  let closed = "#define CHECK(x) { sink(x); }\n";
  let definitions = [
    DefinitionBuffer {
      path: Path::new("closed.h"),
      source: closed,
    },
    DefinitionBuffer {
      path: Path::new("open.h"),
      source: HEADER,
    },
  ];
  let mut expansions = [
    expansion(source, "CHECK(first())", closed, "CHECK"),
    expansion(source, "CHECK(second())", HEADER, "CHECK"),
    expansion(source, "CHECK()", HEADER, "CHECK"),
  ];
  expansions[1].definition.as_mut().unwrap().buffer = 1;
  expansions[2].definition.as_mut().unwrap().buffer = 1;
  let names = BTreeSet::from(["CHECK".to_owned()]);
  let observation = Observation {
    path: Path::new("sample.cc"),
    source,
    definitions: &definitions,
    expansions: &expansions,
    expanded_names: &names,
    complete: true,
    volatile_inputs: false,
  };
  let report = audit(observation.path, source, &observation).unwrap();
  assert_eq!(report.eligible_names, ["CHECK"]);
  assert_eq!(
    report.macro_spans,
    [
      expansions[0].invocation.clone(),
      expansions[1].invocation.clone()
    ]
  );
  assert!(
    report.has_error,
    "invalid empty argument still produces a real error"
  );
  for name in ["first", "second", "after"] {
    let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
    assert_eq!(&source[span.clone()], format!("{name}()"));
  }
  assert!(
    audit_recovery(observation.path, source, &[])
      .macro_spans
      .is_empty()
  );
}

#[test]
fn invalid_expression_context_retains_diagnostics_and_independent_statement_sites() {
  let source = "void run() { CHECK(first()); int bad = CHECK(second()); CHECK(last()) CHECK(1,) }";
  let header = "#define CHECK(x) { sink(x); }\n";
  let definitions = [DefinitionBuffer {
    path: Path::new("proof.h"),
    source: header,
  }];
  let expansions = [
    "CHECK(first())",
    "CHECK(second())",
    "CHECK(last())",
    "CHECK(1,)",
  ]
  .map(|invocation| expansion(source, invocation, header, "CHECK"));
  let names = BTreeSet::from(["CHECK".to_owned()]);
  let observation = Observation {
    path: Path::new("sample.cc"),
    source,
    definitions: &definitions,
    expansions: &expansions,
    expanded_names: &names,
    complete: true,
    volatile_inputs: false,
  };
  let report = audit(observation.path, source, &observation).unwrap();
  assert_eq!(report.eligible_names, ["CHECK"]);
  assert_eq!(
    report.macro_spans,
    [
      expansions[0].invocation.clone(),
      expansions[2].invocation.clone()
    ]
  );
  assert!(report.has_error);
  assert_eq!(report.context_errors, [expansions[1].invocation.clone()]);
  for name in ["first", "second", "last"] {
    let (_, span) = report.calls.iter().find(|(n, _)| n == name).unwrap();
    assert_eq!(&source[span.clone()], format!("{name}()"));
  }
}
