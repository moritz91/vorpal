#[test]
fn cpp_explicit_member_operators_preserve_callees_and_original_spans() {
  let lf = r#"
struct Stream {
  Stream& operator<<(int);
  int operator()(int);
  int operator[](int);
  Stream& operator++(int);
  template<typename T> Stream& operator>>(T);
};
int value();
int index();
void after();
void run(Stream& stream, Stream* ptr) {
  stream.operator<<(value());
  ptr->operator()(value());
  stream.operator[](index());
  ptr->operator++(0);
  stream.operator>> <int>(value());
  after();
}
void following() { after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert!(product.items.iter().any(|item| item.entry.name == "following"));
    for (name, call) in [
      ("operator<<", "stream.operator<<(value())"),
      ("operator()", "ptr->operator()(value())"),
      ("operator[]", "stream.operator[](index())"),
      ("operator++", "ptr->operator++(0)"),
      ("operator>>", "stream.operator>> <int>(value())"),
    ] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 1, "{name}: {:?}", product.refs);
      assert_eq!(&source[calls[0].start as usize..calls[0].end as usize], call);
    }
    assert_eq!(product.refs.iter().filter(|r| r.kind == 0 && r.name == "value").count(), 3);
    assert_eq!(product.refs.iter().filter(|r| r.kind == 0 && r.name == "index").count(), 1);
  }
  for source in [
    "void run() { stream.operator(value()); }",
    "void run() { stream.operator<<(value()) after(); }",
    "void run() { stream.operator<< <int>(value(); }",
    "void run() { stream.*operator<<(value()); }",
    "void run() { stream->*operator<<(value()); }",
  ] {
    assert!(SgLang::from_path("operators.cc").unwrap().grep(source).root().has_error(), "{source}");
  }
}

#[test]
fn cpp_trailing_primitive_type_arguments_do_not_invent_macro_runtime_callees() {
  let lf = r#"
#define TYPE_META(v, t) ((void)(v), sizeof(t))
int value();
void consume(unsigned long long);
void run() {
  consume(TYPE_META(value(), int));
  auto size = TYPE_META(value(), int*);
  auto converted = TYPE_META(int(value()), float);
}
#undef TYPE_META
int TYPE_META(int a, int b) { return a + b; }
void ordinary() { TYPE_META(1, 2); }
struct Plain {};
struct Value {};
void declarations() { int(Plain::*callback)(int, float); int(Plain::*typed)(Value, int); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert!(
      product
        .items
        .iter()
        .any(|item| item.entry.name == "ordinary")
    );
    assert!(
      product
        .items
        .iter()
        .any(|item| item.entry.name == "TYPE_META")
    );
    let meta_calls: Vec<_> = product
      .refs
      .iter()
      .filter(|r| r.kind == 0 && r.name == "TYPE_META")
      .collect();
    assert_eq!(meta_calls.len(), 1);
    assert_eq!(
      &source[meta_calls[0].start as usize..meta_calls[0].end as usize],
      "TYPE_META(1, 2)"
    );
    assert_eq!(
      product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == "value")
        .count(),
      3
    );
    let outer = product
      .refs
      .iter()
      .find(|r| r.kind == 0 && r.name == "consume")
      .unwrap();
    assert_eq!(
      &source[outer.start as usize..outer.end as usize],
      "consume(TYPE_META(value(), int))"
    );
    assert!(
      !product
        .refs
        .iter()
        .any(|r| r.kind == 0 && matches!(r.name.as_str(), "callback" | "typed"))
    );
  }
  for source in [
    "void run() { TYPE_META(value(), int }",
    "void run() { TYPE_META(value() int); }",
  ] {
    assert!(
      SgLang::from_path("metadata.cc")
        .unwrap()
        .grep(source)
        .root()
        .has_error()
    );
  }
}

#[test]
fn cpp_explicit_template_callbacks_preserve_types_and_runtime_call_spans() {
  let lf = r#"
struct Name {};
struct Vector2 { float x, y; };
template<class F> void Bind(const Name&, F) {}
template void Bind(const Name&, Vector2 (*)(float, float));
namespace Callbacks {
  template<class F> void Register(const Name&, F) {}
}
template void Callbacks::Register(const Name&, Vector2 (*)(float, float));
#define _In_
#define _In_reads_(x)
template void Bind(const Name&, _In_ int (*)(float, float));
template void Bind(const Name&, _In_reads_(2) int (*)(float, float));
void following(Name name) { Bind(name, value()); after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert!(
      product
        .items
        .iter()
        .any(|item| item.entry.name == "following")
    );
    for expected in ["Bind(name, value())", "value()", "after()"] {
      assert!(
        product
          .refs
          .iter()
          .any(|r| r.kind == 0 && &source[r.start as usize..r.end as usize] == expected),
        "missing {expected}"
      );
    }
    assert_eq!(
      product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == "Bind")
        .count(),
      1
    );
    assert!(!product.refs.iter().any(|r| r.kind == 0
      && matches!(
        r.name.as_str(),
        "Register" | "Vector2" | "_In_" | "_In_reads_"
      )));
    let parsed = SgLang::from_path("callbacks.cc").unwrap().grep(&source);
    let instantiations: Vec<_> = parsed
      .root()
      .dfs()
      .filter(|n| n.kind().as_ref() == "template_instantiation")
      .collect();
    assert_eq!(instantiations.len(), 4);
    for node in instantiations {
      assert_eq!(
        node.field("declarator").unwrap().kind().as_ref(),
        "function_declarator"
      );
    }
  }
  for source in [
    "template void Bind(Vector2 (*)(float, float);",
    "template void Bind(Vector2 (*)(float first float second));",
    "template void Bind(Vector2 (*)(float, float))",
    "void run() { Bind(value()) missing() }",
  ] {
    assert!(
      SgLang::from_path("callbacks.cc")
        .unwrap()
        .grep(source)
        .root()
        .has_error(),
      "silenced {source}"
    );
  }
}

use vorpal_ingest::OutlineExtractor;
use vorpal_core::{Language, tree_sitter::LanguageExt};
use vorpal_lang_registry::SgLang;

#[test]
fn cpp_inline_sdk_member_definitions_preserve_body_and_call_spans() {
  let source = r#"
class Base { public: virtual long SDKCALL Draw(int) = 0; };
class Renderer : public Base {
public:
  Renderer() { initialize(); }
  int Ordinary() { return ordinary(); }
  long SDKCALL Draw(int value) override { return draw(value); }
  unsigned SDKCALL AddRef() { return retain(); }
  long __stdcall Native(int value) { return native(value); }
};
void following() { after(); }
"#;
  let product = clean_product(source);
  let renderer = product
    .items
    .iter()
    .find(|item| item.entry.name == "Renderer")
    .unwrap();
  for name in ["Draw", "AddRef", "Native", "Ordinary", "Renderer"] {
    assert!(
      renderer
        .members
        .iter()
        .any(|member| member.entry.name == name),
      "missing {name}: {:?}",
      renderer
        .members
        .iter()
        .map(|m| &m.entry.name)
        .collect::<Vec<_>>()
    );
  }
  for member in &renderer.members {
    let expected = if member.entry.name == "Renderer" {
      vorpal_outline::model::SymbolType::Constructor
    } else {
      vorpal_outline::model::SymbolType::Method
    };
    assert_eq!(member.entry.symbol_type, expected);
    assert!(member.is_public);
  }
  assert!(
    product
      .items
      .iter()
      .any(|item| item.entry.name == "following")
  );
  for call_text in [
    "draw(value)",
    "retain()",
    "native(value)",
    "ordinary()",
    "initialize()",
    "after()",
  ] {
    assert!(
      product
        .refs
        .iter()
        .any(|r| r.kind == 0 && &source[r.start as usize..r.end as usize] == call_text),
      "missing {call_text}"
    );
  }
  assert!(
    !product
      .refs
      .iter()
      .any(|r| r.kind == 0 && r.name == "SDKCALL")
  );
  for invalid in [
    "struct Broken { long SDKCALL Draw(int) { return value } };",
    "struct Broken { long SDKCALL Draw(int) { return; };",
    "void f() { ordinary() }",
  ] {
    assert!(
      SgLang::from_path("sdk.cc")
        .unwrap()
        .grep(invalid)
        .root()
        .has_error()
    );
  }
}

#[test]
fn cpp_decltype_bases_preserve_following_definitions() {
  let source = r#"
struct Base {};
Base produce();
struct Derived : decltype(produce()) {};
template <typename Fun, typename... Args>
struct Callable : decltype(tester::test<Fun, Args...>(0)) {};
struct Other {};
struct Multiple : public decltype(produce()), Other {};
void following() { after(); }
void ordinary() { produce(); }
"#;
  let product = clean_product(source);
  for name in ["Base", "Derived", "Callable", "Multiple", "following"] {
    assert!(product.items.iter().any(|item| item.entry.name == name), "missing {name}");
  }
  let call = product.refs.iter().find(|r| r.name == "after" && r.kind == 0).unwrap();
  assert_eq!(&source[call.start as usize..call.end as usize], "after()");
  let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == "produce").collect();
  assert_eq!(calls.len(), 1);
  assert_eq!(calls[0].start as usize, source.rfind("produce()").unwrap());
  assert!(!product.refs.iter().any(|r| r.kind == 0 && r.name == "test"));
  for invalid in ["struct Broken : decltype() {};", "struct Broken : decltype(produce()) Base {};", "void f() { CALL() }"] {
    assert!(SgLang::from_path("base.cc").unwrap().grep(invalid).root().has_error());
  }
}

#[test]
fn cpp_continued_macro_comments_keep_following_definitions() {
  let lf = "#define BODY(x) body(x); /* note */ \\\n  finish(x);\n#define VALUE 1 /* note */ + 2\n#define FAKE void synthetic() { /* note */ return; }\nvoid run() { BODY(1); after(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert!(product.items.iter().any(|item| item.entry.name == "run"));
    assert!(!product.items.iter().any(|item| item.entry.name == "synthetic"));
    let call = product.refs.iter().find(|r| r.name == "after" && r.kind == 0).unwrap();
    assert_eq!(&source[call.start as usize..call.end as usize], "after()");
    assert!(!product.refs.iter().any(|r| r.kind == 0 && ["body", "finish"].contains(&r.name.as_str())));
  }
}

#[test]
fn cpp_conditional_linkage_retains_declarations_and_definition_spans() {
  let source = r#"
#ifndef HEADER_H
#define HEADER_H
#ifndef FORCE_CPP
#ifdef __cplusplus
extern "C" {
#endif
#endif
struct Info { int field; };
Info* acquire();
int inside() { return target(); }
#ifndef FORCE_CPP
#ifdef __cplusplus
}
#endif
#endif
int following() { return after(); }
#endif
"#;
  let product = clean_product(source);
  for name in ["Info", "inside", "following"] {
    assert!(product.items.iter().any(|item| item.entry.name == name), "missing {name}");
  }
  for name in ["target", "after"] {
    let call = product.refs.iter().find(|r| r.name == name && r.kind == 0).unwrap();
    assert_eq!(&source[call.start as usize..call.end as usize], format!("{name}()"));
  }
  for invalid in ["extern \"C\" { int incomplete;", "#ifdef __cplusplus\nextern \"C\" {\n#endif\nint incomplete;", "}\n#endif"] {
    assert!(SgLang::from_path("sdk.cc").unwrap().grep(invalid).root().has_error());
  }
}

#[test]
fn cpp_sdk_pointer_return_conventions_preserve_names() {
  let source = r#"
struct Info {};
extern "C" Info* WINAPI acquire() noexcept;
Info* WINAPI create() { return target(); }
char* __stdcall load(int);
char* __stdcall read() { return bytes(); }
int WINAPI(int value) { return value; }
int ordinary() { return WINAPI(1); }
"#;
  let product = clean_product(source);
  for name in ["create", "read", "ordinary"] {
    assert!(product.items.iter().any(|item| item.entry.name == name));
  }
  for name in ["target", "bytes", "WINAPI"] {
    assert!(product.refs.iter().any(|r| r.name == name && r.kind == 0));
  }
}

#[test]
fn cpp_conditional_if_prefix_preserves_common_body_and_calls() {
  let source = r#"
template<unsigned size, typename STR, typename... ARGS>
unsigned long* encode(unsigned long (&buffer)[size], unsigned long color, STR format, ARGS... args) {
  unsigned long* destination = buffer;
  write(destination, color, format, args...);
  return destination;
}
void run() {
#if ENABLE_FAST && defined(AVAILABLE)
  if (ready()) { fast(); }
  else
#endif
  { slow(); }
#ifdef USE_GPU
  if (gpu()) { upload(); }
  else
#endif
  { fallback(); }
  after();
}
"#;
  let product = clean_product(source);
  for name in ["ready", "fast", "slow", "gpu", "upload", "fallback", "after"] {
    let call = product.refs.iter().find(|r| r.name == name && r.kind == 0).unwrap();
    assert_eq!(&source[call.start as usize..call.end as usize], format!("{name}()"));
  }
  for invalid in ["void run() { if (ready()) { fast(); } else }", "void run() { else { slow(); } }"] {
    assert!(SgLang::from_path("sdk.cc").unwrap().grep(invalid).root().has_error());
  }
}

#[test]
fn cpp_sdk_parameter_annotations_are_metadata() {
  let source = r#"
void target();
void copy(_Out_writes_to_ptr_(limit) unsigned long*& destination,
          _In_ unsigned long limit, _In_reads_bytes_(size) const void* data,
          _When_(size > 0, _Out_writes_(size)) char* buffer = nullptr) {
  target();
}
int _In_(int value) { return value; }
int run() { return _In_(1); }
"#;
  let product = clean_product(source);
  for name in ["target", "_In_"] {
    let call = product.refs.iter().find(|r| r.name == name && r.kind == 0).unwrap();
    assert_eq!(&source[call.start as usize..call.end as usize], format!("{name}({})", if name == "_In_" { "1" } else { "" }));
  }
  assert!(!product.refs.iter().any(|r| r.kind == 0 && r.name == "_Out_writes_"));
  assert!(SgLang::from_path("sdk.cc").unwrap().grep("void run() { target() }").root().has_error());
}

#[test]
fn cpp_abstract_member_function_types_preserve_global_delete() {
  let source = r#"
template <typename F> struct Traits;
template <typename C, typename R, typename... Args>
struct Traits<R (C::*)(Args...) const> {};
template <typename C, typename R, typename... Args>
struct Traits<R (C::*)(Args...) const noexcept> {};
struct Object {};
using Callback = int (Object::*)(int) const;
void release(int* values) { ::delete[] values; after(); }
"#;
  let product = clean_product(source);
  let after = product
    .refs
    .iter()
    .find(|r| r.name == "after" && r.kind == 0)
    .unwrap();
  assert_eq!(&source[after.start as usize..after.end as usize], "after()");
  assert!(
    SgLang::from_path("macros.cc")
      .unwrap()
      .grep("using Broken = int (Object::*)(int) const ???;")
      .root()
      .has_error()
  );
}

#[test]
fn cpp_member_pointer_fields_parse() {
  let product = clean_product(r#"
struct Material { int field; int method(int) const; };
struct Slots { int Material::* ref; int (Material::*callback)(int) const; };
void invoke(Material& material, int (Material::*callback)(int) const) {
  (material.*callback)(1);
}
"#);
  let slots = product.items.iter().find(|item| item.entry.name == "Slots").unwrap();
  for name in ["ref", "callback"] {
    assert!(slots.members.iter().any(|member| member.entry.name == name), "missing {name}");
  }
}

#[test]
fn cpp_unnamed_pointer_defaults_preserve_global_delete() {
  clean_product(r#"
struct Diagnostic {};
void inspect(Diagnostic* = nullptr, const char* const = nullptr);
void run() {
  auto values = new int[5];
  ::delete[] values;
}
"#);
}

#[test]
fn cpp_initializer_conditionals_preserve_branches_and_following_calls() {
  let source = r#"
void configure() {
  auto backends = {
#ifndef DISABLE_BACKEND
    preferred(),
#else
    fallback(),
#endif
    always()
  };
  int nested[] = {
#if OUTER
#ifdef INNER
    1,
#elif defined(OTHER)
    2,
#endif
#elifdef ALTERNATIVE
    3,
#else
    4,
#endif
    5,
  };
  after();
}
"#;
  let product = clean_product(source);
  for name in ["preferred", "fallback", "always", "after"] {
    let reference = product.refs.iter().find(|r| r.name == name && r.kind == 0).unwrap();
    assert_eq!(&source[reference.start as usize..reference.end as usize], format!("{name}()"));
  }
  let invalid = "void run() { int values[] = { 1 2 }; }";
  assert!(SgLang::from_path("macros.cc").unwrap().grep(invalid).root().has_error());
}

fn clean_product(source: &str) -> vorpal_ingest::FileProduct {
  assert!(!SgLang::from_path("macros.cc").unwrap().grep(source).root().has_error());
  let product = OutlineExtractor::new().unwrap().extract_product("macros.cc", source).unwrap();
  assert_eq!(product.error_nodes, 0, "errors at {:?}: {source}", product.error_spans);
  assert_eq!(product.error_bytes, 0);
  for reference in &product.refs {
    assert!(source.is_char_boundary(reference.start as usize));
    assert!(source.is_char_boundary(reference.end as usize));
    assert!((reference.end as usize) <= source.len());
  }
  product
}

#[test]
fn cpp_sdk_convention_macros_preserve_declaration_names_and_ordinary_identifiers() {
  let product = clean_product(r#"
typedef int (WINAPI *Callback)(int);
typedef float (F_CALL *Rolloff)(void*, float);
int F_API apiDeclaration(void* context);
int F_API exported(int value) { return target(value); }
struct System { int F_API initialize(int); };
int WINAPI(int value) { return value; }
int ordinary() { return WINAPI(1); }
HRESULT result;
"#);
  assert!(product.items.iter().any(|i| i.entry.name == "exported"));
  assert!(product.items.iter().any(|i| i.entry.name == "WINAPI"));
  assert!(product.refs.iter().any(|r| r.name == "target" && r.kind == 0));
  assert!(product.refs.iter().any(|r| r.name == "WINAPI" && r.kind == 0));
  assert!(!product.items.iter().any(|i| i.entry.name == "F_API"));
}

#[test]
fn cpp_sdk_support_retains_real_syntax_errors() {
  for source in ["template <typename T>", "int F_API broken(int value) {", "}"] {
    let parsed = SgLang::from_path("sdk.cc").unwrap().grep(source);
    assert!(parsed.root().has_error(), "silenced invalid source: {source}");
  }
}

#[test]
fn cpp_type_macro_arguments_do_not_swallow_lambdas_or_following_functions() {
  let source = r#"
namespace App {
void registerCallbacks() {
  invoke([](State* s) -> int {
    GET_USERDATA(const InputManager, input, 1, "key");
    GET_VALUE(const double, value, 2);
    return after(input, value);
  }, 0);
}
int following() { return target(); }
}
"#;
  let product = clean_product(source);
  for name in ["registerCallbacks", "following"] {
    assert!(product.items.iter().any(|item| item.entry.name == name), "missing {name}");
  }
  for name in ["after", "target"] {
    let reference = product.refs.iter().find(|r| r.name == name && r.kind == 0).unwrap();
    let expected = if name == "after" { "after(input, value)" } else { "target()" };
    assert_eq!(&source[reference.start as usize..reference.end as usize], expected);
  }
}

#[test]
fn cpp_format_macros_and_header_probes_parse_without_expansion() {
  clean_product(r#"
#if __has_include(<SDL2/SDL.h>)
#include <SDL2/SDL.h>
#elif __has_include("SDL.h")
#include "SDL.h"
#endif
void log(unsigned long value) {
  printf("entity %" PRIu64, value);
  printf(PREFIX "entity %" PRIu64 " count %" PRId32, value);
  printf(R"(entity %)" PRIu64, value);
}
"#);
}

#[test]
fn cpp_ordinary_calls_casts_and_declarations_keep_their_reading() {
  let source = "int target(int); int run(int value) { int local(value); return target(int(value)); }";
  let product = clean_product(source);
  assert!(product.refs.iter().any(|r| r.name == "target" && r.kind == 0));
  assert!(!product.refs.iter().any(|r| r.name == "local" && r.kind == 0));
}

#[test]
fn cpp_typeid_pointer_members_and_qualified_new_arrays_parse() {
  clean_product(r#"
struct Value { int member; };
bool inspect(Value* value, int Value::*member, int count) {
  auto strings = new const char*[count];
  value->*member = 1;
  return typeid(int) == typeid(value) && typeid(const Value*) == typeid(void);
}
"#);
}

#[test]
fn cpp_pointer_to_member_calls_do_not_invent_named_callees() {
  let product = clean_product(r#"
struct Value { void direct(); };
void run(Value* pointer, Value& value, void (Value::*method)()) {
  pointer->direct();
  (pointer->*method)();
  (value.*method)();
}
"#);
  assert!(product.refs.iter().any(|r| r.name == "direct" && r.kind == 0));
  assert!(!product.refs.iter().any(|r| r.name == "method" && r.kind == 0));
}
#[test]
fn missing_tokens_are_reported_without_inventing_damaged_bytes() {
  let path = "missing.cc";
  let source = "int damaged() { return 1 }\nvoid after() {}\n";
  let raw = SgLang::from_path(path).unwrap().grep(source);
  assert!(raw.root().has_error());
  let missing: Vec<_> = raw.root().dfs().filter(|n| n.is_missing()).map(|n| n.range()).collect();
  assert!(!missing.is_empty());
  assert!(!raw.root().dfs().any(|n| n.is_error()));
  let extractor = OutlineExtractor::new().unwrap();
  let product = extractor.extract_product(path, source).unwrap();
  assert_eq!(product.error_nodes as usize, missing.len());
  assert_eq!(product.error_bytes, 0);
  for span in &product.error_spans {
    assert_eq!(span.0, span.1);
    assert!(missing.iter().any(|range| range.start == span.0 as usize));
  }
  let mut owned = Vec::new();
  vorpal_ingest::encode_product_into(&product, &mut owned);
  let mut streamed = Vec::new();
  extractor.extract_product_encoded(path, source, 0, 0, &mut streamed).unwrap();
  assert_eq!(owned, streamed);
  let handoff = extractor.extract_product_from_root(path, &raw).unwrap();
  assert_eq!(handoff.error_nodes, product.error_nodes);
  assert_eq!(handoff.error_spans, product.error_spans);
  // Prior products must not replay their incorrect clean telemetry.
  owned[4..8].copy_from_slice(&22u32.to_le_bytes());
  assert!(vorpal_ingest::decode_product_view(&owned).is_err());
}

#[test]
fn cpp_braced_parameter_defaults_preserve_types_calls_and_missing_semicolons() {
  let source = r#"
struct Options { int value; };
Options make();
int value();
void configure(const Options& options = {}, Options other = {value()});
void named(Options options = Options{});
void unnamed(const Options& = {});
template<typename T> void generic(T argument = {});
void after() { configure(); }
"#;
  for source in [source.to_owned(), source.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["value", "configure"] {
      let call = product.refs.iter().find(|r| r.name == name && r.kind == 0).unwrap();
      assert_eq!(&source[call.start as usize..call.end as usize], format!("{name}()"));
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && r.name == "Options"));
    let parsed = SgLang::from_path("defaults.cc").unwrap().grep(&source);
    assert!(parsed.root().dfs().any(|node| node.kind() == "initializer_list" && node.text() == "{}"));
  }
  for source in [
    "void configure(int x = {1 2});",
    "void configure(int x = {1);",
    "void after() { configure() }",
  ] {
    assert!(SgLang::from_path("defaults.cc").unwrap().grep(source).root().has_error(), "{source}");
  }
}

#[test]
fn cpp_pointer_return_array_references_keep_parameters_and_runtime_calls() {
  let lf = r#"
using Word = unsigned long long;
#define _In_reads_(n)
template<unsigned size> Word* encode(Word (&buffer)[size], Word value) {
  buffer[0] = value;
  return buffer;
}
template<unsigned size> const Word* inspect(const Word (&buffer)[size]) {
  return buffer;
}
Word* annotated(_In_reads_(limit()) Word* buffer) { return buffer; }
void following() {
  Word buffer[4]{};
  encode(buffer, 1);
  inspect(buffer);
  annotated(buffer);
  auto owned = new Word[2];
  ::delete[] owned;
}
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["encode", "inspect", "annotated", "following"] {
      assert!(product.items.iter().any(|item| item.entry.name == name), "missing {name}");
    }
    for (name, spelling) in [("encode", "encode(buffer, 1)"), ("inspect", "inspect(buffer)"), ("annotated", "annotated(buffer)")] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 1, "{name}");
      assert_eq!(&source[calls[0].start as usize..calls[0].end as usize], spelling);
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && ["_In_reads_", "limit", "Word", "buffer"].contains(&r.name.as_str())));
    let parsed = SgLang::from_path("pointer.cc").unwrap().grep(&source);
    let arrays: Vec<_> = parsed.root().dfs().filter(|n| n.kind() == "array_declarator" && n.text() == "(&buffer)[size]").collect();
    assert_eq!(arrays.len(), 2);
    for array in arrays {
      assert_eq!(&source[array.range()], "(&buffer)[size]");
      assert!(array.dfs().any(|n| n.kind() == "reference_declarator" && n.text() == "&buffer"));
    }
  }
  for bad in ["Word* broken(Word (&buffer)[);", "void broken() { encode(buffer, 1) }"] {
    assert!(SgLang::from_path("pointer.cc").unwrap().grep(bad).root().has_error(), "{bad}");
  }
}

#[test]
fn cpp_typed_function_contexts_keep_array_references_after_pointer_parameters() {
  let lf = r#"
using Handle = unsigned long long;
void observe();
void gather(void* device, Handle (&cascades)[4]) {
  for (Handle& handle : cascades) handle = 0;
  observe();
}

void ordinary(void* device, Handle (&cascades)[4]);
void variadic(Handle (&values)[4], ...);
void following() {
  Handle cascades[4]{};
  gather(nullptr, cascades);
  int value(1);
}
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["gather", "following"] {
      assert!(product.items.iter().any(|item| item.entry.name == name), "missing {name}");
    }
    for (name, spelling) in [("observe", "observe()"), ("gather", "gather(nullptr, cascades)")] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 1, "{name}");
      assert_eq!(&source[calls[0].start as usize..calls[0].end as usize], spelling);
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && ["Handle", "cascades", "value", "ordinary"].contains(&r.name.as_str())));
    let parsed = SgLang::from_path("array.cc").unwrap().grep(&source);
    assert_eq!(parsed.root().dfs().filter(|n| n.kind() == "array_declarator" && n.text() == "(&cascades)[4]").count(), 2);
    assert!(parsed.root().dfs().any(|n| n.kind() == "function_declarator" && n.text().starts_with("ordinary(")));
    assert!(parsed.root().dfs().any(|n| n.kind() == "function_declarator" && n.text().starts_with("variadic(")));
    assert!(parsed.root().dfs().any(|n| n.kind() == "init_declarator" && n.text() == "value(1)"));
  }
  for bad in ["void broken(void* device, Handle (&cascades)[);", "void broken() { gather(nullptr, cascades) }", "void broken(..., Handle (&cascades)[4]);"] {
    assert!(SgLang::from_path("array.cc").unwrap().grep(bad).root().has_error(), "{bad}");
  }
}

#[test]
fn cpp_native_dll_declarations_keep_metadata_and_runtime_call_spans() {
  let lf = r#"
#define API extern
#define _In_(n)
API __declspec(dllimport) int __stdcall convert(const char* input, int size);
__declspec(dllimport) int __cdecl plain(int value);
API __declspec(dllimport) int __stdcall annotated(_In_(limit()) int value);
void following() {
  convert("value", 5);
  plain(1);
  annotated(2);
}
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert!(product.items.iter().any(|item| item.entry.name == "following"));
    for (name, spelling) in [("convert", "convert(\"value\", 5)"), ("plain", "plain(1)"), ("annotated", "annotated(2)")] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 1, "{name}");
      assert_eq!(&source[calls[0].start as usize..calls[0].end as usize], spelling);
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && ["API", "_In_", "limit", "__declspec", "dllimport", "__stdcall", "__cdecl"].contains(&r.name.as_str())));
    let parsed = SgLang::from_path("dll.cc").unwrap().grep(&source);
    assert_eq!(parsed.root().dfs().filter(|n| n.kind() == "ms_declspec_modifier").count(), 3);
    assert_eq!(parsed.root().dfs().filter(|n| n.kind() == "ms_call_modifier").count(), 3);
  }
  for bad in [
    "API __declspec(dllimport) int __stdcall convert(int value)",
    "API __declspec(dllimport) int __stdcall convert(int value;",
    "void following() { convert(1) }",
  ] {
    assert!(SgLang::from_path("dll.cc").unwrap().grep(bad).root().has_error(), "{bad}");
  }
}

#[test]
fn cpp_guarded_storage_modifiers_keep_conditions_and_variable_spans() {
  let lf = r#"
#define local_storage thread_local
#define LOCAL_ENABLED() 1
const char* format();
static
#if LOCAL_ENABLED()
local_storage
#endif
const char *reason;
static
#ifdef USE_NATIVE
__declspec(thread)
#endif
int count;
void following() { reason = format(); count = 1; }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert!(product.items.iter().any(|item| item.entry.name == "following"));
    let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == "format").collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(&source[calls[0].start as usize..calls[0].end as usize], "format()");
    assert!(!product.refs.iter().any(|r| r.kind == 0 && ["LOCAL_ENABLED", "local_storage", "__declspec", "thread"].contains(&r.name.as_str())));
    let parsed = SgLang::from_path("storage.cc").unwrap().grep(&source);
    let guards: Vec<_> = parsed.root().dfs().filter(|n| n.kind() == "conditional_storage_modifier").collect();
    assert_eq!(guards.len(), 2);
    for guard in guards {
      assert!(guard.text().contains("#endif"));
      assert_eq!(&source[guard.range()], guard.text());
      assert!(guard.field("preproc_condition").is_some());
    }
    let variable = parsed.root().dfs().find(|n| n.kind() == "declaration" && n.text().contains("*reason;")).unwrap();
    assert!(variable.text().starts_with("static"));
    assert_eq!(variable.field("type").unwrap().text(), "char");
    assert_eq!(variable.field("declarator").unwrap().text(), "*reason");
  }
  for bad in [
    "static\n#ifdef LOCAL\nthread_local\n#endif\nint value",
    "static\n#ifdef LOCAL\nthread_local\nint value;",
    "static\n#ifdef LOCAL\nordinary()\n#endif\nint value;",
    "void following() { format() }",
  ] {
    assert!(SgLang::from_path("storage.cc").unwrap().grep(bad).root().has_error(), "{bad}");
  }
}
#[test]
fn cpp_preprocessor_conditions_keep_runtime_bodies_and_same_named_calls() {
  let lf = r#"
#if FEATURE_AVAILABLE(probe())
void first() { inside(); }
#elif OTHER_AVAILABLE(other())
void second() { inside(); }
#endif
void runtime() {
#if FEATURE_AVAILABLE(probe())
  if (ready()) { fast(); } else
#endif
  { slow(); }
  FEATURE_AVAILABLE(probe());
}
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["FEATURE_AVAILABLE", "probe", "ready", "fast", "slow"] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 1, "{name}: {calls:?}");
      let expected = if name == "FEATURE_AVAILABLE" { "FEATURE_AVAILABLE(probe())".to_owned() } else { format!("{name}()") };
      assert_eq!(&source[calls[0].start as usize..calls[0].end as usize], expected);
    }
    assert_eq!(product.refs.iter().filter(|r| r.kind == 0 && r.name == "inside").count(), 2);
    assert!(!product.refs.iter().any(|r| r.kind == 0 && ["OTHER_AVAILABLE", "other"].contains(&r.name.as_str())));
  }
}
#[test]
fn cpp_abstract_member_data_types_preserve_named_scopes_and_global_delete() {
  let lf = r#"
void observe();
template <typename T> struct StringMaker {};
struct Owner { int value; };
template <typename R, typename C> struct StringMaker<R C::*> {
  static bool convert(R C::* p) { observe(); return p != nullptr; }
};
using Member = int Owner::*;
using Qualified = int Owner::* const;
void following(int* values) {
  observe();
  ::delete[] values;
  ::delete[] ::new int[4];
}
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert!(product.items.iter().any(|item| item.entry.name == "following"));
    let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == "observe").collect();
    assert_eq!(calls.len(), 2);
    for call in calls {
      assert_eq!(&source[call.start as usize..call.end as usize], "observe()");
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && ["StringMaker", "convert", "Owner", "C", "R"].contains(&r.name.as_str())));
    let parsed = SgLang::from_path("member.cc").unwrap().grep(&source);
    for (spelling, scope) in [("C::*", "C"), ("Owner::*", "Owner"), ("Owner::* const", "Owner")] {
      let pointer = parsed.root().dfs().find(|n| n.kind() == "abstract_pointer_declarator" && n.text() == spelling).unwrap();
      assert_eq!(&source[pointer.range()], spelling);
      assert_eq!(pointer.field("scope").unwrap().text(), scope);
    }
    assert_eq!(parsed.root().dfs().filter(|n| n.kind() == "delete_expression").count(), 2);
  }
  for bad in ["using Broken = int ::*;", "using Broken = int Owner::;", "using Broken = int Owner::*", "void run() { observe() }"] {
    assert!(SgLang::from_path("member.cc").unwrap().grep(bad).root().has_error(), "{bad}");
  }
}

#[test]
fn cpp_decltype_comma_operands_remain_unevaluated_and_keep_original_spans() {
  let lf = r#"
struct Stream {};
Stream& stream();
int left();
bool right();
Stream& operator<<(Stream&, int);
template <typename T> T&& obtain();
struct TrueType {};
template <typename U> struct Probe {
  static auto test(int) -> decltype(stream() << obtain<U>(), TrueType());
};
using Result = decltype(left(), right());
void runtime() { left(); right(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert!(product.items.iter().any(|item| item.entry.name == "runtime"));
    for name in ["left", "right"] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 1, "{name}: {calls:?}");
      assert_eq!(&source[calls[0].start as usize..calls[0].end as usize], format!("{name}()"));
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && ["stream", "obtain", "TrueType", "test"].contains(&r.name.as_str())));
    let parsed = SgLang::from_path("decltype.cc").unwrap().grep(&source);
    let operands: Vec<_> = parsed.root().dfs().filter(|n| n.is_named() && n.kind() == "decltype").collect();
    assert_eq!(operands.len(), 2);
    for operand in operands {
      assert_eq!(&source[operand.range()], operand.text());
      assert!(operand.children().any(|n| n.kind() == "comma_expression"));
    }
  }
  for bad in ["using Broken = decltype(left(),);", "using Broken = decltype(,right());", "using Broken = decltype(left(), right()); void run() { left() }"] {
    assert!(SgLang::from_path("decltype.cc").unwrap().grep(bad).root().has_error(), "{bad}");
  }
}

#[test]
fn cpp_native_cpuid_assembly_keeps_operand_lines_and_ordinary_call_spans() {
  let lf = r#"
void observe();
void mov();
void cpuid();
int query() {
  int res;
  __asm {
    mov eax,1
    cpuid
    mov res,edx
  }
  observe();
  return res;
}
void ordinary() { mov(); cpuid(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["query", "ordinary"] {
      assert!(product.items.iter().any(|item| item.entry.name == name));
    }
    for name in ["observe", "mov", "cpuid"] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 1, "{name}: {calls:?}");
      assert_eq!(&source[calls[0].start as usize..calls[0].end as usize], format!("{name}()"));
    }
    let parsed = SgLang::from_path("assembly.cc").unwrap().grep(&source);
    let block = parsed.root().dfs().find(|n| n.kind() == "ms_asm_statement").unwrap();
    assert_eq!(&source[block.range()], block.text());
    let instructions: Vec<_> = block.children().filter(|n| n.kind() == "ms_asm_instruction").collect();
    assert_eq!(instructions.len(), 3);
    assert_eq!(instructions[0].field("destination").unwrap().text(), "eax");
    assert_eq!(instructions[0].field("source").unwrap().text(), "1");
    assert_eq!(instructions[1].field("opcode").unwrap().text(), "cpuid");
    assert_eq!(instructions[2].field("destination").unwrap().text(), "res");
    assert_eq!(instructions[2].field("source").unwrap().text(), "edx");
    for instruction in instructions { assert_eq!(&source[instruction.range()], instruction.text()); }
  }
  for bad in [
    "void f() { __asm { mov eax,\n cpuid\n } }",
    "void f() { __asm { mov\n eax,1\n } }",
    "void f() { __asm { mov eax\n ,1\n } }",
    "void f() { __asm { mov eax,1 cpuid } }",
    "void f() { __asm { cpuid() } }",
    "void f() { __asm { mov 1,2\n } }",
    "__asm { cpuid }",
    "void f() { __asm { cpuid\n } observe() }",
    "void f() { __asm { mov eax,1\n }",
  ] {
    assert!(SgLang::from_path("assembly.cc").unwrap().grep(bad).root().has_error(), "{bad}");
  }
}

#[test]
fn cpp_split_conditional_if_preserves_guards_runtime_calls_and_errors() {
  let lf = r#"int platform(); int alternate(); int test(); void consume(int); void cleanup(int); void missing();
void run() {
#if METADATA_ONLY(PLATFORM)
  int value = platform();
  if (test()) {
#else
  int value = alternate();
  if (test()) {
#endif
    consume(value);
#if METADATA_ONLY(PLATFORM)
    cleanup(value);
  } else { missing(); }
#else
  } else { missing(); }
#endif
}
void following() { missing(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert!(product.items.iter().any(|item| item.entry.name == "following"));
    assert!(!product.refs.iter().any(|r| r.kind == 0 && r.name == "METADATA_ONLY"));
    for (name, count) in [("platform", 1), ("alternate", 1), ("test", 2), ("consume", 1), ("cleanup", 1), ("missing", 3)] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), count, "{name}: {calls:?}");
      for call in calls {
        assert!(source[call.start as usize..call.end as usize].starts_with(&format!("{name}(")));
      }
    }
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new();
    vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let raw = SgLang::from_path("macros.cc").unwrap().grep(&source);
    let handed = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut scan = Vec::new();
    vorpal_ingest::encode_product_into(&handed, &mut scan);
    assert_eq!(owned, scan);
    let parsed = SgLang::from_path("split.cc").unwrap().grep(&source);
    let split = parsed.root().dfs().find(|n| n.kind() == "conditional_split_if_statement").unwrap();
    for field in ["open", "close"] {
      let guard = split.field(field).unwrap();
      assert_eq!(guard.field("preproc_condition").unwrap().text(), "METADATA_ONLY(PLATFORM)");
      assert!(guard.field("first_branch").is_some());
      assert!(guard.field("second_branch").is_some());
      assert_eq!(&source[guard.range()], guard.text());
    }
  }
  for bad in [
    lf.replacen("#else\n", "", 1),
    lf.replacen("#endif\n", "", 1),
    lf.replacen("#else\n  } else", "  } else", 1),
    lf.replacen("consume(value);", "consume(value)", 1),
    lf.replacen("if (test()) {", "if (test())", 1),
    lf.replacen("} else { missing(); }", "else { missing(); }", 1),
    lf.trim_end().trim_end_matches("void following() { missing(); }").replacen("#endif\n}", "}", 1),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      assert!(SgLang::from_path("split.cc").unwrap().grep(&source).root().has_error(), "{source}");
    }
  }
}

#[test]
fn cpp_explicit_objc_guards_preserve_message_arguments_and_cpp_boundaries() {
  let lf = r#"// UTF-8: ÃƒÆ’Ã‚Â¼
#ifdef __OBJC__
inline void dispose(Probe* object) { [object release]; }
#if defined(ARC)
inline id optional(Probe* object, void* sel) {
  if ([object respondsToSelector: sel]) return [object performSelector: sel];
  return nil;
}
#else
inline id forward(Probe* object) {
  return [provider() relay: value() to: [object performSelector: selector()]];
}
#endif
#else
void fallback() { ordinary(); }
#endif
void following() { after(); }
int __OBJC__();
void word() { __OBJC__(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["dispose", "optional", "forward", "fallback", "following", "word"] {
      assert!(product.items.iter().any(|item| item.entry.name == name), "{name}");
    }
    for name in ["provider", "value", "selector", "ordinary", "after", "__OBJC__"] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 1, "{name}: {calls:?}");
      assert_eq!(&source[calls[0].start as usize..calls[0].end as usize], format!("{name}()"));
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && matches!(r.name.as_str(), "release" | "respondsToSelector" | "performSelector" | "relay" | "to")));
    let raw = SgLang::from_path("guarded.cc").unwrap().grep(&source);
    let messages: Vec<_> = raw.root().dfs().filter(|n| n.kind() == "objc_message_expression").collect();
    assert_eq!(messages.len(), 5);
    for message in messages {
      assert_eq!(&source[message.range()], message.text());
      assert!(message.field("receiver").is_some());
      assert!(message.field("selector").is_some());
    }
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new();
    vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let scan = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut handed = Vec::new();
    vorpal_ingest::encode_product_into(&scan, &mut handed);
    assert_eq!(owned, handed);
  }
  for bad in [
    "void run(Probe* o) { [o release]; }",
    "#ifdef PLATFORM\nvoid run(Probe* o) { [o release]; }\n#endif\n",
    "#ifndef __OBJC__\nvoid run(Probe* o) { [o release]; }\n#endif\n",
    "#ifdef __OBJC__\nvoid ordinary() {}\n#else\nvoid run(Probe* o) { [o release]; }\n#endif\n",
    "#ifdef __OBJC__ extra\nvoid run(Probe* o) { [o release]; }\n#endif\n",
    "#ifdef __OBJC__\nvoid run(Probe* o) { [o release] }\n#endif\n",
    "#ifdef __OBJC__\nvoid run(Probe* o) { [o]; }\n#endif\n",
    "#ifdef __OBJC__\nvoid run(Probe* o) { [o release]; }\n",
    "#ifdef __OBJC__\nvoid run(Probe* o) { [o release]; }\n#endif\nvoid outside(Probe* o) { [o release]; }",
  ] {
    for source in [bad.to_owned(), bad.replace('\n', "\r\n")] {
      assert!(SgLang::from_path("guarded.cc").unwrap().grep(&source).root().has_error(), "{source}");
    }
  }
}

#[test]
fn cpp_guarded_objc_exceptions_preserve_cpp_call_arguments() {
  let lf = r#"#ifdef __OBJC__
id run() {
  @try { before(); }
  @catch (Probe* exception) {
    return finish(nested([receiver() relay: payload()]), [exception description]);
  }
}
#endif
void following() { after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["before", "finish", "nested", "receiver", "payload", "after"] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 1, "{name}: {calls:?}");
      let text = &source[calls[0].start as usize..calls[0].end as usize];
      assert!(text.starts_with(&format!("{name}(")) && text.ends_with(')'), "{name}: {text}");
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && matches!(r.name.as_str(), "relay" | "description")));
    for name in ["run", "following"] { assert!(product.items.iter().any(|i| i.entry.name == name)); }
    let raw = SgLang::from_path("guarded.cc").unwrap().grep(&source);
    for kind in ["try_statement", "catch_clause"] {
      let nodes: Vec<_> = raw.root().dfs().filter(|n| n.kind() == kind).collect();
      assert_eq!(nodes.len(), 1);
      assert_eq!(&source[nodes[0].range()], nodes[0].text());
      assert!(nodes[0].text().starts_with('@'));
    }
    assert_eq!(raw.root().dfs().filter(|n| n.kind() == "objc_message_expression").count(), 2);
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new(); vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let scan = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut handed = Vec::new(); vorpal_ingest::encode_product_into(&scan, &mut handed);
    assert_eq!(owned, handed);
  }
  for bad in [
    lf.replace("__OBJC__", "PLATFORM"),
    lf.replace("#ifdef __OBJC__\n", "").replace("#endif\n", ""),
    lf.replace("#ifdef __OBJC__", "#ifdef __OBJC__\nvoid ordinary() {}\n#else"),
    lf.replace("@catch (Probe* exception)", ""),
    lf.replace("@catch (Probe* exception)", "@catch ()"),
    lf.replace("@catch (Probe* exception)", "@catch (/* empty */)"),
    lf.replace("@catch (Probe* exception) {", "@catch (Probe* exception)"),
    lf.replace("payload()", "payload("),
    lf.replace("description]);", "description])"),
    lf.replace("#endif\n", ""),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      assert!(SgLang::from_path("guarded.cc").unwrap().grep(&source).root().has_error(), "{source}");
    }
  }
}

#[test]
fn cpp_guarded_objc_class_templates_and_binary_operands_preserve_spans() {
  let lf = r#"#ifdef __OBJC__
namespace Sample {
template<class T> struct Wrapper {
  Wrapper() { [receiver() release]; }
  ~Wrapper() { [receiver() release]; }
  id run() { return forward([receiver() description]); }
  int count() { return [receiver() count] + value(); }
  template<class U> id again(U) { return [receiver() relay: payload()]; }
};
}
#endif
void following() { after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["Wrapper", "following"] {
      assert!(product.items.iter().any(|item| item.entry.name == name), "{name}: {:?}", product.items);
    }
    let wrapper = product.items.iter().find(|item| item.entry.name == "Wrapper").unwrap();
    for name in ["Wrapper", "~Wrapper", "run", "count", "again"] {
      let member = wrapper.members.iter().find(|member| member.entry.name == name).unwrap();
      assert!(source[member.entry.range.byte_offset.clone()].contains(name));
      let expected = if matches!(name, "Wrapper" | "~Wrapper") {
        vorpal_outline::model::SymbolType::Constructor
      } else {
        vorpal_outline::model::SymbolType::Method
      };
      assert_eq!(member.entry.symbol_type, expected, "{name}");
      assert!(member.is_public, "{name}");
    }
    for (name, count) in [("receiver", 5), ("forward", 1), ("value", 1), ("payload", 1), ("after", 1)] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), count, "{name}: {calls:?}");
      for call in calls {
        let text = &source[call.start as usize..call.end as usize];
        assert!(text.starts_with(&format!("{name}(")) && text.ends_with(')'), "{text}");
      }
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && matches!(r.name.as_str(), "release" | "description" | "count" | "relay")));
    let raw = SgLang::from_path("guarded.cc").unwrap().grep(&source);
    assert_eq!(raw.root().dfs().filter(|n| n.kind() == "objc_message_expression").count(), 5);
    for kind in ["namespace_definition", "template_declaration", "struct_specifier", "field_declaration_list", "binary_expression"] {
      let node = raw.root().dfs().find(|n| n.kind() == kind).unwrap();
      assert_eq!(&source[node.range()], node.text());
    }
    let binary = raw.root().dfs().find(|n| n.kind() == "binary_expression").unwrap();
    assert_eq!(binary.field("left").unwrap().text(), "[receiver() count]");
    assert_eq!(binary.field("right").unwrap().text(), "value()");
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new(); vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let scan = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut handed = Vec::new(); vorpal_ingest::encode_product_into(&scan, &mut handed);
    assert_eq!(owned, handed);
  }
  for bad in [
    lf.replace("__OBJC__", "PLATFORM"),
    lf.replace("#ifdef __OBJC__\n", "").replace("#endif\n", ""),
    lf.replace("#ifdef __OBJC__", "#ifdef __OBJC__\nvoid ordinary() {}\n#else"),
    lf.replace("description]);", "description])"),
    lf.replace("[receiver() count]", "[receiver()]"),
    lf.replace("payload()", "payload("),
    lf.replace("#endif\n", ""),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      assert!(SgLang::from_path("guarded.cc").unwrap().grep(&source).root().has_error(), "{source}");
    }
  }
}

#[test]
fn cpp_guarded_objc_initializers_keep_fields_calls_and_declarator_boundaries() {
  let lf = r#"#ifdef __OBJC__
namespace Sample {
template<class T> class Holder {
public:
  id stored = [receiver() description];
  id run() { id local = [receiver() relay: payload()]; return local; }
};
}

#endif
void following() { after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    let holder = product.items.iter().find(|item| item.entry.name == "Holder").unwrap();
    for (name, kind) in [("stored", vorpal_outline::model::SymbolType::Field), ("run", vorpal_outline::model::SymbolType::Method)] {
      let member = holder.members.iter().find(|member| member.entry.name == name).unwrap();
      assert_eq!(member.entry.symbol_type, kind);
      assert!(member.is_public);
      assert!(source[member.entry.range.byte_offset.clone()].contains(name));
    }
    for (name, count) in [("receiver", 2), ("payload", 1), ("after", 1)] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), count, "{name}: {calls:?}");
      for call in calls { assert_eq!(&source[call.start as usize..call.end as usize], format!("{name}()")); }
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && matches!(r.name.as_str(), "description" | "relay")));
    let raw = SgLang::from_path("guarded.cc").unwrap().grep(&source);
    assert_eq!(raw.root().dfs().filter(|n| n.kind() == "objc_message_expression").count(), 2);
    let field = raw.root().dfs().find(|n| n.kind() == "field_declaration").unwrap();
    assert_eq!(field.field("declarator").unwrap().text(), "stored");
    assert_eq!(field.field("default_value").unwrap().text(), "[receiver() description]");
    let init = raw.root().dfs().find(|n| n.kind() == "init_declarator").unwrap();
    assert_eq!(init.field("declarator").unwrap().text(), "local");
    assert_eq!(init.field("value").unwrap().text(), "[receiver() relay: payload()]");
    assert_eq!(&source[init.range()], init.text());
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new(); vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut scan = Vec::new(); vorpal_ingest::encode_product_into(&handed, &mut scan);
    assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace("__OBJC__", "PLATFORM"),
    lf.replace("#ifdef __OBJC__\n", "").replace("#endif\n", ""),
    lf.replace("#ifdef __OBJC__", "#ifdef __OBJC__\nvoid ordinary() {}\n#else"),
    lf.replace("description];", "description]"),
    lf.replace("payload()];", "payload() ]"),
    lf.replace("[receiver() description]", "[receiver()]"),
    lf.replace("#endif\n", ""),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      assert!(SgLang::from_path("guarded.cc").unwrap().grep(&source).root().has_error(), "{source}");
    }
  }
}

#[test]
fn cpp_guarded_objc_protocols_strings_and_variadic_messages_keep_structure() {
  let lf = r#"#ifdef __OBJC__
@protocol Fixture
@optional
-(void) setUp;
-(void) tearDown;
@required
+(id) create;
-(void) receive:(id)value forward:(id)other;
@end
namespace Sample {
struct Owner {
  id stored = @"text";
  id run() { consume(@ "literal\n"); return [[receiver() alloc] initWithFormat:@"%s", payload(), nested()]; }
};
}
#endif
void following() { after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    let owner = product.items.iter().find(|item| item.entry.name == "Owner").unwrap();
    for (name, kind) in [("stored", vorpal_outline::model::SymbolType::Field), ("run", vorpal_outline::model::SymbolType::Method)] {
      let member = owner.members.iter().find(|member| member.entry.name == name).unwrap();
      assert_eq!(member.entry.symbol_type, kind);
      assert!(member.is_public);
      assert!(source[member.entry.range.byte_offset.clone()].contains(name));
    }
    for name in ["receiver", "payload", "nested", "consume", "after"] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 1, "{name}: {calls:?}");
      let call = calls[0];
      let expected = if name == "consume" { "consume(@ \"literal\\n\")".to_owned() } else { format!("{name}()") };
      assert_eq!(&source[call.start as usize..call.end as usize], expected);
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && matches!(r.name.as_str(), "setUp" | "tearDown" | "create" | "receive" | "forward" | "alloc" | "initWithFormat")));
    assert!(!product.items.iter().any(|item| item.entry.name == "Fixture" || item.entry.name == "end"));
    let raw = SgLang::from_path("guarded.cc").unwrap().grep(&source);
    let protocol = raw.root().dfs().find(|n| n.kind() == "objc_protocol_declaration").unwrap();
    assert_eq!(protocol.field("name").unwrap().text(), "Fixture");
    assert_eq!(&source[protocol.range()], protocol.text());
    let methods: Vec<_> = protocol.dfs().filter(|n| n.kind() == "objc_method_declaration").collect();
    assert_eq!(methods.len(), 4);
    assert_eq!(methods[0].field("return_type").unwrap().text(), "void");
    assert_eq!(methods[3].field("parameter_type").unwrap().text(), "id");
    assert_eq!(methods[3].field("parameter").unwrap().text(), "value");
    let literals: Vec<_> = raw.root().dfs().filter(|n| n.kind() == "objc_string_literal").collect();
    assert_eq!(literals.len(), 3);
    for literal in literals {
      assert_eq!(&source[literal.range()], literal.text());
      assert!(literal.field("value").unwrap().text().starts_with('"'));
    }
    let message = raw.root().dfs().find(|n| n.kind() == "objc_message_expression" && n.text().contains("initWithFormat")).unwrap();
    let arguments: Vec<_> = message.field_children("argument").collect();
    assert_eq!(arguments.iter().map(|n| n.text().into_owned()).collect::<Vec<_>>(), ["@\"%s\"", "payload()", "nested()"]);
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new(); vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut scan = Vec::new(); vorpal_ingest::encode_product_into(&handed, &mut scan);
    assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace("__OBJC__", "PLATFORM"),
    lf.replace("#ifdef __OBJC__\n", "").replace("#endif\n", ""),
    lf.replace("#ifdef __OBJC__", "#ifdef __OBJC__\nvoid ordinary() {}\n#else"),
    lf.replace("@protocol Fixture", "@protocolFixture"),
    lf.replace("@optional", "@optionally"),
    lf.replace("@end", "@ending"),
    lf.replace("@end\n", ""),
    lf.replace("setUp;", "setUp"),
    lf.replace("-(void) setUp;", "-() setUp;"),
    lf.replace("receive:(id)value", "receive:()value"),
    lf.replace("@\"text\"", "@L\"text\""),
    lf.replace("@\"text\"", "@u8\"text\""),
    lf.replace("@\"text\"", "@R\"(text)\""),
    lf.replace("@\"text\"", "@\"text"),
    lf.replace("nested()];", "nested(),];"),
    lf.replace("initWithFormat:@\"%s\"", "description"),
    lf.replace("stored = @\"text\";", "stored = @\"text\""),
    lf.replace("#endif\n", ""),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      assert!(SgLang::from_path("guarded.cc").unwrap().grep(&source).root().has_error(), "{source}");
    }
  }
}

#[test]
fn cpp_guarded_objc_selectors_are_metadata_with_original_spans() {
  let lf = r#"void before() { selector(real()); }
#ifdef __OBJC__
void run() {
  consume(@selector(setUp));
  consume(@selector(receive:forward:));
  selector(real());
}
#endif
void following() { selector(real()); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["before", "run", "following"] {
      let item = product.items.iter().find(|item| item.entry.name == name).unwrap();
      assert!(source[item.entry.range.byte_offset.clone()].starts_with(&format!("void {name}()")));
    }
    for name in ["selector", "real"] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), 3, "{name}: {calls:?}");
      let expected = if name == "selector" { "selector(real())" } else { "real()" };
      for call in calls { assert_eq!(&source[call.start as usize..call.end as usize], expected); }
    }
    let consume: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == "consume").collect();
    assert_eq!(consume.len(), 2);
    for (call, expected) in consume.iter().zip(["consume(@selector(setUp))", "consume(@selector(receive:forward:))"]) {
      assert_eq!(&source[call.start as usize..call.end as usize], expected);
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && matches!(r.name.as_str(), "setUp" | "receive" | "forward")));
    let raw = SgLang::from_path("guarded.cc").unwrap().grep(&source);
    let selectors: Vec<_> = raw.root().dfs().filter(|n| n.kind() == "objc_selector_expression").collect();
    assert_eq!(selectors.len(), 2);
    for (node, labels) in selectors.iter().zip([vec!["setUp"], vec!["receive", "forward"]]) {
      assert_eq!(&source[node.range()], node.text());
      assert_eq!(node.field_children("selector").map(|n| n.text().into_owned()).collect::<Vec<_>>(), labels);
      assert!(!node.dfs().any(|n| n.kind() == "call_expression"));
    }
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new(); vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut scan = Vec::new(); vorpal_ingest::encode_product_into(&handed, &mut scan);
    assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace("__OBJC__", "PLATFORM"),
    lf.replace("#ifdef __OBJC__\n", "").replace("#endif\n", ""),
    lf.replace("#ifdef __OBJC__", "#ifdef __OBJC__\nvoid ordinary() {}\n#else"),
    lf.replace("@selector(", "@selectorSuffix("),
    lf.replace("@selector(setUp)", "@selector()"),
    lf.replace("@selector(setUp)", "@selector(setUp())"),
    lf.replace("@selector(receive:forward:)", "@selector(receive forward)"),
    lf.replace("@selector(receive:forward:)", "@selector(receive:, forward:)"),
    lf.replace("@selector(setUp)", "@selector(setUp, real())"),
    lf.replace("@selector(setUp)", "@selector(setUp"),
    lf.replace("consume(@selector(setUp));", "consume(@selector(setUp))"),
    lf.replace("#endif\n", ""),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      assert!(SgLang::from_path("guarded.cc").unwrap().grep(&source).root().has_error(), "{source}");
    }
  }
}

#[test]
fn cpp_guarded_objc_message_fields_keep_structure_and_call_spans() {
  let lf = r#"void before() { consume(ordinary.location); }
#ifdef __OBJC__
long run() {
  auto local = [receiver() range:payload()].location;
  consume([receiver() range].nested.location);
  consume(ordinary.location);
  return local;
}
#endif
void following() { consume(ordinary.location); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["before", "run", "following"] { assert!(product.items.iter().any(|item| item.entry.name == name)); }
    for (name, count) in [("receiver", 2), ("payload", 1)] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), count, "{name}: {calls:?}");
      for call in calls { assert_eq!(&source[call.start as usize..call.end as usize], format!("{name}()")); }
    }
    let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == "consume").collect();
    assert_eq!(calls.len(), 4);
    for (call, expected) in calls.iter().zip(["consume(ordinary.location)", "consume([receiver() range].nested.location)", "consume(ordinary.location)", "consume(ordinary.location)"]) {
      assert_eq!(&source[call.start as usize..call.end as usize], expected);
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && matches!(r.name.as_str(), "range" | "nested" | "location")));
    let raw = SgLang::from_path("guarded.cc").unwrap().grep(&source);
    let fields: Vec<_> = raw.root().dfs().filter(|n| n.kind() == "field_expression").collect();
    assert_eq!(fields.len(), 6);
    for field in &fields {
      assert_eq!(&source[field.range()], field.text());
      assert_eq!(field.field("operator").unwrap().text(), ".");
      assert!(matches!(field.field("field").unwrap().text().as_ref(), "nested" | "location"));
    }
    let rooted: Vec<_> = fields.iter().filter(|n| n.field("argument").unwrap().kind() == "objc_message_expression").collect();
    assert_eq!(rooted.len(), 2);
    assert_eq!(rooted[0].field("argument").unwrap().text(), "[receiver() range:payload()]");
    assert_eq!(rooted[0].field("field").unwrap().text(), "location");
    assert_eq!(rooted[1].field("argument").unwrap().text(), "[receiver() range]");
    assert_eq!(rooted[1].field("field").unwrap().text(), "nested");
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new(); vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new(); extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut scan = Vec::new(); vorpal_ingest::encode_product_into(&handed, &mut scan); assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace("__OBJC__", "PLATFORM"),
    lf.replace("#ifdef __OBJC__\n", "").replace("#endif\n", ""),
    lf.replace("#ifdef __OBJC__", "#ifdef __OBJC__\nvoid ordinary() {}\n#else"),
    lf.replace("].location;", "].;"),
    lf.replace("].location;", "] location;"),
    lf.replace("].location;", "].location"),
    lf.replace("range:payload()]", "range:payload()"),
    lf.replace("#endif\n", ""),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      assert!(SgLang::from_path("guarded.cc").unwrap().grep(&source).root().has_error(), "{source}");
    }
  }
}

#[test]
fn cpp_guarded_objc_constructor_initializers_preserve_member_and_argument_spans() {
  let lf = r#"#ifdef __OBJC__
struct Holder {
  id stored;
  Holder(id input) : stored([receiver(input) copy]) { after(); }
  Holder(const Holder& other) : stored([other.stored copy]) { after(); }
};
#endif
void following() { ordinary(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    let owner = product.items.iter().find(|item| item.entry.name == "Holder").unwrap();
    let constructors: Vec<_> = owner.members.iter().filter(|member| member.entry.symbol_type == vorpal_outline::model::SymbolType::Constructor).collect();
    assert_eq!(constructors.len(), 2);
    for constructor in constructors {
      assert_eq!(constructor.entry.name, "Holder"); assert!(constructor.is_public);
      assert!(source[constructor.entry.range.byte_offset.clone()].contains("stored(["));
    }
    assert!(owner.members.iter().any(|member| member.entry.name == "stored" && member.entry.symbol_type == vorpal_outline::model::SymbolType::Field));
    assert!(product.items.iter().any(|item| item.entry.name == "following"));
    for (name, count, expected) in [("receiver", 1, "receiver(input)"), ("after", 2, "after()"), ("ordinary", 1, "ordinary()")] {
      let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
      assert_eq!(calls.len(), count, "{name}: {calls:?}");
      for call in calls { assert_eq!(&source[call.start as usize..call.end as usize], expected); }
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && matches!(r.name.as_str(), "copy" | "stored")));
    let raw = SgLang::from_path("guarded.cc").unwrap().grep(&source);
    let initializers: Vec<_> = raw.root().dfs().filter(|n| n.kind() == "field_initializer").collect();
    assert_eq!(initializers.len(), 2);
    for (initializer, expected) in initializers.iter().zip(["stored([receiver(input) copy])", "stored([other.stored copy])"]) {
      assert_eq!(initializer.text(), expected); assert_eq!(&source[initializer.range()], expected);
      assert_eq!(initializer.children().find(|n| n.kind() == "field_identifier").unwrap().text(), "stored");
      let args = initializer.children().find(|n| n.kind() == "argument_list").unwrap();
      assert_eq!(&source[args.range()], args.text());
      assert_eq!(args.dfs().filter(|n| n.kind() == "objc_message_expression").count(), 1);
    }
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new(); vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new(); extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut scan = Vec::new(); vorpal_ingest::encode_product_into(&handed, &mut scan); assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace("__OBJC__", "PLATFORM"),
    lf.replace("#ifdef __OBJC__\n", "").replace("#endif\n", ""),
    lf.replace("#ifdef __OBJC__", "#ifdef __OBJC__\nvoid ordinary() {}\n#else"),
    lf.replace("copy])", "copy]"),
    lf.replace("copy])", "copy)"),
    lf.replace("stored([", "(["),
    lf.replace("};", "}"),
    lf.replace("#endif\n", ""),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      assert!(SgLang::from_path("guarded.cc").unwrap().grep(&source).root().has_error(), "{source}");
    }
  }
}

#[test]
fn cpp_guarded_objc_statements_require_a_body_context() {
  let lf = r#"#ifdef __OBJC__
@protocol Fixture
-(void) release;
@end
namespace Sample {
  void run() {
#if PLATFORM
    [receiver() release];
#endif
    consume([receiver() description]);
  }
  struct Owner { void run() { [receiver() release]; } };
}
#endif
void following() { after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert!(product.items.iter().any(|i| i.entry.name == "following"));
    let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == "receiver").collect();
    assert_eq!(calls.len(), 3);
    for call in calls { assert_eq!(&source[call.start as usize..call.end as usize], "receiver()"); }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && matches!(r.name.as_str(), "release" | "description")));
    let raw = SgLang::from_path("guarded.cc").unwrap().grep(&source);
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new(); vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new(); extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut scan = Vec::new(); vorpal_ingest::encode_product_into(&handed, &mut scan); assert_eq!(owned, scan);
  }
  for statement in ["[receiver() release];", "consume([receiver() release]);", "return [receiver() description];", "@try {} @catch (...) {}"] {
    for body in [statement.to_owned(), format!("namespace Invalid {{ {statement} }}"), format!("namespace Invalid {{\n#if PLATFORM\n{statement}\n#else\nvoid valid() {{}}\n#endif\n}}") ] {
      let bad = format!("#ifdef __OBJC__\n{body}\n#endif\nvoid following() {{ after(); }}\n");
      for source in [bad.clone(), bad.replace('\n', "\r\n")] {
        let raw = SgLang::from_path("guarded.cc").unwrap().grep(&source);
        assert!(raw.root().has_error(), "{source}");
        let extractor = OutlineExtractor::new().unwrap();
        let product = extractor.extract_product("guarded.cc", &source).unwrap();
        assert!(product.error_nodes > 0, "{source}");
        let following = product.items.iter().find(|i| i.entry.name == "following").unwrap();
        assert_eq!(&source[following.entry.range.byte_offset.clone()], "void following() { after(); }");
        let mut owned = Vec::new(); vorpal_ingest::encode_product_into(&product, &mut owned);
        let mut streamed = Vec::new(); extractor.extract_product_encoded("guarded.cc", &source, 0, 0, &mut streamed).unwrap();
        assert_eq!(owned, streamed);
        let handed = extractor.extract_product_from_root("guarded.cc", &raw).unwrap();
        let mut scan = Vec::new(); vorpal_ingest::encode_product_into(&handed, &mut scan); assert_eq!(owned, scan);
      }
    }
  }
}

#[test]
fn cpp_inverse_objc_guard_preserves_else_spans_and_body_boundaries() {
  let lf = r#"
#ifndef __OBJC__
namespace Ordinary { void run() { normal(); } }
#else
@protocol Probe
- (id)description;
@end
namespace Dialect {
  void run() {
#if FEATURE
    consume([receiver() description]);
#endif
  }
}
#endif
void nested() {
#ifndef __OBJC__
  ordinary();
#else
  consume([receiver() description]);
#endif
}
void following() { after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    let following = product.items.iter().find(|i| i.entry.name == "following").unwrap();
    assert_eq!(&source[following.entry.range.byte_offset.clone()], "void following() { after(); }");
    let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == "receiver").collect();
    assert_eq!(calls.len(), 2);
    for call in calls { assert_eq!(&source[call.start as usize..call.end as usize], "receiver()"); }
    assert!(!product.refs.iter().any(|r| r.kind == 0 && r.name == "description"));
    let raw = SgLang::from_path("inverse.cc").unwrap().grep(&source);
    let groups: Vec<_> = raw.root().dfs().filter(|n| n.kind().as_ref() == "preproc_ifdef" && n.field("name").is_some_and(|f| f.text() == "__OBJC__")).collect();
    assert_eq!(groups.len(), 2);
    for group in groups {
      let alternative = group.field("alternative").unwrap();
      assert_eq!(alternative.kind().as_ref(), "preproc_else");
      assert!(alternative.text().starts_with("#else"));
      assert!(alternative.text().contains("consume([receiver() description]);"));
    }
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new(); vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new(); extractor.extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed).unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor.extract_product_from_root("macros.cc", &raw).unwrap();
    let mut scan = Vec::new(); vorpal_ingest::encode_product_into(&handed, &mut scan); assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace("__OBJC__", "PLATFORM"),
    lf.replace("__OBJC__", "__OBJC__EXTRA"),
    lf.replace("#else", "#elif defined(__OBJC__)"),
    lf.replace("#else", "#else junk"),
    lf.replace("consume([receiver() description]);", "consume([receiver() description])"),
    lf.replace("normal();", "[receiver() description];"),
    lf.replace("namespace Dialect {", "namespace Dialect { [receiver() description];"),
    lf.replace("#endif\nvoid nested", "void nested"),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      let raw = SgLang::from_path("inverse.cc").unwrap().grep(&source);
      assert!(raw.root().has_error(), "{source}");
      assert!(OutlineExtractor::new().unwrap().extract_product("inverse.cc", &source).unwrap().error_nodes > 0, "{source}");
    }
  }
}

#[test]
fn cpp_managed_handles_stay_in_the_explicit_positive_guard() {
  let lf = r#"
#if defined(_MANAGED)
namespace Native {
template<class T> struct Box {};
template<class T> struct Box<T^> {
  static T^ convert(T^ value) { return value; }
};
template<class T> T^ retained(T^ value) { return ::Native::Box<T^>::convert(value); }
#if defined(SECONDARY)
template<class T> T^ nested(T^ value) { return value; }
#else
template<class T> T^ alternative(T^ value) { return value; }
#endif
}
#else
int ordinary(int a, int b) { return a ^ b; }
#endif
void following() { after(); }
"#;
  for guard in [
    lf.to_owned(),
    lf.replace("#if defined(_MANAGED)", "#ifdef _MANAGED"),
  ] {
    for source in [guard.clone(), guard.replace('\n', "\r\n")] {
      let product = clean_product(&source);
      for name in ["retained", "nested", "alternative", "ordinary", "following"] {
        assert!(
          product.items.iter().any(|item| item.entry.name == name),
          "missing {name}: {:?}",
          product.items
        );
      }
      assert!(
        product
          .items
          .iter()
          .flat_map(|item| &item.members)
          .any(|member| {
            member.entry.name == "convert"
              && member.is_public
              && source[member.entry.range.byte_offset.clone()].contains("T^ convert(T^ value)")
          })
      );
      let retained = product
        .items
        .iter()
        .find(|item| item.entry.name == "retained")
        .unwrap();
      let call = if source.contains("convert(touch(value))") {
        "::Native::Box<T^>::convert(touch(value))"
      } else {
        "::Native::Box<T^>::convert(value)"
      };
      assert_eq!(
        &source[retained.entry.range.byte_offset.clone()],
        format!("T^ retained(T^ value) {{ return {call}; }}")
      );
      let convert: Vec<_> = product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == "convert")
        .collect();
      assert_eq!(convert.len(), 1, "{:?}", product.refs);
      assert_eq!(
        &source[convert[0].start as usize..convert[0].end as usize],
        call
      );
      assert!(!product.refs.iter().any(|r| r.kind == 0 && r.name == "Box"));
      let touch: Vec<_> = product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == "touch")
        .collect();
      assert_eq!(
        touch.len(),
        usize::from(source.contains("convert(touch(value))"))
      );
      for call in touch {
        assert_eq!(
          &source[call.start as usize..call.end as usize],
          "touch(value)"
        );
      }

      let raw = SgLang::from_path("managed.cc").unwrap().grep(&source);
      assert!(
        raw
          .root()
          .dfs()
          .any(|n| n.kind().as_ref() == "abstract_managed_handle_declarator" && n.text() == "^")
      );
      assert!(
        raw
          .root()
          .dfs()
          .any(|n| n.kind().as_ref() == "managed_handle_declarator" && n.text() == "^ value")
      );
      assert!(
        raw
          .root()
          .dfs()
          .any(|n| n.kind().as_ref() == "binary_expression" && n.text() == "a ^ b")
      );
      let extractor = OutlineExtractor::new().unwrap();
      let mut owned = Vec::new();
      vorpal_ingest::encode_product_into(&product, &mut owned);
      let mut streamed = Vec::new();
      extractor
        .extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed)
        .unwrap();
      assert_eq!(owned, streamed);
      let handed = extractor
        .extract_product_from_root("macros.cc", &raw)
        .unwrap();
      let mut scan = Vec::new();
      vorpal_ingest::encode_product_into(&handed, &mut scan);
      assert_eq!(owned, scan);
    }
  }
  for bad in [
    lf.replace("_MANAGED", "PLATFORM"),
    lf.replace("_MANAGED", "_MANAGED_EXTRA"),
    lf.replace("#if defined(_MANAGED)", "#if defined(_MANAGED) trailing"),
    lf.replace("convert(value);", "convert(value)"),
    lf.replace("::Native::Box<T^>", "::Native::Box<^>"),
    lf.replace("\n#endif\nvoid following", "\nvoid following"),
    "#ifdef _MANAGED\nvoid good() {}\n#else\ntemplate<class T> T^ bad(T^ value) { return value; }\n#endif\n".to_owned(),
    "#ifdef PLATFORM\nvoid good() {}\n#elif defined(_MANAGED)\ntemplate<class T> T^ bad(T^ value) { return value; }\n#endif\n".to_owned(),
    "template<class T> T^ bad(T^ value) { return value; }".to_owned(),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      let raw = SgLang::from_path("managed.cc").unwrap().grep(&source);
      assert!(raw.root().has_error(), "{source}");
      assert!(OutlineExtractor::new().unwrap().extract_product("managed.cc", &source).unwrap().error_nodes > 0, "{source}");
    }
  }
}

#[test]
fn cpp_conditional_heads_share_original_body_calls() {
  let lf = r#"
#ifdef OUTER
namespace Native {
#ifndef __OBJC__
#if defined(USE_WIDE)
extern "C" int wide(int argc, wchar_t* argv[], wchar_t*[]) {
#else
int narrow(int argc, char* argv[]) {
#endif
  struct Local { int field; };
  return shared(touch(argc));
}
#else
int alternative(int argc, char** argv) { return other(argc); }
#endif
void following() { after(); }
}
#endif
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["wide", "narrow", "alternative", "following"] {
      assert!(
        product.items.iter().any(|i| i.entry.name == name),
        "missing {name}"
      );
    }
    let owners: Vec<_> = product
      .items
      .iter()
      .enumerate()
      .filter(|(_, i)| matches!(i.entry.name.as_ref(), "wide" | "narrow"))
      .map(|(index, item)| {
        assert_eq!(item.entry.ast_kind, "conditional_function_prefix");
        assert!(source[item.entry.range.byte_offset.clone()].ends_with('{'));
        assert!(!source[item.entry.range.byte_offset.clone()].contains("shared"));
        index as u32 + 1
      })
      .collect();
    for (name, text) in [("shared", "shared(touch(argc))"), ("touch", "touch(argc)")] {
      let calls: Vec<_> = product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == name)
        .collect();
      assert_eq!(calls.len(), 2, "{:?}", product.refs);
      for owner in &owners {
        let call = calls
          .iter()
          .find(|r| r.from_entity_index == *owner)
          .unwrap();
        assert_eq!(&source[call.start as usize..call.end as usize], text);
      }
    }
    for name in ["other", "after"] {
      assert_eq!(
        product
          .refs
          .iter()
          .filter(|r| r.kind == 0 && r.name == name)
          .count(),
        1
      );
    }
    assert!(
      !product
        .refs
        .iter()
        .any(|r| r.kind == 0 && r.name == "defined")
    );
    assert!(
      !product
        .signatures
        .iter()
        .any(|sketch| owners.contains(&sketch.entity_index))
    );
    assert!(!product.items.iter().any(|item| item.entry.name == "Local"));
    for name in ["wide", "narrow"] {
      assert!(
        product
          .returns
          .iter()
          .any(|(function, ty)| function == name && ty == "int")
      );
    }
    let raw = SgLang::from_path("conditional.cc").unwrap().grep(&source);
    let definition = raw
      .root()
      .dfs()
      .find(|n| n.kind().as_ref() == "conditional_function_definition")
      .unwrap();
    let body = definition.field("body").unwrap();
    assert_eq!(body.kind().as_ref(), "conditional_function_body");
    assert!(body.text().trim_start().starts_with("struct Local"));
    let group = definition.field("prefixes").unwrap();
    assert_eq!(group.kind().as_ref(), "preproc_if");
    assert_eq!(
      group.field("condition").unwrap().text(),
      "defined(USE_WIDE)"
    );
    assert!(
      group
        .field("alternative")
        .unwrap()
        .text()
        .starts_with("#else")
    );
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new();
    vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor
      .extract_product_from_root("macros.cc", &raw)
      .unwrap();
    let mut scan = Vec::new();
    vorpal_ingest::encode_product_into(&handed, &mut scan);
    assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace("return shared(touch(argc));", "return shared(touch(argc))"),
    lf.replace("#endif\n  struct Local", "  struct Local"),
    lf.replace("}\n#else\nint alternative", "#else\nint alternative"),
    lf.replace("#else\nint narrow", "}\n#else\nint narrow"),
    lf.replace("#else\nint narrow", "#elif defined(SECOND)\nint narrow"),
    lf.replace("#if defined(USE_WIDE)", "#if defined(USE_WIDE) trailing"),
    format!("void invalid() {{ {lf} }}"),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      assert!(
        SgLang::from_path("conditional.cc")
          .unwrap()
          .grep(&source)
          .root()
          .has_error(),
        "{source}"
      );
      assert!(
        OutlineExtractor::new()
          .unwrap()
          .extract_product("conditional.cc", &source)
          .unwrap()
          .error_nodes
          > 0,
        "{source}"
      );
    }
  }
}

#[test]
fn conditional_function_parameters_remain_branch_local() {
  let source = "struct Wide {}; struct Narrow {};\n#if defined(PLATFORM)\nvoid wide(Wide& value) {\n#else\nvoid narrow(Narrow& value) {\n#endif\nvalue.visit();\n}\n";
  let product = clean_product(source);
  let calls: Vec<_> = product
    .refs
    .iter()
    .filter(|r| r.kind == 0 && r.name == "visit")
    .collect();
  assert_eq!(calls.len(), 2);
  for (name, ty) in [("wide", "Wide"), ("narrow", "Narrow")] {
    let index = product
      .items
      .iter()
      .position(|i| i.entry.name == name)
      .unwrap() as u32
      + 1;
    let call = calls.iter().find(|r| r.from_entity_index == index).unwrap();
    assert_eq!(call.receiver_type.as_deref(), Some(ty));
    assert_eq!(
      &source[call.start as usize..call.end as usize],
      "value.visit()"
    );
    let (_, params) = product
      .entity_params
      .iter()
      .find(|(i, _)| *i == index)
      .unwrap();
    assert_eq!(params[0], ("value".to_owned(), Some(ty.to_owned())));
  }
}

#[test]
fn cpp_unqualified_operator_calls_preserve_names_owners_and_arguments() {
  let lf = r#"
struct Iter {
  Iter& operator++();
  bool operator==(const Iter&) const;
  const Iter& other() const;
  Iter operator++(int) { Iter old(*this); operator++(); return old; }
  bool operator!=(const Iter&) const { return !operator==(other()); }
};
void ordinary();
void following() { ordinary(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for (name, call) in [
      ("operator++", "operator++()"),
      ("operator==", "operator==(other())"),
      ("other", "other()"),
      ("ordinary", "ordinary()"),
    ] {
      let calls: Vec<_> = product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == name)
        .collect();
      assert_eq!(calls.len(), 1, "{name}: {:?}", product.refs);
      assert_eq!(
        &source[calls[0].start as usize..calls[0].end as usize],
        call
      );
      assert_ne!(calls[0].from_entity_index, 0);
    }
    assert!(product.items.iter().any(|i| i.entry.name == "following"));
    let extractor = OutlineExtractor::new().unwrap();
    let mut owned = Vec::new();
    vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let raw = SgLang::from_path("macros.cc").unwrap().grep(&source);
    let handed = extractor
      .extract_product_from_root("macros.cc", &raw)
      .unwrap();
    let mut scan = Vec::new();
    vorpal_ingest::encode_product_into(&handed, &mut scan);
    assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace("operator++(); return", "operator++() return"),
    lf.replace("operator==(other())", "operator(other())"),
    lf.replace("operator==(other())", "operator==(other(), ,)"),
  ] {
    assert!(
      SgLang::from_path("macros.cc")
        .unwrap()
        .grep(&bad)
        .root()
        .has_error(),
      "{bad}"
    );
  }
}
#[test]
fn damaged_operator_signatures_do_not_invent_runtime_calls() {
  // Unsupported partial statement macros leave this class damaged. Recovery
  // must not turn its following typed operator definition into a runtime edge.
  let lf = r#"namespace Outer { namespace Bench {
struct Benchmark {
 void run() {
  CATCH_TRY { work(); } CATCH_CATCH_ALL { fail(); }
 }
 template <typename Fun, typename std::enable_if<!Detail::is_related<Fun, Benchmark>::value, int>::type = 0>
 Benchmark& operator=(Fun func) { run(); return *this; }
};
}}
void following() { after(); }
"#;
  for parameter in ["Fun func", "Fun callback()"] {
    let variant = lf.replace("Fun func", parameter);
    for source in [variant.clone(), variant.replace('\n', "\r\n")] {
      let raw = SgLang::from_path("macros.cc").unwrap().grep(&source);
      assert!(raw.root().has_error());
      let call = raw
        .root()
        .dfs()
        .find(|n| {
          n.kind() == "call_expression"
            && n
              .field("function")
              .is_some_and(|f| f.kind() == "operator_name")
        })
        .unwrap();
      assert!(call.field("arguments").unwrap().has_error());
      if parameter.contains("callback") {
        assert!(call.dfs().any(|n| n.kind() == "call_expression"
          && n.field("function").is_some_and(|f| f.text() == "callback")));
      }
      let extractor = OutlineExtractor::new().unwrap();
      let product = extractor.extract_product("macros.cc", &source).unwrap();
      assert!(
        !product
          .refs
          .iter()
          .any(|r| r.kind == 0 && matches!(r.name.as_str(), "operator=" | "callback"))
      );
      let after: Vec<_> = product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == "after")
        .collect();
      assert_eq!(after.len(), 1);
      assert_eq!(
        &source[after[0].start as usize..after[0].end as usize],
        "after()"
      );
      assert!(product.items.iter().any(|i| i.entry.name == "following"));
      let mut owned = Vec::new();
      vorpal_ingest::encode_product_into(&product, &mut owned);
      let mut streamed = Vec::new();
      extractor
        .extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed)
        .unwrap();
      assert_eq!(owned, streamed);
      let handed = extractor
        .extract_product_from_root("macros.cc", &raw)
        .unwrap();
      let mut scan = Vec::new();
      vorpal_ingest::encode_product_into(&handed, &mut scan);
      assert_eq!(owned, scan);
    }
  }
}

#[test]
fn conditional_function_objc_alternative_preserves_body_and_metadata() {
  let lf = r#"#ifdef OUTER
#ifndef __OBJC__
#if defined(WIDE)
int wide(int v){
#else
int narrow(int v){
#endif
return next(v);
}
#else
int objc(int v){
#if !ARC
Pool *p=[[Pool alloc] init];
#endif
#if !ARC
[p drain];
#endif
return next(v);
}
#endif
#endif
void following() { after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let raw = SgLang::from_path("macros.cc").unwrap().grep(&source);
    assert!(!raw.root().has_error());
    let messages: Vec<_> = raw
      .root()
      .dfs()
      .filter(|n| n.kind() == "objc_message_expression")
      .collect();
    assert_eq!(messages.len(), 3);
    for message in messages {
      assert_eq!(&source[message.range()], message.text());
    }
    let extractor = OutlineExtractor::new().unwrap();
    let product = extractor.extract_product("macros.cc", &source).unwrap();
    let calls: Vec<_> = product
      .refs
      .iter()
      .filter(|r| r.kind == 0 && r.name == "next")
      .collect();
    assert_eq!(calls.len(), 3);
    for name in ["wide", "narrow", "objc"] {
      let owner = product
        .items
        .iter()
        .position(|i| i.entry.name == name)
        .unwrap() as u32
        + 1;
      let call = calls.iter().find(|r| r.from_entity_index == owner).unwrap();
      assert_eq!(&source[call.start as usize..call.end as usize], "next(v)");
    }
    let objc = raw
      .root()
      .dfs()
      .find(|n| {
        n.kind() == "function_definition"
          && n
            .field("declarator")
            .is_some_and(|n| n.text().starts_with("objc("))
      })
      .unwrap();
    assert_eq!(objc.field("body").unwrap().kind(), "compound_statement");
    assert!(
      objc
        .field("body")
        .unwrap()
        .dfs()
        .any(|n| n.kind() == "preproc_if")
    );
    assert!(
      !product
        .refs
        .iter()
        .any(|r| r.kind == 0 && matches!(r.name.as_str(), "alloc" | "init" | "drain"))
    );
    assert!(product.items.iter().any(|i| i.entry.name == "following"));
    let mut owned = Vec::new();
    vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor
      .extract_product_from_root("macros.cc", &raw)
      .unwrap();
    let mut scan = Vec::new();
    vorpal_ingest::encode_product_into(&handed, &mut scan);
    assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace("[p drain];", "[p drain]"),
    lf.replace("#endif\nreturn next(v);", "return next(v);"),
    lf.replace("int objc(int v)", "namespace ns"),
    lf.replace("int objc(int v)", "namespace objc(int v)"),
    lf.replace("__OBJC__", "OTHER_DIALECT"),
    lf.replace("int objc(int v)", "Custom objc(int v)"),
    lf.replace("return next(v);\n}\n#endif", "return next(v);\n#endif"),
  ] {
    assert!(
      SgLang::from_path("macros.cc")
        .unwrap()
        .grep(&bad)
        .root()
        .has_error(),
      "{bad}"
    );
  }
}

#[test]
fn inline_friend_functions_keep_declarations_and_runtime_spans() {
  let lf = r#"
struct Stream {};
void work();
int count();
struct Column {
  inline friend Stream& operator<<(Stream& os, Column const& col) { work(); return os; }
  inline friend void inspect(Column&);
  int size() const { return count(); }
};
void following() { after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let raw = SgLang::from_path("macros.cc").unwrap().grep(&source);
    assert!(!raw.root().has_error());
    let friends: Vec<_> = raw
      .root()
      .dfs()
      .filter(|n| n.kind() == "friend_declaration")
      .collect();
    assert_eq!(friends.len(), 2);
    for node in friends {
      assert!(node.text().starts_with("inline friend"));
      assert_eq!(&source[node.range()], node.text());
    }
    let operator = raw
      .root()
      .dfs()
      .find(|n| {
        n.kind() == "function_definition"
          && n
            .field("declarator")
            .is_some_and(|d| d.text().contains("operator<<"))
      })
      .unwrap();
    assert_eq!(operator.field("type").unwrap().text(), "Stream");
    assert_eq!(operator.field("body").unwrap().kind(), "compound_statement");
    let extractor = OutlineExtractor::new().unwrap();
    let product = extractor.extract_product("macros.cc", &source).unwrap();
    assert_eq!(product.error_nodes, 0);
    assert!(!product.refs.iter().any(|r| r.name == "friend"));
    for name in ["work", "count", "after"] {
      let calls: Vec<_> = product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == name)
        .collect();
      assert_eq!(calls.len(), 1);
      assert_eq!(
        &source[calls[0].start as usize..calls[0].end as usize],
        format!("{name}()")
      );
    }
    let column = product
      .items
      .iter()
      .find(|i| i.entry.name == "Column")
      .unwrap();
    assert!(column.members.iter().any(|m| m.entry.name == "size"));
    assert!(!column.members.iter().any(|m| m.entry.name == "operator<<"));
    assert!(product.items.iter().any(|i| i.entry.name == "following"));
    let mut owned = Vec::new();
    vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor
      .extract_product_from_root("macros.cc", &raw)
      .unwrap();
    let mut scan = Vec::new();
    vorpal_ingest::encode_product_into(&handed, &mut scan);
    assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace("work();", "work()"),
    lf.replace("Stream& os, Column", "Stream& os Column"),
    lf.replace("inline friend", "inline banana friend"),
    lf.replace("return os; }", "return os;"),
  ] {
    assert!(
      SgLang::from_path("macros.cc")
        .unwrap()
        .grep(&bad)
        .root()
        .has_error(),
      "{bad}"
    );
  }
}

#[test]
fn conditional_return_logical_suffixes_keep_calls_and_original_guards() {
  let lf = r#"bool base(int); bool extra(int); int compute(); bool last();
bool run(int value) {
 return base(value)
#ifdef ON
 || extra(compute())
#endif
#if defined(SECOND)
 && last()
#endif
 ;
}
void following() { after(); }
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let raw = SgLang::from_path("macros.cc").unwrap().grep(&source);
    assert!(!raw.root().has_error());
    let node = raw
      .root()
      .dfs()
      .find(|v| v.kind() == "conditional_logical_expression")
      .unwrap();
    assert_eq!(node.field("left").unwrap().text(), "base(value)");
    let groups: Vec<_> = node
      .children()
      .filter(|n| matches!(n.kind().as_ref(), "preproc_if" | "preproc_ifdef"))
      .collect();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].field("name").unwrap().text(), "ON");
    assert_eq!(groups[0].field("operator").unwrap().text(), "||");
    assert_eq!(groups[0].field("right").unwrap().text(), "extra(compute())");
    assert_eq!(
      groups[1].field("condition").unwrap().text(),
      "defined(SECOND)"
    );
    let extractor = OutlineExtractor::new().unwrap();
    let product = extractor.extract_product("macros.cc", &source).unwrap();
    assert_eq!(product.error_nodes, 0);
    for (name, call) in [
      ("base", "base(value)"),
      ("extra", "extra(compute())"),
      ("compute", "compute()"),
      ("last", "last()"),
      ("after", "after()"),
    ] {
      let refs: Vec<_> = product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == name)
        .collect();
      assert_eq!(refs.len(), 1, "{name}: {:?}", product.refs);
      assert_eq!(&source[refs[0].start as usize..refs[0].end as usize], call);
      let expected = product
        .items
        .iter()
        .find(|i| i.entry.name == if name == "after" { "following" } else { "run" })
        .unwrap();
      let owner = 1
        + product
          .items
          .iter()
          .take_while(|i| i.entry.range != expected.entry.range)
          .map(|i| 1 + i.members.len())
          .sum::<usize>();
      assert_eq!(refs[0].from_entity_index as usize, owner);
    }
    assert!(
      !product
        .refs
        .iter()
        .any(|r| r.kind == 0 && matches!(r.name.as_str(), "ON" | "SECOND" | "defined"))
    );
    let mut owned = Vec::new();
    vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("macros.cc", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let handed = extractor
      .extract_product_from_root("macros.cc", &raw)
      .unwrap();
    let mut scan = Vec::new();
    vorpal_ingest::encode_product_into(&handed, &mut scan);
    assert_eq!(owned, scan);
  }
  for bad in [
    lf.replace(" ;", " "),
    lf.replace("|| extra(compute())", "||"),
    lf.replace("|| extra(compute())", "extra(compute())"),
    lf.replace("#ifdef ON", "#ifdef ON junk"),
    lf.replace("#endif", "//gone"),
    lf.replace("base(value)", "base(value);"),
    lf.replace("after();", "after()"),
  ] {
    assert!(
      SgLang::from_path("macros.cc")
        .unwrap()
        .grep(&bad)
        .root()
        .has_error(),
      "{bad}"
    );
  }
}

#[test]
fn ambiguous_recovered_type_prefixes_do_not_mint_cpp_runtime_callees() {
  let extractor = OutlineExtractor::new().unwrap();
  for lf in [
    "void run() { auto objects = { Scope fake(work()) }; after(); }",
    "void run() { CHECK(work(), \"message\")\n obj.fake(value()); after(); }",
    "void run() { auto objects = { first(), Type object(work()), second() }; after(); }",
    "void run() { auto objects = { Scope /* kept */ fake(work()) }; after(); }",
    "void run() { CATCH_TRY {\n#if defined(A)\n RedirectedStreams captured(work()); timer.start(); invoke();\n#else\n OutputRedirect r(value()); timer.start(); invoke();\n#endif\n } CATCH_CATCH_ALL { failed(); }\n } void following() { after(); }",
  ] {
    for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
      let raw = SgLang::from_path("ambiguous.cc").unwrap().grep(&source);
      assert!(raw.root().has_error());
      let product = extractor.extract_product("ambiguous.cc", &source).unwrap();
      assert!(product.error_nodes > 0);
      assert!(
        !product
          .refs
          .iter()
          .any(|r| r.kind == 0 && matches!(r.name.as_str(), "fake" | "object" | "captured" | "r")),
        "{:?}",
        product.refs
      );
      for name in ["work", "after"] {
        let calls: Vec<_> = product
          .refs
          .iter()
          .filter(|r| r.kind == 0 && r.name == name)
          .collect();
        assert_eq!(calls.len(), 1, "{source}: {:?}", product.refs);
        assert_eq!(
          &source[calls[0].start as usize..calls[0].end as usize],
          format!("{name}()")
        );
      }
      let mut owned = Vec::new();
      vorpal_ingest::encode_product_into(&product, &mut owned);
      let mut streamed = Vec::new();
      extractor
        .extract_product_encoded("ambiguous.cc", &source, 0, 0, &mut streamed)
        .unwrap();
      assert_eq!(owned, streamed);
      let handed = extractor
        .extract_product_from_root("ambiguous.cc", &raw)
        .unwrap();
      let mut scan = Vec::new();
      vorpal_ingest::encode_product_into(&handed, &mut scan);
      assert_eq!(owned, scan);
    }
  }
  // An unrelated real error must not hide ordinary/member/static/operator calls.
  let source = "void run() { auto objects = { 1 2 }; plain(value()); ns::staticCall(value()); obj.method(value()); operator<<(value(), value()); after(); }";
  let product = extractor.extract_product("ambiguous.cc", source).unwrap();
  assert!(product.error_nodes > 0);
  for name in ["plain", "staticCall", "method", "operator<<", "after"] {
    assert_eq!(
      product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == name)
        .count(),
      1,
      "{:?}",
      product.refs
    );
  }
  assert_eq!(
    product
      .refs
      .iter()
      .filter(|r| r.kind == 0 && r.name == "value")
      .count(),
    5
  );
  let cpp = SgLang::from_path("ambiguous.cc").unwrap();
  let bare = vorpal_lang_registry::grammar_digest(cpp).unwrap();
  assert_eq!(vorpal_ingest::grammar_generation_for(cpp), Some(bare));
  assert_eq!(
    vorpal_ingest::extraction_identity_for_path("ambiguous.cc", extractor.rules_digest()),
    Some(vorpal_ingest::extraction_identity(
      bare,
      extractor.rules_digest()
    ))
  );
}

#[test]
fn cpp_declaration_macros_are_not_foreign_constructors() {
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_language::{LanguageExt, SupportLang};
  let extractor = OutlineExtractor::new().unwrap();
  // Rule identity must reject products extracted with the legacy constructor rule.
  let rules = include_str!("../../outline/src/default_rules/cpp.yml");
  let start = rules
    .find("      # Bare constructor names must agree")
    .unwrap();
  let end = start + rules[start..].find("\nname: $NAME").unwrap();
  let legacy_rules = format!("{}{}", &rules[..start], &rules[end + 1..]);
  let legacy = OutlineExtractor::from_rules(&legacy_rules).unwrap();
  let current = OutlineExtractor::from_rules(rules).unwrap();
  assert_ne!(
    legacy.extraction_identity_for_path("declarations.cpp"),
    current.extraction_identity_for_path("declarations.cpp")
  );
  let legacy_product = legacy
    .extract_product(
      "declarations.cpp",
      "struct Holder { DECLARE_STORAGE(Item); };",
    )
    .unwrap();
  assert_eq!(
    legacy_product.items[0].members[0].entry.name,
    "DECLARE_STORAGE"
  );
  let lf = "#define DECLARE_STORAGE(Type) int* storage();\nstruct Holder {\n Holder();\n ~Holder();\n int* getter() { return value(); }\n DECLARE_STORAGE(Item);\n};\nHolder::Holder() { work(); }\nHolder::~Holder() { cleanup(); }\nvoid following() { after(); }\nvoid broken() { missing() }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = extractor
      .extract_product("declarations.cpp", &source)
      .unwrap();
    let holder = product
      .items
      .iter()
      .find(|item| item.entry.name == "Holder")
      .unwrap();
    assert_eq!(
      holder
        .members
        .iter()
        .map(|member| member.entry.name.as_ref())
        .collect::<Vec<_>>(),
      ["Holder", "~Holder", "getter"]
    );
    for name in ["Holder::Holder", "Holder::~Holder", "following"] {
      assert!(product.items.iter().any(|item| item.entry.name == name));
    }
    for name in ["value", "work", "cleanup", "after"] {
      let calls: Vec<_> = product
        .refs
        .iter()
        .filter(|reference| reference.kind == 0 && reference.name == name)
        .collect();
      assert_eq!(calls.len(), 1);
      for call in calls {
        assert_eq!(
          &source[call.start as usize..call.end as usize],
          format!("{name}()")
        );
      }
    }
    assert!(
      product
        .refs
        .iter()
        .filter(|reference| reference.kind == 0)
        .all(|reference| reference.name != "DECLARE_STORAGE")
    );
    let parsed = vorpal_lang_registry::SgLang::Builtin(SupportLang::Cpp).grep(&source);
    assert!(parsed.root().has_error());
    assert!(product.error_nodes > 0);
    let mut owned = Vec::new();
    encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("declarations.cpp", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let from_root = extractor
      .extract_product_from_root("declarations.cpp", &parsed)
      .unwrap();
    let mut scanned = Vec::new();
    encode_product_into(&from_root, &mut scanned);
    assert_eq!(owned, scanned);
  }
}

#[test]
fn cpp_specialized_constructor_products_preserve_original_calls() {
  let extractor = vorpal_ingest::OutlineExtractor::new().unwrap();
  let lf = "template<class T> struct Box { Box() { first(); } };\ntemplate<class T> struct Box<T*> { Box() { second(); } };\ntemplate<> struct Box<void> { template<class T> Box(T value) { third(); } };\nstruct Outer { struct Inner; };\nstruct Outer::Inner { Inner() { fourth(); } };\nnamespace ns { template<class T> struct Scope { struct Nested; }; }\ntemplate<class T> struct ns::Scope<T>::Nested { Nested() { fifth(); } };\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = extractor
      .extract_product("constructors.cpp", &source)
      .unwrap();
    assert_eq!(product.error_nodes, 0);
    for (owner, call) in [
      ("Box", "first"),
      ("Box<T*>", "second"),
      ("Box<void>", "third"),
      ("Outer::Inner", "fourth"),
      ("ns::Scope<T>::Nested", "fifth"),
    ] {
      let item = product
        .items
        .iter()
        .find(|i| i.entry.name == owner)
        .unwrap();
      assert_eq!(item.members.len(), 1);
      assert_eq!(
        item.members[0].entry.name,
        if owner == "Outer::Inner" {
          "Inner"
        } else if owner.contains("Nested") {
          "Nested"
        } else {
          "Box"
        }
      );
      let refs: Vec<_> = product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == call)
        .collect();
      assert_eq!(refs.len(), 1);
      assert_eq!(
        &source[refs[0].start as usize..refs[0].end as usize],
        format!("{call}()")
      );
      let owner_index = 2
        + product
          .items
          .iter()
          .take_while(|i| i.entry.name != owner)
          .map(|i| 1 + i.members.len())
          .sum::<usize>();
      assert_eq!(refs[0].from_entity_index as usize, owner_index);
    }
    let mut owned = Vec::new();
    vorpal_ingest::encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("constructors.cpp", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
  }
}

#[test]
fn cpp_unproven_declaration_macros_keep_original_sites_without_fake_owners() {
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_language::{LanguageExt, SupportLang};
  let extractor = OutlineExtractor::new().unwrap();
  let lf = "#define TEST(name) void test_##name()\n#define DECLARE_STORAGE(Type) int* storage();\nstruct Holder {\n Holder();\n ~Holder();\n int* getter() { return value(); }\n DECLARE_STORAGE(Item);\n};\nTEST(originalTest) { use(); }\n#undef TEST\nvoid TEST(int value) { use(); }\nHolder::Holder() { work(); }\nHolder::~Holder() { cleanup(); }\nvoid following() { after(); }\nvoid broken() { missing() }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = extractor
      .extract_product("declarations.cpp", &source)
      .unwrap();
    assert_eq!(
      product
        .items
        .iter()
        .filter(|item| item.entry.name == "TEST"
          && item.entry.symbol_type == vorpal_outline::model::SymbolType::Function)
        .count(),
      1
    );
    assert!(
      !product
        .items
        .iter()
        .any(|item| item.entry.name == "test_originalTest")
    );
    let holder = product
      .items
      .iter()
      .find(|item| item.entry.name == "Holder")
      .unwrap();
    assert_eq!(
      holder
        .members
        .iter()
        .map(|member| member.entry.name.as_ref())
        .collect::<Vec<_>>(),
      ["Holder", "~Holder", "getter"]
    );
    for name in ["Holder::Holder", "Holder::~Holder", "following"] {
      assert!(product.items.iter().any(|item| item.entry.name == name));
    }
    for name in ["value", "use", "work", "cleanup", "after"] {
      let calls: Vec<_> = product
        .refs
        .iter()
        .filter(|reference| reference.kind == 0 && reference.name == name)
        .collect();
      assert_eq!(calls.len(), if name == "use" { 2 } else { 1 });
      for call in calls {
        assert_eq!(
          &source[call.start as usize..call.end as usize],
          format!("{name}()")
        );
      }
    }
    assert!(
      product
        .refs
        .iter()
        .filter(|reference| reference.kind == 0)
        .all(|reference| !matches!(reference.name.as_ref(), "TEST" | "DECLARE_STORAGE"))
    );
    let first_use = product
      .refs
      .iter()
      .find(|reference| reference.kind == 0 && reference.name == "use")
      .unwrap();
    assert_eq!(
      first_use.from_entity_index, 0,
      "unproven generated owner belongs to the file"
    );
    assert!(
      !product
        .refs
        .iter()
        .any(|reference| reference.kind == 1 && reference.name == "originalTest")
    );
    let parsed = vorpal_lang_registry::SgLang::Builtin(SupportLang::Cpp).grep(&source);
    assert!(parsed.root().has_error());
    assert!(product.error_nodes > 0);
    let mut owned = Vec::new();
    encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("declarations.cpp", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let from_root = extractor
      .extract_product_from_root("declarations.cpp", &parsed)
      .unwrap();
    let mut scanned = Vec::new();
    encode_product_into(&from_root, &mut scanned);
    assert_eq!(owned, scanned);
  }
}

#[test]
fn cpp_unproven_function_heads_keep_body_local_types_separate() {
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_language::{LanguageExt, SupportLang};
  let extractor = OutlineExtractor::new().unwrap();
  let lf = "struct A { void ping(); }; struct B { void ping(); };\n#define GENERATE(name) void test_##name()\nGENERATE(one) { A object; object.ping(); auto alias = object; alias = object; alias.ping(); }\nGENERATE(two) { B object; object.ping(); auto alias = object; alias = object; alias.ping(); }\n#undef GENERATE\nvoid following() { A object; object.ping(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = extractor.extract_product("bodies.cpp", &source).unwrap();
    assert!(
      !product
        .items
        .iter()
        .any(|item| item.entry.name == "GENERATE"
          && item.entry.symbol_type == vorpal_outline::model::SymbolType::Function)
    );
    assert!(
      !product
        .refs
        .iter()
        .any(|reference| reference.kind == 1 && matches!(reference.name.as_ref(), "one" | "two"))
    );
    let mut calls: Vec<_> = product
      .refs
      .iter()
      .filter(|reference| reference.kind == 0 && reference.name == "ping")
      .collect();
    calls.sort_by_key(|reference| reference.start);
    assert_eq!(calls.len(), 5);
    for (call, ty) in calls
      .iter()
      .zip([Some("A"), None, Some("B"), None, Some("A")])
    {
      assert_eq!(call.receiver_type.as_deref(), ty, "{call:?}");
      let original = &source[call.start as usize..call.end as usize];
      assert!(matches!(original, "object.ping()" | "alias.ping()"));
    }
    assert!(calls[..4].iter().all(|call| call.from_entity_index == 0));
    assert_ne!(calls[4].from_entity_index, 0);
    let mut owned = Vec::new();
    encode_product_into(&product, &mut owned);
    let mut streamed = Vec::new();
    extractor
      .extract_product_encoded("bodies.cpp", &source, 0, 0, &mut streamed)
      .unwrap();
    assert_eq!(owned, streamed);
    let parsed = vorpal_lang_registry::SgLang::Builtin(SupportLang::Cpp).grep(&source);
    let scanned = extractor
      .extract_product_from_root("bodies.cpp", &parsed)
      .unwrap();
    let mut bytes = Vec::new();
    encode_product_into(&scanned, &mut bytes);
    assert_eq!(owned, bytes);
  }
}

#[test]
fn anonymous_cpp_bodies_do_not_promote_local_types_or_export_their_facts() {
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_language::{LanguageExt, SupportLang};
  let extractor = OutlineExtractor::new().unwrap();
  let lf = "struct A { void ping(); };\nstruct Local { void ping(); static void factory(); };\nstruct Alias { void ping(); };\n#define GENERATE(name) void test_##name()\nGENERATE(one) {\n struct Local { A field; A method(A parameter) { parameter.ping(); return field; } };\n using Alias = Local; typedef Local* Pointer;\n Local object; object.ping(); auto created = Local(); Local::factory();\n Alias alias; alias.ping(); Pointer pointer; pointer.ping();\n A known; known.ping();\n}\nGENERATE(two) { A object; object.ping(); }\n#undef GENERATE\nvoid following() { Local object; object.ping(); Alias alias; alias.ping(); field.ping(); }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = extractor
      .extract_product("local-types.cpp", &source)
      .unwrap();
    assert_eq!(product.error_nodes, 0);
    for name in ["Local", "Alias"] {
      assert_eq!(
        product
          .items
          .iter()
          .filter(|i| i.entry.name == name)
          .count(),
        1
      );
    }
    assert!(!product.items.iter().any(|i| i.entry.name == "Pointer"));
    assert!(!product.items.iter().any(|i| i.entry.name == "GENERATE"
      && i.entry.symbol_type == vorpal_outline::model::SymbolType::Function));
    assert!(!product.returns.iter().any(|(name, _)| name == "method"));
    assert!(
      product.entity_params.is_empty(),
      "local method parameters cannot attach to the file"
    );
    let start = source.find("GENERATE(one)").unwrap();
    let end = source.find("GENERATE(two)").unwrap();
    assert!(
      !product
        .refs
        .iter()
        .any(|r| (start..end).contains(&(r.start as usize))
          && matches!(r.name.as_ref(), "Local" | "Alias" | "Pointer"))
    );
    for (site, ty) in [
      ("object.ping()", None),
      ("alias.ping()", None),
      ("pointer.ping()", None),
      ("known.ping()", Some("A")),
      ("parameter.ping()", None),
    ] {
      let call = product
        .refs
        .iter()
        .find(|r| r.kind == 0 && &source[r.start as usize..r.end as usize] == site)
        .unwrap();
      assert_eq!(call.receiver_type.as_deref(), ty, "{call:?}");
    }
    let following = source.find("void following").unwrap();
    for (site, ty) in [
      ("object.ping()", Some("Local")),
      ("alias.ping()", Some("Alias")),
      ("field.ping()", None),
    ] {
      let call = product
        .refs
        .iter()
        .find(|r| {
          r.kind == 0
            && r.start as usize > following
            && &source[r.start as usize..r.end as usize] == site
        })
        .unwrap();
      assert_eq!(call.receiver_type.as_deref(), ty, "{call:?}");
    }
    let mut bytes = Vec::new();
    encode_product_into(&product, &mut bytes);
    let mut streaming = Vec::new();
    extractor
      .extract_product_encoded("local-types.cpp", &source, 0, 0, &mut streaming)
      .unwrap();
    assert_eq!(bytes, streaming);
    let parsed = vorpal_lang_registry::SgLang::Builtin(SupportLang::Cpp).grep(&source);
    let scan = extractor
      .extract_product_from_root("local-types.cpp", &parsed)
      .unwrap();
    let mut scanned = Vec::new();
    encode_product_into(&scan, &mut scanned);
    assert_eq!(bytes, scanned);
  }
}

#[test]
fn anonymous_cpp_type_uses_preserve_file_and_body_domains() {
  use vorpal_ingest::{OutlineExtractor, encode_product_into};
  use vorpal_language::{LanguageExt, SupportLang};
  let extractor = OutlineExtractor::new().unwrap();
  let lf = "struct Shared {};\n#define HEAD(name) void test_##name()\nHEAD(one) { Shared first; Shared duplicate; }\nHEAD(two) { Shared second; }\nShared global;\nvoid following() { Shared local; }\n";
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = extractor
      .extract_product("type-domains.cpp", &source)
      .unwrap();
    assert_eq!(product.error_nodes, 0);
    let types: Vec<_> = product
      .refs
      .iter()
      .filter(|r| r.kind == 1 && r.name == "Shared")
      .collect();
    let expected: Vec<_> = [
      "Shared first",
      "Shared second",
      "Shared global",
      "Shared local",
    ]
    .iter()
    .map(|site| source.find(site).unwrap() as u32)
    .collect();
    assert_eq!(types.iter().map(|r| r.start).collect::<Vec<_>>(), expected);
    assert!(types[..3].iter().all(|r| r.from_entity_index == 0));
    assert_ne!(types[3].from_entity_index, 0);
    let mut bytes = Vec::new();
    encode_product_into(&product, &mut bytes);
    let mut streaming = Vec::new();
    extractor
      .extract_product_encoded("type-domains.cpp", &source, 0, 0, &mut streaming)
      .unwrap();
    assert_eq!(bytes, streaming);
    let parsed = vorpal_lang_registry::SgLang::Builtin(SupportLang::Cpp).grep(&source);
    let scanned = extractor
      .extract_product_from_root("type-domains.cpp", &parsed)
      .unwrap();
    let mut scan = Vec::new();
    encode_product_into(&scanned, &mut scan);
    assert_eq!(bytes, scan);
  }
}

#[test]
fn cpp_native_linkage_prototypes_preserve_nested_heads_and_call_sites() {
  let lf = r#"#ifndef OUTER
#define OUTER
extern "C" __declspec(dllimport) void __stdcall DebugBreak();
extern "C" __declspec(dllimport) int __stdcall IsDebuggerPresent();
#define TRAP() DebugBreak()
#ifndef DEBUG_SKIP
#ifdef TRAP
#endif
#endif
#ifndef __OBJC__
#if defined(WIDE)
extern "C" int wide(int argc, wchar_t* argv[]) {
#else
int narrow(int argc, char* argv[]) {
#endif
  return touch(argc);
}
#else
int objc_entry() { return alternative(); }
#endif
void following() { DebugBreak(); observe(IsDebuggerPresent()); }
#endif
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    for name in ["wide", "narrow", "objc_entry", "following"] {
      assert!(
        product.items.iter().any(|item| item.entry.name == name),
        "{name}"
      );
    }
    for (name, spelling, count) in [
      ("DebugBreak", "DebugBreak()", 1),
      ("IsDebuggerPresent", "IsDebuggerPresent()", 1),
      ("observe", "observe(IsDebuggerPresent())", 1),
      ("touch", "touch(argc)", 2),
      ("alternative", "alternative()", 1),
    ] {
      let calls: Vec<_> = product
        .refs
        .iter()
        .filter(|r| r.kind == 0 && r.name == name)
        .collect();
      assert_eq!(calls.len(), count, "{name}");
      for call in calls {
        assert_eq!(&source[call.start as usize..call.end as usize], spelling);
      }
    }
    assert!(!product.refs.iter().any(|r| r.kind == 0
      && ["TRAP", "__declspec", "dllimport", "__stdcall"].contains(&r.name.as_str())));
    let raw = SgLang::from_path("linkage.cc").unwrap().grep(&source);
    assert_eq!(
      raw
        .root()
        .dfs()
        .filter(|n| n.kind() == "ms_declspec_modifier")
        .count(),
      2
    );
    assert_eq!(
      raw
        .root()
        .dfs()
        .filter(|n| n.kind() == "ms_call_modifier")
        .count(),
      2
    );
    assert_eq!(
      raw
        .root()
        .dfs()
        .filter(|n| n.kind() == "linkage_specification" && n.text().contains("__declspec"))
        .count(),
      2
    );
  }
  for bad in [
    "extern \"C\" __declspec(dllimport) int __stdcall broken()",
    "extern \"C\" __declspec(dllimport) int __stdcall broken(int value;",
    "void following() { DebugBreak() }",
  ] {
    assert!(
      SgLang::from_path("linkage.cc")
        .unwrap()
        .grep(bad)
        .root()
        .has_error(),
      "{bad}"
    );
  }
}

#[test]
fn cpp_native_linkage_keeps_incomplete_guard_errors_after_the_prototype() {
  let lf = r#"extern "C" __declspec(dllimport) void __stdcall DebugBreak();
#define TRAP() DebugBreak()
#ifndef DEBUG_SKIP
#ifdef TRAP
#endif
#ifndef __OBJC__
#if defined(WIDE)
extern "C" int wide(int argc, wchar_t* argv[]) {
#else
int narrow(int argc, char* argv[]) {
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let raw = SgLang::from_path("linkage.cc").unwrap().grep(&source);
    assert!(raw.root().has_error());
    assert_eq!(raw.root().kind(), "translation_unit");
    let product = OutlineExtractor::new()
      .unwrap()
      .extract_product("linkage.cc", &source)
      .unwrap();
    assert!(product.error_nodes > 0);
    let damaged = source.find("#ifndef DEBUG_SKIP").unwrap();
    assert!(product.error_spans.iter().all(|span| (span.0 as usize) >= damaged));
    assert_eq!(
      raw
        .root()
        .dfs()
        .filter(|n| n.kind() == "ms_declspec_modifier")
        .count(),
      1
    );
    assert!(
      !product
        .refs
        .iter()
        .any(|r| r.kind == 0 && ["DebugBreak", "TRAP"].contains(&r.name.as_str()))
    );
  }
}

#[test]
fn cpp_native_linkage_cannot_swallow_macros_namespaces_or_function_owners() {
  let lf = r#"#ifndef OUTER
#define OUTER
#if defined(PLATFORM_A)
extern "C" __declspec(dllimport) void __stdcall DebugBreak();
#define TRAP() DebugBreak()
#ifndef BREAK_NOW
#ifdef TRAP
#define BREAK_NOW() []{ if (Native::isDebuggerActive()) { TRAP(); } }()
#endif
#endif
#endif
#if defined(PLATFORM_A)
extern "C" __declspec(dllimport) int __stdcall IsDebuggerPresent();
namespace Native {
  bool isDebuggerActive() { return IsDebuggerPresent() != 0; }
}
#elif defined(PLATFORM_B)
extern "C" __declspec(dllimport) int __stdcall IsDebuggerPresent();
namespace Native {
  bool isDebuggerActive() { return IsDebuggerPresent() != 0; }
}
#endif
#endif
"#;
  for source in [lf.to_owned(), lf.replace('\n', "\r\n")] {
    let product = clean_product(&source);
    assert_eq!(product.cuts, vec![(0, false)]);
    assert!(!product.items.iter().any(|i| {
      i.entry.symbol_type == vorpal_outline::model::SymbolType::Function
        && ["DebugBreak", "IsDebuggerPresent", "TRAP", "BREAK_NOW"].contains(&i.entry.name.as_ref())
    }));
    for name in ["TRAP", "BREAK_NOW"] {
      assert!(
        product.items.iter().any(|i| i.entry.name == name
          && i.entry.symbol_type == vorpal_outline::model::SymbolType::Macro)
      );
    }
    assert_eq!(
      product
        .items
        .iter()
        .filter(|i| i.entry.name == "Native"
          && i.entry.symbol_type == vorpal_outline::model::SymbolType::Module)
        .count(),
      2
    );
    let owners: Vec<_> = product
      .items
      .iter()
      .enumerate()
      .filter(|(_, i)| i.entry.name == "isDebuggerActive")
      .map(|(idx, _)| idx as u32 + 1)
      .collect();
    assert_eq!(owners.len(), 2);
    let calls: Vec<_> = product.refs.iter().filter(|r| r.kind == 0).collect();
    assert_eq!(calls.len(), 2);
    for owner in owners {
      let call = calls.iter().find(|r| r.from_entity_index == owner).unwrap();
      assert_eq!(call.name, "IsDebuggerPresent");
      assert_eq!(
        &source[call.start as usize..call.end as usize],
        "IsDebuggerPresent()"
      );
    }
    assert!(product.signatures.is_empty());
  }
}


#[test]
fn conditional_return_prefix_and_values_preserve_all_arms_and_owners() {
  let lf = r#"bool prefix(); bool selected(); bool fallback(); bool next();
namespace Palette {
bool use() {
 return
#if FLAG_A
 prefix() &&
#endif
#if FLAG_B
 selected()
#else
 fallback()
#endif
 ;
}
bool following() { return next(); }
}
"#;
  for form in [
    lf.to_owned(),
    lf.replace("#if FLAG_A", "#ifdef FLAG_A")
      .replace("#if FLAG_B", "#ifndef FLAG_B"),
    lf.replace("#if FLAG_A\n prefix() &&\n#endif\n", ""),
  ] {
    for source in [form.clone(), form.replace('\n', "\r\n")] {
      let raw = SgLang::from_path("conditional.cc").unwrap().grep(&source);
      assert!(!raw.root().has_error());
      let value = raw
        .root()
        .dfs()
        .find(|n| n.kind() == "conditional_return_expression")
        .unwrap();
      let groups: Vec<_> = value
        .children()
        .filter(|n| matches!(n.kind().as_ref(), "preproc_if" | "preproc_ifdef"))
        .collect();
      assert_eq!(
        groups.len(),
        if source.contains("prefix() &&") { 2 } else { 1 }
      );
      let last = groups.last().unwrap();
      assert_eq!(last.field("left").unwrap().text(), "selected()");
      let alternative = last.field("alternative").unwrap();
      assert_eq!(alternative.kind(), "preproc_else");
      assert_eq!(alternative.field("right").unwrap().text(), "fallback()");
      if groups.len() == 2 {
        assert_eq!(groups[0].field("left").unwrap().text(), "prefix()");
        assert_eq!(groups[0].field("operator").unwrap().text(), "&&");
      }
      let extractor = OutlineExtractor::new().unwrap();
      let product = extractor
        .extract_product("conditional.cc", &source)
        .unwrap();
      assert_eq!(product.error_nodes, 0);
      let namespace = product
        .items
        .iter()
        .find(|i| i.entry.name == "Palette")
        .unwrap();
      assert_eq!(
        namespace.entry.range.byte_offset.end,
        source.rfind('}').unwrap() + 1
      );
      for name in ["prefix", "selected", "fallback", "next"] {
        let calls: Vec<_> = product
          .refs
          .iter()
          .filter(|r| r.kind == 0 && r.name == name)
          .collect();
        let count = usize::from(name != "prefix" || groups.len() == 2);
        assert_eq!(calls.len(), count, "{name}");
        for call in calls {
          assert_eq!(
            &source[call.start as usize..call.end as usize],
            format!("{name}()")
          );
          let owner = product
            .items
            .iter()
            .position(|i| i.entry.name == if name == "next" { "following" } else { "use" })
            .unwrap() as u32
            + 1;
          assert_eq!(call.from_entity_index, owner);
        }
      }
      assert!(
        !product
          .refs
          .iter()
          .any(|r| r.kind == 0 && ["FLAG_A", "FLAG_B"].contains(&r.name.as_str()))
      );
      let mut owned = Vec::new();
      vorpal_ingest::encode_product_into(&product, &mut owned);
      let mut streamed = Vec::new();
      extractor
        .extract_product_encoded("conditional.cc", &source, 0, 0, &mut streamed)
        .unwrap();
      assert_eq!(owned, streamed);
      let scan = extractor
        .extract_product_from_root("conditional.cc", &raw)
        .unwrap();
      let mut handed = Vec::new();
      vorpal_ingest::encode_product_into(&scan, &mut handed);
      assert_eq!(owned, handed);
    }
  }
  for bad in [
    lf.replace(" fallback()", ""),
    lf.replace("#else\n fallback()\n", ""),
    lf.replace("#endif\n ;", " ;"),
    lf.replace(" ;\n", "\n"),
    lf.replace("prefix() &&", "prefix()"),
    "bool f() { ordinary() }\n".to_owned(),
  ] {
    for source in [bad.clone(), bad.replace('\n', "\r\n")] {
      assert!(
        SgLang::from_path("conditional.cc")
          .unwrap()
          .grep(&source)
          .root()
          .has_error(),
        "{source}"
      );
    }
  }
}

#[test]
fn cpp_conditional_catch_groups_retain_guards_calls_and_real_errors() {
  for newline in ["\n", "\r\n"] {
    for (opening, condition) in [("#if !defined(DISABLE)", "!defined(DISABLE)"),
                                 ("#ifdef ENABLE", "ENABLE"),
                                 ("#ifndef DISABLE", "DISABLE")] {
      let source = format!("// ÃŽÂ±\nvoid run() {{ try {{ work(); }}\n{opening}\ncatch (Error& error) {{ recover(error); }}\ncatch (...) {{ fallback(); }}\n#endif\nafter(); }}\nvoid following() {{ final_call(); }}\n").replace('\n', newline);
      let parsed = SgLang::from_path("handlers.cc").unwrap().grep(&source);
      assert!(!parsed.root().has_error(), "{source}");
      let guard = parsed.root().dfs().find(|n| n.kind() == "preproc_if" || n.kind() == "preproc_ifdef").unwrap();
      assert_eq!(guard.field(if opening.starts_with("#if ") { "condition" } else { "name" }).unwrap().text(), condition);
      let handlers: Vec<_> = guard.dfs().filter(|n| n.kind() == "catch_clause").collect();
      assert_eq!(handlers.len(), 2);
      assert_eq!(handlers[0].field("parameters").unwrap().text(), "(Error& error)");
      assert_eq!(handlers[0].field("body").unwrap().text(), "{ recover(error); }");
      let product = clean_product(&source);
      for (name, spelling) in [("work", "work()"), ("recover", "recover(error)"), ("fallback", "fallback()"), ("after", "after()"), ("final_call", "final_call()")] {
        let refs: Vec<_> = product.refs.iter().filter(|r| r.kind == 0 && r.name == name).collect();
        assert_eq!(refs.len(), 1, "{name}");
        assert_eq!(&source[refs[0].start as usize..refs[0].end as usize], spelling);
      }
      assert!(product.items.iter().any(|i| i.entry.name == "following"));
    }
    for invalid in [
      "void run() { try { work(); }\n#if ENABLE\n#endif\n}",
      "void run() { try { work(); }\n#if ENABLE\ncatch (...) { recover() }\n#endif\n}",
      "void run() { try { work(); }\n#if ENABLE\ncatch (...) { recover(); }\n}",
      "void run() { try { work(); }\n#if ENABLE\ncatch (...) { recover(); }\n#else\nafter();\n#endif\n}",
      "void run() { try { work(); } after(); }",
    ] {
      let source = invalid.replace('\n', newline);
      assert!(SgLang::from_path("handlers.cc").unwrap().grep(&source).root().has_error(), "{source}");
    }
  }
}
