use vorpal_language::SupportLang;

#[allow(dead_code)]
mod common;

#[test]
fn csharp_rules_parse_and_extract_dotnet_shapes() {
  const RULES: &str = include_str!("../src/default_rules/csharp.yml");
  common::assert_outline_snapshot(
    SupportLang::CSharp,
    RULES,
    r#"
using System;
namespace Demo.Core;
public interface IService { void Run(); }
public class Parser { private int count; public Parser(int count) { this.count = count; } public string Parse(string input) { return input; } }
public enum Mode { Fast, Slow }
"#,
    r#"
- Module import private System
- Module item exported Demo.Core
- Interface item exported IService
  - Method public Run
- Class item exported Parser
  - Field private count
  - Constructor public Parser
  - Method public Parse
- Enum item exported Mode
  - EnumMember public Fast
  - EnumMember public Slow
"#,
  );
}

#[test]
fn c_rules_parse_and_extract_native_shapes() {
  const RULES: &str = include_str!("../src/default_rules/c.yml");
  common::assert_outline_snapshot(
    SupportLang::C,
    RULES,
    r#"
#include <stdio.h>
typedef struct Config { int value; } Config;
enum Mode { Fast, Slow };
int count;
int helper(int value) { return value; }
"#,
    r#"
- Module import private <stdio.h>
- Struct item exported Config
  - Field public value
- Enum item exported Mode
  - EnumMember public Fast
  - EnumMember public Slow
- Variable item exported count
- Function item exported helper
"#,
  );
}

#[test]
fn cpp_rules_parse_and_extract_native_shapes() {
  const RULES: &str = include_str!("../src/default_rules/cpp.yml");
  common::assert_outline_snapshot(
    SupportLang::Cpp,
    RULES,
    r#"
#include <vector>
namespace demo {
class Parser { public: Parser(); int parse(const char* input); private: int count; };
struct Config { int value; };
enum Mode { Fast, Slow };
int helper(int value) { return value; }
}
"#,
    r#"
- Module import private <vector>
- Module item exported demo
- Class item exported Parser
  - Constructor public Parser
  - Method public parse
  - Field private count
- Struct item exported Config
  - Field public value
- Enum item exported Mode
  - EnumMember public Fast
  - EnumMember public Slow
- Function item exported helper
"#,
  );
}

#[test]
fn c_declarator_shapes_extract_clean_names() {
  const RULES: &str = include_str!("../src/default_rules/c.yml");
  // Pointer/array/function-pointer declarators must never leak into names (`*allocation`),
  // pointer-returning function definitions must still be functions, initializers must not
  // ride along in variable names, prototypes must not become variables, and `struct` type
  // *references* must not mint phantom struct definitions.
  common::assert_outline_snapshot(
    SupportLang::C,
    RULES,
    r#"
struct pool {
  int plain;
  void *allocation;
  int buf[4];
  unsigned bits : 3;
  int (*cb)(int);
  struct pool *next;
};
int plain_fn(int a) { return a; }
void *ptr_fn(int a) { return 0; }
static struct pool *sptr_fn(void) { return 0; }
int global_plain;
int global_init = 3;
char *global_ptr;
int global_arr[8];
void *proto_fn(int);
"#,
    r#"
- Struct item exported pool
  - Field public plain
  - Field public allocation
  - Field public buf
  - Field public bits
  - Field public cb
  - Field public next
- Function item exported plain_fn
- Function item exported ptr_fn
- Function item private sptr_fn
- Variable item exported global_plain
- Variable item exported global_init
- Variable item exported global_ptr
- Variable item exported global_arr
"#,
  );
}

#[test]
fn cpp_declarator_shapes_extract_clean_names() {
  const RULES: &str = include_str!("../src/default_rules/cpp.yml");
  // Same declarator discipline for C++, plus the classification split: a pointer-returning
  // method declaration is a *method* (not a `*ptr_method(int x)` field), and a
  // function-pointer member is a *field* named `cb` (not a `(*cb)` method).
  common::assert_outline_snapshot(
    SupportLang::Cpp,
    RULES,
    r#"
class Widget {
public:
  int plain;
  char *cursor;
  int *ptr_method(int x);
  int plain_method(int x);
  int (*cb)(int);
};
void *free_ptr_fn() { return 0; }
int free_fn() { return 1; }
"#,
    r#"
- Class item exported Widget
  - Field public plain
  - Field public cursor
  - Method public ptr_method
  - Method public plain_method
  - Field public cb
- Function item exported free_ptr_fn
- Function item exported free_fn
"#,
  );
}


#[test]
fn c_struct_definition_with_trailing_declarator_mints_the_type() {
  // `struct X { ... } tail;` is first and foremost a TYPE definition — the kernel's
  // `__randomize_layout` / `__packed` idiom parses exactly like a variable declaration
  // with an inline struct type, and the variable item used to swallow the subtree so the
  // struct never existed (file_operations, cpuinfo_x86 at kernel scale). Named+body
  // specifiers win; anonymous bodies and body-less type references keep their variables.
  const RULES: &str = include_str!("../src/default_rules/c.yml");
  common::assert_outline_snapshot(
    SupportLang::C,
    RULES,
    r#"
struct file_operations { int owner; } __randomize_layout;
union addr { int v4; } __packed;
struct { int x; } anonymous_var;
struct forward_only decl_var;
"#,
    r#"
- Struct item exported file_operations
  - Field public owner
- Union item exported addr
  - Field public v4
- Variable item exported anonymous_var
- Variable item exported decl_var
"#,
  );
}

/// The parser-swallow shape (cpython `Objects/object.c` from `_PyObject_GetAttrId`):
/// bare statement-position macros wreck a body, tree-sitter loses the closing brace and
/// parses every later definition INSIDE that body with no top-level ERROR. The recovery
/// walk must lift them all, keep the swallower's locals out, cut the swallower's span
/// back to its real body, and report the recovery.
#[test]
fn c_swallowed_tail_definitions_are_lifted_as_items() {
  use vorpal_language::LanguageExt as _;
  use vorpal_outline::{DEFAULT_OUTLINE_RULES, combined_extractor::CombinedExtractors, extractor::parse_outline_rules};
  let rules = parse_outline_rules::<SupportLang>(DEFAULT_OUTLINE_RULES)
    .expect("rules parse")
    .into_iter()
    .filter(|r| r.common().language == SupportLang::C)
    .collect::<Vec<_>>();
  let combined = CombinedExtractors::try_from(rules, &Default::default()).expect("rules compile");
  let source = r#"#include "Python.h"

static int counter = 0;

int
before_swallow(int x)
{
    return x + counter;
}

PyObject *
_PyObject_GetAttrId(PyObject *v, _Py_Identifier *name)
{
    PyObject *result;
_Py_COMP_DIAG_PUSH
_Py_COMP_DIAG_IGNORE_DEPR_DECLS
    PyObject *oname = _PyUnicode_FromId(name); /* borrowed */
_Py_COMP_DIAG_POP
    if (!oname)
        return NULL;
    result = PyObject_GetAttr(v, oname);
    return result;
}

int
_PyObject_SetAttributeErrorContext(PyObject* v, PyObject* name)
{
    assert(PyErr_Occurred());
    return 0;
}

PyObject *
PyObject_GetAttr(PyObject *v, PyObject *name)
{
    PyTypeObject *tp = Py_TYPE(v);
    return NULL;
}

#define SWALLOWED_MACRO(x) ((x) + 1)

struct swallowed_record {
    int field;
};

typedef struct swallowed_record swallowed_t;

static PyNumberMethods none_as_number = {
    0,
};

PyObject _Py_NoneStruct = _PyObject_HEAD_INIT(&_PyNone_Type);

int
PyCallable_Check(PyObject *x)
{
    int local_in_lifted = 0;
    if (x == NULL)
        return 0;
    return local_in_lifted;
}
"#;
  let grep = SupportLang::C.grep(source);
  assert!(grep.root().has_error(), "the fixture must carry the parse damage it models");
  let mut report = Vec::new();
  let items = combined.extract_with(grep.root(), &mut report).collect::<Vec<_>>();
  let names: Vec<&str> = items.iter().map(|i| i.entry.name.as_ref()).collect();
  assert_eq!(
    names,
    vec![
      "\"Python.h\"",
      "counter",
      "before_swallow",
      "_PyObject_GetAttrId",
      "PyObject_GetAttr",
      "SWALLOWED_MACRO",
      "swallowed_record",
      "swallowed_t",
      "none_as_number",
      "_Py_NoneStruct",
      "PyCallable_Check",
    ],
    "lifted in document order; locals (`result`, `oname`, `local_in_lifted`) and the \
     wreckage blob's `if` never surface"
  );
  // The swallower's span is cut back to its real body: it ends before the floor, which is
  // the first clean nested definition (`PyObject_GetAttr`) — the keyword-named fusion
  // blob (`_Py_COMP_DIAG_POP if (!oname) … { <next function's body> }`) is neither the
  // floor nor an item, so the swallower's span runs through it.
  let swallower = &items[3];
  let floor = source.find("PyObject *\nPyObject_GetAttr(").expect("floor text");
  assert!(
    swallower.entry.range.byte_offset.end <= floor,
    "swallower must not span to EOF: ends at {} (floor {floor})",
    swallower.entry.range.byte_offset.end
  );
  // Zero-based lines: the `PyObject *` return-type line through the closing brace the
  // parser fused into the blob (the real `_PyObject_SetAttributeErrorContext` body's).
  assert_eq!(swallower.entry.range.start.line, 10);
  assert_eq!(swallower.entry.range.end.line, 29);
  // Lifted items carry their real spans.
  let get_attr = &items[4];
  let get_attr_start = source.find("PyObject *\nPyObject_GetAttr(").expect("def text");
  assert_eq!(get_attr.entry.range.byte_offset.start, get_attr_start);
  assert_eq!(items[6].members.len(), 1, "a lifted struct keeps its members");
  assert_eq!(
    report,
    vec![vorpal_outline::model::SwallowRecovery {
      start: swallower.entry.range.byte_offset.start as u32,
      lifted: 7,
    }]
  );
}

/// The kernel shapes: a macro-with-block idiom (`scoped_guard(x) { … }`) fuses the
/// swallower's tail into a function-shaped blob; `for_each_*(x) { }` loops parse as
/// nested function definitions with a parenthesized declarator. Neither may be lifted or
/// become a false floor; the swallower's span ends at the blob (its real closing brace).
#[test]
fn c_swallow_recovery_ignores_macro_block_wreckage() {
  use vorpal_language::LanguageExt as _;
  use vorpal_outline::{DEFAULT_OUTLINE_RULES, combined_extractor::CombinedExtractors, extractor::parse_outline_rules};
  let rules = parse_outline_rules::<SupportLang>(DEFAULT_OUTLINE_RULES)
    .expect("rules parse")
    .into_iter()
    .filter(|r| r.common().language == SupportLang::C)
    .collect::<Vec<_>>();
  let combined = CombinedExtractors::try_from(rules, &Default::default()).expect("rules compile");
  let source = r#"void clock_was_set(unsigned int bases)
{
	cpumask_var_t mask;

	for_each_online_cpu(cpu) {
		struct hrtimer_cpu_base *cpu_base = &per_cpu(hrtimer_bases, cpu);
		cpumask_set_cpu(cpu, mask);
	}
	scoped_guard(cpus_read_lock) {
		int cpu;

		scoped_guard(preempt)
			smp_call_function_many(mask, retrigger_next_event, NULL, 1);
	}
	free_cpumask_var(mask);

out_timerfd:
	timerfd_clock_was_set();
}

static void clock_was_set_work(struct work_struct *work)
{
	clock_was_set(CLOCK_SET_WALL);
}

static DECLARE_WORK(hrtimer_work, clock_was_set_work);

void hrtimer_start_range_ns(struct hrtimer *timer, ktime_t tim)
{
	int local = 0;
}
"#;
  let grep = SupportLang::C.grep(source);
  assert!(grep.root().has_error());
  let mut report = Vec::new();
  let items = combined.extract_with(grep.root(), &mut report).collect::<Vec<_>>();
  let names: Vec<&str> = items.iter().map(|i| i.entry.name.as_ref()).collect();
  // The empty-named variable is `static DECLARE_WORK(…);` — parity with the ordinary
  // top-level traversal, which mints the same item for it (a MISSING identifier).
  assert_eq!(
    names,
    vec!["clock_was_set", "clock_was_set_work", "", "hrtimer_start_range_ns"],
    "no `cpu`, `cpu_base`, `mask`, `scoped_guard`, `for_each_online_cpu`, or `local`"
  );
  assert_eq!(report.len(), 1);
  assert_eq!(report[0].lifted, 3);
  let end = items[0].entry.range.byte_offset.end;
  assert_eq!(
    &source[..end].trim_end().lines().last().unwrap_or(""),
    &"}",
    "the swallower ends at its real closing brace"
  );
}

/// No swallow, no change: the file's last definition carrying an internal error but no
/// nested definition is extracted exactly as before (the diagnosis needs a floor).
#[test]
fn c_last_definition_with_body_damage_is_not_a_swallow() {
  use vorpal_language::LanguageExt as _;
  use vorpal_outline::{DEFAULT_OUTLINE_RULES, combined_extractor::CombinedExtractors, extractor::parse_outline_rules};
  let rules = parse_outline_rules::<SupportLang>(DEFAULT_OUTLINE_RULES)
    .expect("rules parse")
    .into_iter()
    .filter(|r| r.common().language == SupportLang::C)
    .collect::<Vec<_>>();
  let combined = CombinedExtractors::try_from(rules, &Default::default()).expect("rules compile");
  let source = "int a;\nvoid last(void)\n{\n\tint x = ;\n\tstruct local_only { int f; } v;\n}\n";
  let grep = SupportLang::C.grep(source);
  assert!(grep.root().has_error());
  let mut report = Vec::new();
  let items = combined.extract_with(grep.root(), &mut report).collect::<Vec<_>>();
  let names: Vec<&str> = items.iter().map(|i| i.entry.name.as_ref()).collect();
  assert_eq!(names, vec!["a", "last"]);
  assert!(report.is_empty());
  assert_eq!(items[1].entry.range.byte_offset.end, source.trim_end().len());
}

#[test]
fn cpp_member_templates_keep_method_kinds_and_enclosing_access() {
  const RULES: &str = include_str!("../src/default_rules/cpp.yml");
  common::assert_outline_snapshot(
    SupportLang::Cpp,
    RULES,
    r#"
struct DefaultPublic {
  DefaultPublic() {}
  ~DefaultPublic() {}
  template<class T> int choose(T) { return 1; }
  template<class T> int declared(T);
  int (*callback)(int);
  int (*filtered)(int predicate(int));
  int* (*pointerFiltered)(int callback(int));
  int (*factory())(int predicate(int));
  template<class T> int (*create(T))(int predicate(int));
private:
  template<class T> int hidden(T) { return 0; }
};
class Labeled {
  template<class T> int hidden(T) { return 0; }
public:
  template<class T> Labeled(T) {}
  template<class T> int visible(T) { return 1; }
protected:
  template<class T> int guarded(T);
};
"#,
    r#"
- Struct item exported DefaultPublic
  - Constructor public DefaultPublic
  - Constructor public ~DefaultPublic
  - Method public choose
  - Method public declared
  - Field public callback
  - Field public filtered
  - Field public pointerFiltered
  - Method public factory
  - Method public create
  - Method private hidden
- Class item exported Labeled
  - Method private hidden
  - Constructor public Labeled
  - Method public visible
  - Method private guarded
"#,
  );
}

#[test]
fn cpp_friend_definitions_are_functions_and_prototypes_are_not_members() {
  const RULES: &str = include_str!("../src/default_rules/cpp.yml");
  let lf = r#"
struct Stream {};
struct Other { Other(); };
class Column {
  friend void inspect(Column&);
  friend Other::Other();
  friend class Peer;
  inline friend Stream& operator<<(Stream& os, Column const& col) { work(); return os; }
  inline friend int visit(Column const& col) { return work(); }
  friend int* pointer(Column& col) { return nullptr; }
  template<typename T> friend T echo(T value) { return value; }
  friend void local() { struct Local { void nested() { work(); } }; work(); }
public:
  int size() const { return count(); }
};
int ordinary() { return work(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    common::assert_outline_snapshot(
      SupportLang::Cpp,
      RULES,
      &source,
      r#"
- Struct item exported Stream
- Struct item exported Other
  - Constructor public Other
- Class item exported Column
  - Method public size
- Function item exported ordinary
- Function item exported operator<<
- Function item exported visit
- Function item exported pointer
- Function item exported echo
- Function item exported local
"#,
    );
  }
}

#[test]
fn cpp_constructor_names_must_match_their_enclosing_type() {
  const RULES: &str = include_str!("../src/default_rules/cpp.yml");
  let lf = "#define DECLARE_STORAGE(Type) int* storage();\nstruct Holder {\n Holder();\n ~Holder();\n int* getter() { return value(); }\n DECLARE_STORAGE(Item);\n};\nHolder::Holder() { work(); }\nHolder::~Holder() { cleanup(); }\nvoid following() { after(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    common::assert_outline_snapshot(
      SupportLang::Cpp,
      RULES,
      &source,
      r#"
- Macro item exported DECLARE_STORAGE
- Struct item exported Holder
  - Constructor public Holder
  - Constructor public ~Holder
  - Method public getter
- Function item exported Holder::Holder
- Function item exported Holder::~Holder
- Function item exported following
"#,
    );
  }
  // A known type name is not a constructor of a different enclosing class.
  common::assert_outline_snapshot(
    SupportLang::Cpp,
    RULES,
    "struct Other {}; struct Holder { Other(); Holder(); ~Holder(); };",
    "- Struct item exported Other\n- Struct item exported Holder\n  - Constructor public Holder\n  - Constructor public ~Holder\n",
  );
}

#[test]
fn cpp_specialized_and_nested_constructors_keep_the_injected_class_name() {
  const RULES: &str = include_str!("../src/default_rules/cpp.yml");
  let lf = "template<class T> struct Box { Box(); ~Box(); };\ntemplate<class T> struct Box<T*> { Box() { work(); } ~Box(); };\ntemplate<> struct Box<void> { template<class T> Box(T value) { work(); } ~Box(); };\nstruct Outer { Outer(); struct Inner { Inner(); ~Inner(); Outer(); }; };\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    common::assert_outline_snapshot(
      SupportLang::Cpp,
      RULES,
      &source,
      r#"
- Struct item exported Box
  - Constructor public Box
  - Constructor public ~Box
- Struct item exported Box<T*>
  - Constructor public Box
  - Constructor public ~Box
- Struct item exported Box<void>
  - Constructor public Box
  - Constructor public ~Box
- Struct item exported Outer
  - Constructor public Outer
  - Struct private Inner
"#,
    );
  }
}

#[test]
fn cpp_constructor_matcher_stops_at_the_nearest_class() {
  use vorpal_language::LanguageExt;
  use vorpal_outline::extractor::{MemberExtractor, SerializableOutlineRule, parse_outline_rules};
  use vorpal_outline::options::OutlineEntryDetail;
  let rules =
    parse_outline_rules::<SupportLang>(include_str!("../src/default_rules/cpp.yml")).unwrap();
  let rule = rules
    .into_iter()
    .find(|r| r.common().id == "cpp-member-constructor")
    .unwrap();
  let SerializableOutlineRule::Member(rule) = rule else {
    panic!("member rule")
  };
  let matcher =
    MemberExtractor::try_from(rule, &Default::default(), OutlineEntryDetail::Signature).unwrap();
  let source = "struct Outer { Outer(); struct Inner { Inner(); Outer(); }; };";
  let parsed = SupportLang::Cpp.grep(source);
  let matches: Vec<_> = parsed
    .root()
    .dfs()
    .filter_map(|node| matcher.match_node(&node))
    .map(|node| node.get_node().text().into_owned())
    .collect();
  assert_eq!(matches, ["Outer();", "Inner();"]);
}

#[test]
fn cpp_qualified_class_definitions_keep_only_the_terminal_constructor_name() {
  const RULES: &str = include_str!("../src/default_rules/cpp.yml");
  let lf = "struct Outer { struct Inner; };\nstruct Outer::Inner { Inner(); ~Inner(); Outer(); };\nnamespace ns { template<class T> struct Box { struct Nested; }; }\ntemplate<class T> struct ns::Box<T>::Nested { Nested(); ~Nested(); Box(); T(); };\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    common::assert_outline_snapshot(
      SupportLang::Cpp,
      RULES,
      &source,
      r#"
- Struct item exported Outer
  - Struct private Inner
- Struct item exported Outer::Inner
  - Constructor public Inner
  - Constructor public ~Inner
- Module item exported ns
- Struct item exported Box
  - Struct private Nested
- Struct item exported ns::Box<T>::Nested
  - Constructor public Nested
  - Constructor public ~Nested
"#,
    );
  }
}

#[test]
fn cpp_unexpanded_macro_heads_do_not_become_functions_or_constructors() {
  const RULES: &str = include_str!("../src/default_rules/cpp.yml");
  let lf = "#define TEST(name) void test_##name()\n#define DECLARE_STORAGE(Type) int* storage();\nstruct Holder {\n Holder();\n ~Holder();\n int* getter() { return value(); }\n DECLARE_STORAGE(Item);\n};\nTEST(originalTest) { use(); }\n#undef TEST\nvoid TEST(int value) { use(); }\nHolder::Holder() { work(); }\nHolder::~Holder() { cleanup(); }\nvoid following() { after(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    common::assert_outline_snapshot(
      SupportLang::Cpp,
      RULES,
      &source,
      r#"
- Macro item exported TEST
- Macro item exported DECLARE_STORAGE
- Struct item exported Holder
  - Constructor public Holder
  - Constructor public ~Holder
  - Method public getter
- Function item exported TEST
- Function item exported Holder::Holder
- Function item exported Holder::~Holder
- Function item exported following
"#,
    );
  }
  // A constructor name from an outer type cannot validate a nested foreign head.
  common::assert_outline_snapshot(
    SupportLang::Cpp,
    RULES,
    "struct Other {}; struct Holder { Other(); Holder(); ~Holder(); };",
    "- Struct item exported Other\n- Struct item exported Holder\n  - Constructor public Holder\n  - Constructor public ~Holder\n",
  );
}

#[test]
fn anonymous_cpp_bodies_remain_boundaries_through_output_filters() {
  use vorpal_language::LanguageExt;
  use vorpal_outline::{
    combined_extractor::CombinedExtractors, extractor::parse_outline_rules, model::SymbolType,
    options::OutlineExtractorOptions,
  };
  let source = "struct Global {};\nGENERATE(one) { struct Local {}; }\nvoid real() { struct AlsoLocal {}; }\nstruct Following {};\n";
  for text in [source.to_owned(), source.replace('\n', "\r\n")] {
    for symbol_types in [None, Some(vec![SymbolType::Struct])] {
      let rules =
        parse_outline_rules::<SupportLang>(include_str!("../src/default_rules/cpp.yml")).unwrap();
      let combined = CombinedExtractors::try_from_rules(
        rules,
        OutlineExtractorOptions {
          symbol_types,
          ..Default::default()
        },
        &Default::default(),
      )
      .unwrap();
      let parsed = SupportLang::Cpp.grep(&text);
      let items: Vec<_> = combined.extract(parsed.root()).collect();
      assert!(items.iter().any(|i| i.entry.name == "Global"));
      assert!(items.iter().any(|i| i.entry.name == "Following"));
      assert!(
        !items
          .iter()
          .any(|i| matches!(i.entry.name.as_ref(), "GENERATE" | "Local" | "AlsoLocal"))
      );
    }
  }
}
