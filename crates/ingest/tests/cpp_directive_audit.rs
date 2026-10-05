use vorpal_ingest::cpp_directive_audit::audit;

#[test]
fn guard_groups_keep_nested_alternatives_without_selecting_a_branch() {
  use vorpal_ingest::cpp_directive_audit::audit_groups;
  let lf = "# /* directive */ if UNEXPANDED\nfirst;\n#ifdef INNER\ninner;\n#else\nother;\n#endif\n#elif OTHER\nsecond;\n#else\nlast;\n#endif\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let groups = audit_groups(&source).unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].span, 0..source.len());
    assert_eq!(groups[0].branches.len(), 3);
    assert_eq!(groups[1].branches.len(), 2);
    assert!(source[groups[0].branches[0].body.clone()].contains("#ifdef INNER"));
    assert_eq!(source[groups[0].branches[1].body.clone()].trim(), "second;");
    assert_eq!(source[groups[0].branches[2].body.clone()].trim(), "last;");
    for group in groups {
      assert_eq!(source[group.close].trim(), "#endif");
    }
  }
  for source in [
    "#if1\n#endif\n",
    "#if\\u0058\n#endif\n",
    "#else\n",
    "#endif\n",
    "#if UNKNOWN\n",
    "#ifdef X\n#else\n#else\n#endif\n",
    "#if X\n#else\n#elif Y\n#endif\n",
  ] {
    assert!(audit_groups(source).is_err(), "{source}");
  }
}

#[test]
fn block_comment_newlines_do_not_end_or_start_a_directive() {
  let lf = "#define CHECK(x) /* comment\n*/ { sink(x); }\nvoid run() { CHECK(1) }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let spans = audit(&source).unwrap();
    assert_eq!(spans.len(), 1);
    assert!(
      source[spans[0].span.clone()].ends_with(if source.contains('\r') {
        "{ sink(x); }\r\n"
      } else {
        "{ sink(x); }\n"
      })
    );
  }
  // MSVC rejects this # as a directive after ordinary code on the same logical line.
  assert!(
    audit("int x; /* comment\n*/ #define X 1\n")
      .unwrap()
      .is_empty()
  );
}

#[test]
fn unexpanded_declarations_do_not_hide_original_directive_spans() {
  let lf = "#ifndef GUARD\n#define GUARD\n#include <header>\n_STD_BEGIN\n_EXPORT_STD extern TYPE object;\n#if UNKNOWN\n_DECLSPEC_MACRO something;\n#else\nother;\n#endif\n_STD_END\n#endif\n";
  let expected = [
    "#ifndef GUARD",
    "#define GUARD",
    "#include <header>",
    "#if UNKNOWN",
    "#else",
    "#endif",
    "#endif",
  ];
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let directives = audit(&source).unwrap();
    assert_eq!(
      directives
        .iter()
        .map(|d| source[d.span.clone()].trim_end())
        .collect::<Vec<_>>(),
      expected
    );
  }
}

#[test]
fn literals_comments_continuations_and_bom_keep_original_coordinates() {
  let lf = "\u{feff} /* before */ #define VALUE(x) \\\n  { use(x); }\nconst char* text = R\"tag(\n#define HIDDEN 1\n)tag\";\nconst char* escaped = \"\\\"#undef VALUE\";\nchar quote = '\\''; int n = 1'000;\n// comment \\\n#define COMMENTED 2\n/* block\n#define BLOCKED 3\n*/ #undef VALUE\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let spans = audit(&source).unwrap();
    assert_eq!(spans.len(), 2);
    assert!(source[spans[0].span.clone()].starts_with("#define VALUE(x) \\"));
    assert_eq!(source[spans[1].span.clone()].trim_end(), "#undef VALUE");
    for span in spans {
      assert!(source.is_char_boundary(span.span.start));
      assert!(source.is_char_boundary(span.span.end));
    }
  }
}

#[test]
fn malformed_or_unmodeled_lexical_boundaries_fail_closed() {
  for source in [
    "/* unterminated",
    "const char* x = R\"(unterminated",
    "\"broken\n#define FAKE 1\n",
    "char x = '\\",
    "/\\\n* #define FAKE 1 */",
    "#define VALUE /\\\n* comment\n#define FAKE 1 */",
    "%:define ALTERNATIVE 1",
    "??=define TRIGRAPH 1",
    "#define X 1\r#define Y 2",
  ] {
    assert!(audit(source).is_err(), "{source}");
  }
  assert!(
    audit("ordinary #define NOT_A_DIRECTIVE 1\n")
      .unwrap()
      .is_empty()
  );
  let source = "#define LAST(x) { use(x); }";
  assert_eq!(&source[audit(source).unwrap()[0].span.clone()], source);
}
