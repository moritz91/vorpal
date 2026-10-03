use vorpal_ingest::OutlineExtractor;
use vorpal_core::{Language, tree_sitter::LanguageExt};
use vorpal_lang_registry::SgLang;

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
