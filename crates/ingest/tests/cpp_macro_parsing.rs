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
