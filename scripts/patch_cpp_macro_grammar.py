"""Apply the fork's C++ macro grammar extensions to the vendored grammar JSON.

Regenerate with tree-sitter 0.25.10 generate --abi 14 src/grammar.json.
This preserves original source bytes and node spans; no source preprocessing is used.
"""
import json
import sys
from pathlib import Path

path = Path(__file__).resolve().parents[1] / 'grammars/tree-sitter-cpp/src/grammar.json'
grammar = json.loads(path.read_text(encoding='utf-8'))
rules = grammar['rules']
def symbol(name): return {'type': 'SYMBOL', 'name': name}
def choice(*members): return {'type': 'CHOICE', 'members': list(members)}
def seq(*members): return {'type': 'SEQ', 'members': list(members)}
def repeat(content): return {'type': 'REPEAT', 'content': content}
def optional(content): return choice(content, {'type': 'BLANK'})

# A leading qualifier makes this unambiguously a type argument.
# Bare identifiers keep the existing expression parse (no type/value guessing).
pointer_or_reference = optional(choice(symbol('abstract_pointer_declarator'), symbol('abstract_reference_declarator')))
rules['macro_type_argument'] = {'type': 'PREC_DYNAMIC', 'value': -1, 'content': choice(
    seq({'type': 'REPEAT1', 'content': symbol('type_qualifier')},
        symbol('type_specifier'), repeat(symbol('type_qualifier')),
        pointer_or_reference),
)}
def add_argument_type(node):
    if node.get('type') == 'CHOICE' and any(m == symbol('expression') for m in node['members']):
        if symbol('macro_type_argument') not in node['members']:
            node['members'].append(symbol('macro_type_argument'))
        qualifier_name = {'type': 'ALIAS', 'content': symbol('type_qualifier'), 'named': True, 'value': 'identifier'}
        if qualifier_name not in node['members']:
            node['members'].append(qualifier_name)
    for value in node.values():
        if isinstance(value, dict): add_argument_type(value)
        elif isinstance(value, list):
            for child in value:
                if isinstance(child, dict): add_argument_type(child)
add_argument_type(rules['argument_list'])

# Primitive type keywords after an argument comma are unambiguous type metadata
# (for example va_arg(list, int)). Do not enable them in the first argument slot:
# that would compete with ordinary parenthesized function/member declarations.
rules['_primitive_macro_type_argument'] = {'type': 'PREC_DYNAMIC', 'value': -1,
    'content': seq(symbol('primitive_type'), repeat(symbol('type_qualifier')),
                   pointer_or_reference)}
primitive_argument = {'type': 'ALIAS', 'content': symbol('_primitive_macro_type_argument'),
                      'named': True, 'value': 'macro_type_argument'}
def add_trailing_primitive_argument(node):
    if node.get('type') == 'SEQ' and node.get('members', [None])[0] == {'type': 'STRING', 'value': ','}:
        argument = node['members'][1]
        if argument.get('type') == 'CHOICE' and symbol('expression') in argument['members']:
            if primitive_argument not in argument['members']:
                argument['members'].append(primitive_argument)
    for value in node.values():
        if isinstance(value, dict): add_trailing_primitive_argument(value)
        elif isinstance(value, list):
            for child in value:
                if isinstance(child, dict): add_trailing_primitive_argument(child)
add_trailing_primitive_argument(rules['argument_list'])
grammar['conflicts'] = [c for c in grammar['conflicts'] if c != ['type_specifier', 'macro_type_argument']]
conflict = ['_declaration_modifiers', 'macro_type_argument']
if conflict not in grammar['conflicts']:
    grammar['conflicts'].append(conflict)
grammar['conflicts'] = [c for c in grammar['conflicts'] if c != ['type_specifier', 'call_expression', 'macro_type_argument']]

literal = choice(symbol('string_literal'), symbol('raw_string_literal'))
piece = choice(symbol('identifier'), symbol('string_literal'), symbol('raw_string_literal'))
rules['concatenated_string'] = {'type': 'PREC_RIGHT', 'value': 1, 'content': choice(
    seq(literal, {'type': 'REPEAT1', 'content': piece}),
    seq(symbol('identifier'), literal, repeat(piece)),
)}

# __has_include accepts header names, not just arithmetic preprocessor expressions.
def add_header_args(node):
    if node.get('type') == 'CHOICE' and symbol('system_lib_string') in node['members']:
        return
    if node == symbol('_preproc_expression'):
        node.clear()
        node.update(choice(symbol('_preproc_expression'), symbol('system_lib_string'), symbol('string_literal')))
        return
    for value in list(node.values()):
        if isinstance(value, dict): add_header_args(value)
        elif isinstance(value, list):
            for child in value:
                if isinstance(child, dict): add_header_args(child)
add_header_args(rules['preproc_argument_list'])

# typeid is a C++ operator, not an ordinary function call with value-only arguments.
rules['typeid_expression'] = {
    'type': 'PREC_RIGHT', 'value': 13, 'content': seq(
        {'type': 'STRING', 'value': 'typeid'}, {'type': 'STRING', 'value': '('},
        choice({'type': 'FIELD', 'name': 'type', 'content': symbol('type_descriptor')},
               {'type': 'FIELD', 'name': 'value', 'content': symbol('expression')}),
        {'type': 'STRING', 'value': ')'}),
}
if symbol('typeid_expression') not in rules['_expression_not_binary']['members']:
    rules['_expression_not_binary']['members'].append(symbol('typeid_expression'))

# New-expressions may allocate cv-qualified objects and arrays of pointers.
new_members = rules['new_expression']['content']['members']
type_at = next(i for i, m in enumerate(new_members) if m.get('name') == 'type')
if new_members[type_at - 1] != repeat(symbol('type_qualifier')):
    new_members.insert(type_at, repeat(symbol('type_qualifier')))
new_declarator = rules['new_declarator']
if new_declarator['type'] != 'CHOICE':
    rules['new_declarator'] = choice(new_declarator, {
        'type': 'PREC_RIGHT', 'value': 0, 'content': seq(
            {'type': 'STRING', 'value': '*'}, repeat(symbol('type_qualifier')),
            optional(symbol('new_declarator')))
    })

field_base = rules['field_expression']
if field_base['type'] == 'CHOICE':
    field_base = field_base['members'][0]
operators = field_base['members'][0]['content']['members'][1]['content']['members']
arrow_star = {'type': 'STRING', 'value': '->*'}
if arrow_star not in operators:
    operators.append(arrow_star)

# SDKs hide calling conventions/export annotations behind object macros.
# Accept identifiers only after a type or inside a parenthesized declarator;
# prefer the original parse when an ordinary identifier interpretation is viable.
sdk_modifier = {
    'type': 'PREC_DYNAMIC', 'value': -1, 'content': {
        'type': 'ALIAS', 'named': True, 'value': 'identifier',
        'content': symbol('identifier'),
    },
}
rules['sdk_call_modifier'] = sdk_modifier
grammar['conflicts'] = [c for c in grammar['conflicts']
    if c != ['expression', 'sdk_call_modifier']]
for conflict in [['sdk_call_modifier', '_declarator'],
                 ['_declarator', 'type_specifier', 'expression', 'sdk_call_modifier'],
                 ['type_specifier', 'expression', 'sdk_call_modifier'],
                 ['type_specifier', 'sdk_call_modifier'],
                 ['_field_declarator', 'sdk_call_modifier'],
                 ['_declarator', 'type_specifier', 'sdk_call_modifier'],
                 ['_type_declarator', 'sdk_call_modifier']]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)
for name in ['declaration', 'field_declaration']:
    members = rules[name]['members']
    if members[1] != optional(symbol('sdk_call_modifier')):
        members.insert(1, optional(symbol('sdk_call_modifier')))

def sdk_in_parentheses(node):
    if node == symbol('ms_call_modifier'):
        node.clear()
        node.update(choice(symbol('ms_call_modifier'), symbol('sdk_call_modifier')))
        return
    if node == choice(symbol('ms_call_modifier'), symbol('sdk_call_modifier')):
        return
    for value in list(node.values()):
        if isinstance(value, dict): sdk_in_parentheses(value)
        elif isinstance(value, list):
            for child in value:
                if isinstance(child, dict): sdk_in_parentheses(child)
for name in ['parenthesized_declarator', 'parenthesized_field_declarator',
             'parenthesized_type_declarator', 'abstract_parenthesized_declarator']:
    sdk_in_parentheses(rules[name])
definition = rules['function_definition']['members']
type_position = definition.index(symbol('_declaration_specifiers'))
definition[type_position + 1] = optional(choice(symbol('ms_call_modifier'), symbol('sdk_call_modifier')))

# Inline members have a distinct definition rule; conventions must precede only
# their declarator, without broadening general field/declaration modifiers.
inline_members = rules['inline_method_definition']['members']
inline_convention = optional(choice(symbol('ms_call_modifier'), symbol('sdk_call_modifier')))
if inline_members[1] != inline_convention:
    inline_members.insert(1, inline_convention)

# Keep initializer commas strict, while admitting conditional branches as list entries.
# Reuse the normal preprocessor shape/aliases so source spans and branch nodes survive.
initializer_value = choice(symbol('initializer_pair'), symbol('expression'), symbol('initializer_list'))
grammar['conflicts'] = [c for c in grammar['conflicts']
    if c != ['comma_expression', 'initializer_list']]
conflict = ['comma_expression', '_initializer_entries']
if conflict not in grammar['conflicts']:
    grammar['conflicts'].append(conflict)
groups = ['preproc_if', 'preproc_ifdef', 'preproc_else', 'preproc_elif', 'preproc_elifdef']
def initializer_group(name):
    return {'type': 'ALIAS', 'content': symbol(name + '_in_initializer_list'),
            'named': True, 'value': name}
rules['_initializer_entries'] = choice(
    seq({'type': 'REPEAT1', 'content': choice(
        seq(initializer_value, {'type': 'STRING', 'value': ','}),
        initializer_group('preproc_if'), initializer_group('preproc_ifdef'))},
        optional(initializer_value)),
    initializer_value,
)
def initializer_preproc(node):
    if node == repeat(symbol('_block_item')):
        return optional(symbol('_initializer_entries'))
    if node.get('type') == 'SYMBOL' and node.get('name') in groups:
        return initializer_group(node['name'])
    return {key: ([initializer_preproc(c) if isinstance(c, dict) else c for c in value]
                  if isinstance(value, list) else
                  initializer_preproc(value) if isinstance(value, dict) else value)
            for key, value in node.items()}
for name in groups:
    rules[name + '_in_initializer_list'] = initializer_preproc(rules[name])
    conflict = [name, name + '_in_initializer_list']
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)
rules['initializer_list'] = seq({'type': 'STRING', 'value': '{'},
                                optional(symbol('_initializer_entries')), {'type': 'STRING', 'value': '}'})

# Default parameters accept unnamed pointers as well as unnamed references.
# Avoid widening this to every abstract declarator: that changes unrelated parses.
def default_pointer(node):
    if node.get('type') == 'CHOICE' and symbol('abstract_reference_declarator') in node['members']:
        if symbol('abstract_pointer_declarator') not in node['members']:
            node['members'].append(symbol('abstract_pointer_declarator'))
    for value in node.values():
        if isinstance(value, dict): default_pointer(value)
        elif isinstance(value, list):
            for child in value:
                if isinstance(child, dict): default_pointer(child)
default_pointer(rules['optional_parameter_declaration'])

# Member pointer fields and abstract types require a named class scope.
member_scope = json.loads(json.dumps(rules['_scope_resolution']))
scope_field = member_scope['content']['members'][0]
scope_field['content'] = scope_field['content']['members'][0]
rules['_member_pointer_scope'] = member_scope
for name in ['pointer_field_declarator']:
    original = rules[name]['members'][0] if rules[name]['type'] == 'CHOICE' else rules[name]
    member = json.loads(json.dumps(original))
    member['value'] = -1
    members = member['content']['content']['members']
    at = next(i for i, m in enumerate(members) if m == {'type': 'STRING', 'value': '*'})
    members.insert(at, {'type': 'REPEAT1', 'content': symbol('_member_pointer_scope')})
    rules[name] = choice(original, member)
conflict = ['_scope_resolution', '_member_pointer_scope']
if conflict not in grammar['conflicts']:
    grammar['conflicts'].append(conflict)
# Abstract member pointers are admitted only as explicit type descriptors.
# Keep the global abstract-pointer production unchanged (notably ::delete[]).
def alias_rule(name, visible):
    return {'type': 'ALIAS', 'content': symbol(name), 'named': True, 'value': visible}
member_pointer = json.loads(json.dumps(rules['abstract_pointer_declarator']))
member_pointer['value'] = -1
member_pointer['content']['content']['members'].insert(0, {'type': 'REPEAT1', 'content': symbol('_member_pointer_scope')})
rules['_abstract_member_pointer'] = member_pointer
rules['_abstract_member_parenthesized'] = {'type': 'PREC', 'value': 1, 'content': seq(
    {'type': 'STRING', 'value': '('},
    alias_rule('_abstract_member_pointer', 'abstract_pointer_declarator'),
    {'type': 'STRING', 'value': ')'})}
rules['_abstract_member_function'] = seq(
    {'type': 'FIELD', 'name': 'declarator', 'content': alias_rule('_abstract_member_parenthesized', 'abstract_parenthesized_declarator')},
    symbol('_function_declarator_seq'))
descriptor = rules['type_descriptor']['content']['members'][-1]['content']['members']
member_function = alias_rule('_abstract_member_function', 'abstract_function_declarator')
if member_function not in descriptor:
    descriptor.insert(1, member_function)
member_data = alias_rule('_abstract_member_pointer', 'abstract_pointer_declarator')
if member_data not in descriptor:
    descriptor.insert(1, member_data)
# Parameter annotations are declaration metadata, never runtime calls.
# Their identifiers do not introduce lexer tokens or affect expression contexts.
rules['sdk_parameter_annotation'] = {'type': 'PREC_DYNAMIC', 'value': -1, 'content': choice(
    seq(symbol('identifier'), symbol('argument_list')), symbol('identifier'))}
for conflict in [
    ['sized_type_specifier', 'sdk_parameter_annotation'],
    ['type_specifier', 'expression', 'sdk_parameter_annotation'],
    ['type_specifier', 'sdk_call_modifier', 'sdk_parameter_annotation'],
    ['_declarator', 'type_specifier', 'sdk_call_modifier', 'sdk_parameter_annotation'],
    ['_declarator', 'type_specifier', 'sdk_parameter_annotation'],
    ['type_specifier', 'sdk_parameter_annotation'],
]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)
for name in ['parameter_declaration', 'optional_parameter_declaration', 'variadic_parameter_declaration']:
    members = rules[name]['members']
    annotations = repeat(symbol('sdk_parameter_annotation'))
    if members[0] != annotations:
        members.insert(0, annotations)
# C++ permits braced defaults without an explicit type: parameter = {} or {value}.
# Admit initializer lists only in the explicit parameter default-value context.
field = rules['optional_parameter_declaration']['members'][-1]
if field['content'] == symbol('expression'):
    field['content'] = choice(symbol('expression'), symbol('initializer_list'))
# Explicit instantiations need an annotation-free type path for unnamed callbacks
# returning a user-defined type. Keep the existing declarator path in parallel:
# SDK-annotated instantiations must retain their previous parse. Do not widen
# abstract declarators or change expression/ordinary parameter contexts.
instantiation_parameters = [
    'parameter_declaration', 'optional_parameter_declaration',
    'variadic_parameter_declaration',
]
for name in instantiation_parameters:
    plain = json.loads(json.dumps(rules[name]))
    plain['members'].pop(0)  # the SDK annotation repeat added immediately above
    rules['_instantiation_' + name] = plain

def instantiation_parameter_list(node):
    if isinstance(node, list):
        return [instantiation_parameter_list(value) for value in node]
    if not isinstance(node, dict):
        return node
    if node.get('type') == 'SYMBOL' and node.get('name') in instantiation_parameters:
        return alias_rule('_instantiation_' + node['name'], node['name'])
    return {key: instantiation_parameter_list(value) for key, value in node.items()}

rules['_instantiation_parameter_list'] = instantiation_parameter_list(rules['parameter_list'])
function_tail = json.loads(json.dumps(rules['_function_declarator_seq']))
function_tail['members'][0]['content'] = alias_rule('_instantiation_parameter_list', 'parameter_list')
rules['_instantiation_function'] = {'type': 'PREC_DYNAMIC', 'value': 2, 'content': seq(
    {'type': 'FIELD', 'name': 'declarator', 'content': choice(
        symbol('identifier'), symbol('qualified_identifier'), symbol('template_function'))},
    function_tail)}
rules['template_instantiation']['members'][2]['content'] = choice(
    alias_rule('_instantiation_function', 'function_declarator'), symbol('_declarator'))
for conflict in [
    ['_declarator', 'type_specifier', '_instantiation_function'],
    ['_declarator', '_instantiation_function'],
]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)

# SDKs conditionally compile an if/else prefix while keeping its final body shared.
# Require the exact else-before-endif boundary; ordinary dangling else stays invalid.
grammar['conflicts'] = [c for c in grammar['conflicts']
    if c not in [['preproc_if', 'conditional_if_statement'], ['preproc_ifdef', 'conditional_if_statement']]]
headers = []
for name in ['preproc_if', 'preproc_ifdef']:
    parts = json.loads(json.dumps(rules[name]['content']['members']))
    stop = next(i for i, part in enumerate(parts) if part == repeat(symbol('_block_item')))
    header = parts[:stop]
    for part in header:
        if part.get('type') == 'FIELD':
            part['name'] = 'preproc_condition'
    headers.append(seq(*header))
endif = json.loads(json.dumps(rules['preproc_if']['content']['members'][-1]))
prefix = json.loads(json.dumps(rules['if_statement']['content']['members'][:-1]))
rules['_conditional_else_clause'] = seq({'type': 'STRING', 'value': 'else'}, endif, symbol('statement'))
rules['conditional_if_statement'] = {'type': 'PREC_DYNAMIC', 'value': -1, 'content': seq(
    choice(*headers), *prefix,
    {'type': 'FIELD', 'name': 'alternative', 'content': alias_rule('_conditional_else_clause', 'else_clause')})}
if symbol('conditional_if_statement') not in rules['statement']['members']:
    rules['statement']['members'].append(symbol('conditional_if_statement'))
# Two complete guarded groups can select alternative if-open and if-close
# branches around a shared body. Every group must contain both alternatives;
# each opener has its own runtime condition and each closer a complete else.
# Keep guard nodes visibly preproc_* so macro evidence treats them conservatively.
else_token = json.loads(json.dumps(rules['preproc_else']['content']['members'][0]))
open_branch = seq(repeat(choice(symbol('declaration'), symbol('expression_statement'))),
    *json.loads(json.dumps(rules['if_statement']['content']['members'][:3])),
    {'type': 'STRING', 'value': '{'})
close_branch = seq(repeat(symbol('expression_statement')),
    {'type': 'STRING', 'value': '}'}, symbol('else_clause'))
for name, branch in [('preproc_split_if_open', open_branch),
                     ('preproc_split_if_close', close_branch)]:
    rules[name] = seq(choice(*headers),
        {'type': 'FIELD', 'name': 'first_branch', 'content': branch}, else_token,
        {'type': 'FIELD', 'name': 'second_branch', 'content': branch}, endif)
rules['conditional_split_if_statement'] = {'type': 'PREC_DYNAMIC', 'value': -1,
    'content': seq(
        {'type': 'FIELD', 'name': 'open', 'content': symbol('preproc_split_if_open')},
        repeat(symbol('_block_item')),
        {'type': 'FIELD', 'name': 'close', 'content': symbol('preproc_split_if_close')})}
if symbol('conditional_split_if_statement') not in rules['statement']['members']:
    rules['statement']['members'].append(symbol('conditional_split_if_statement'))
for conflict in [['_block_item', 'preproc_split_if_open'],
                 ['statement', 'preproc_split_if_open'],
                 ['statement', 'preproc_split_if_open', 'preproc_split_if_close']]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)
# A convention after a pointer star must precede a directly named function.
# Recursive declarators here wrongly reinterpret ordinary parameter parentheses.
rules['_sdk_pointer_function'] = seq(
    {'type': 'FIELD', 'name': 'declarator', 'content': symbol('identifier')},
    symbol('_function_declarator_seq'))
original = rules['pointer_declarator']['members'][0] if rules['pointer_declarator']['type'] == 'CHOICE' else rules['pointer_declarator']
convention = json.loads(json.dumps(original))
convention['value'] = -1
members = convention['content']['content']['members']
members.insert(-1, choice(symbol('ms_call_modifier'), symbol('sdk_call_modifier')))
members[-1]['content'] = alias_rule('_sdk_pointer_function', 'function_declarator')
rules['pointer_declarator'] = choice(original, convention)
# A directly named pointer-return function also needs an annotation-free type
# path. Otherwise `Word (&buffer)[size]` can be consumed as an SDK annotation
# followed by an invented missing type. Preserve the SDK path in parallel and
# keep this alternative out of general declarators and expression contexts.
plain_pointer_function = json.loads(json.dumps(rules['_instantiation_function']))
plain_pointer_function['content']['members'][0]['content'] = symbol('identifier')
rules['_plain_pointer_function'] = plain_pointer_function
plain_pointer = json.loads(json.dumps(original))
plain_pointer['content']['content']['members'][-1]['content'] = alias_rule(
    '_plain_pointer_function', 'function_declarator')
rules['pointer_declarator']['members'].append(plain_pointer)
for conflict in [
    ['_declarator', 'expression', '_plain_pointer_function'],
    ['_plain_pointer_function'],
    ['_declarator', '_plain_pointer_function'],
]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)
# A later array-reference parameter can otherwise be swallowed as an annotation
# after an earlier pointer parameter. Require the explicit named array reference
# in this parallel path; unrestricted plain function paths alter Catch2 recovery.
rules['_required_array_reference'] = seq(
    choice({'type': 'STRING', 'value': '&'}, {'type': 'STRING', 'value': '&&'}),
    symbol('identifier'))
rules['_required_parenthesized_array_reference'] = seq(
    {'type': 'STRING', 'value': '('},
    alias_rule('_required_array_reference', 'reference_declarator'),
    {'type': 'STRING', 'value': ')'})
reference_array = json.loads(json.dumps(rules['array_declarator']))
reference_array['content']['members'][0]['content'] = alias_rule(
    '_required_parenthesized_array_reference', 'parenthesized_declarator')
rules['_required_reference_array'] = reference_array
rules['_required_array_parameter'] = seq(symbol('_declaration_specifiers'),
    {'type': 'FIELD', 'name': 'declarator', 'content': alias_rule(
        '_required_reference_array', 'array_declarator')},
    repeat(symbol('attribute_specifier')))
plain_parameter = rules['_instantiation_parameter_list']['members'][1]['members'][0]['members'][0]
non_ellipsis_parameter = choice(*[member for member in plain_parameter['members']
    if member != {'type': 'STRING', 'value': '...'}])
comma = {'type': 'STRING', 'value': ','}
rules['_required_array_parameter_list'] = seq(
    {'type': 'STRING', 'value': '('}, repeat(seq(non_ellipsis_parameter, comma)),
    alias_rule('_required_array_parameter', 'parameter_declaration'),
    repeat(seq(comma, non_ellipsis_parameter)),
    optional(seq(comma, {'type': 'STRING', 'value': '...'})),
    {'type': 'STRING', 'value': ')'})
array_function = json.loads(json.dumps(plain_pointer_function))
array_function['content']['members'][1]['members'][0]['content'] = alias_rule(
    '_required_array_parameter_list', 'parameter_list')
rules['_required_array_function'] = array_function
array_function_alias = alias_rule('_required_array_function', 'function_declarator')
def array_function_context(node):
    if isinstance(node, list):
        for value in node: array_function_context(value)
    elif isinstance(node, dict):
        if node.get('type') == 'FIELD' and node.get('name') == 'declarator':
            if node['content'] == symbol('_declarator'):
                node['content'] = choice(symbol('_declarator'), array_function_alias)
            elif node['content'].get('type') == 'CHOICE':
                if array_function_alias not in node['content']['members']:
                    node['content']['members'].append(array_function_alias)
        else:
            for value in node.values(): array_function_context(value)
array_function_context(rules['function_definition'])
array_function_context(rules['declaration'])
for conflict in [
    ['_declarator', 'sdk_call_modifier', '_required_array_function'],
    ['_declarator', '_required_array_function'],
    ['_declarator', '_required_array_reference'],
]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)
# Native DLL declarations require both an explicit __declspec modifier and a
# native calling-convention keyword. Keep this separate from general declaration
# modifiers: widening those changes Catch2's recovery and ordinary declarations.
rules['_native_dll_declaration'] = {'type': 'PREC_DYNAMIC', 'value': -1, 'content': seq(
    optional(symbol('sdk_call_modifier')), symbol('ms_declspec_modifier'),
    {'type': 'FIELD', 'name': 'type', 'content': symbol('primitive_type')},
    symbol('ms_call_modifier'),
    {'type': 'FIELD', 'name': 'declarator', 'content': choice(
        alias_rule('_plain_pointer_function', 'function_declarator'),
        alias_rule('_sdk_pointer_function', 'function_declarator'))},
    {'type': 'STRING', 'value': ';'})}
native_dll = alias_rule('_native_dll_declaration', 'declaration')
for name in ['_top_level_item', '_block_item']:
    if native_dll not in rules[name]['members']:
        rules[name]['members'].append(native_dll)
for conflict in [
    ['_declaration_modifiers', '_native_dll_declaration'],
    ['parameter_list', '_instantiation_parameter_list'],
    ['parameter_declaration', '_instantiation_parameter_declaration'],
    ['variadic_parameter_declaration', '_instantiation_variadic_parameter_declaration'],
    ['optional_parameter_declaration', '_instantiation_optional_parameter_declaration'],
]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)
# A guarded storage modifier can split a static/extern variable declaration.
# Keep the complete guard in this explicit declaration context; do not admit
# preprocessor groups as general declaration modifiers or select a branch.
storage_modifiers = choice(symbol('storage_class_specifier'), symbol('type_qualifier'),
    symbol('ms_declspec_modifier'), symbol('sdk_call_modifier'))
rules['conditional_storage_modifier'] = {'type': 'PREC_DYNAMIC', 'value': -1, 'content': seq(
    choice(*headers), {'type': 'REPEAT1', 'content': storage_modifiers}, endif)}
storage_prefix = {'type': 'ALIAS', 'content': choice(
    {'type': 'STRING', 'value': 'static'}, {'type': 'STRING', 'value': 'extern'}),
    'named': True, 'value': 'storage_class_specifier'}
rules['_conditional_storage_declaration'] = {'type': 'PREC_DYNAMIC', 'value': -1, 'content': seq(
    storage_prefix, symbol('conditional_storage_modifier'), *rules['declaration']['members'])}
guarded_storage = alias_rule('_conditional_storage_declaration', 'declaration')
for name in ['_top_level_item', '_block_item']:
    if guarded_storage not in rules[name]['members']:
        rules[name]['members'].append(guarded_storage)
# C-compatible SDK headers guard linkage braces independently of their contents.
# Require complete guarded opening/closing groups, including nested guards.
rules['conditional_linkage_open'] = seq(choice(*headers), choice(
    seq({'type': 'STRING', 'value': 'extern'},
        {'type': 'FIELD', 'name': 'value', 'content': symbol('string_literal')},
        {'type': 'STRING', 'value': '{'}),
    symbol('conditional_linkage_open')), endif)
rules['conditional_linkage_close'] = seq(choice(*headers), choice(
    {'type': 'STRING', 'value': '}'}, symbol('conditional_linkage_close')), endif)
rules['conditional_linkage_specification'] = {'type': 'PREC_DYNAMIC', 'value': -1, 'content': seq(
    symbol('conditional_linkage_open'), repeat(symbol('_top_level_item')),
    symbol('conditional_linkage_close'))}
for name in ['_top_level_item', '_block_item']:
    if symbol('conditional_linkage_specification') not in rules[name]['members']:
        rules[name]['members'].append(symbol('conditional_linkage_specification'))
# Block comments may separate replacement-list fragments across continued lines.
# Keep each original preproc_arg and comment span rather than rewriting source.
def macro_fragments(node):
    if node == {'type': 'REPEAT1', 'content': symbol('preproc_arg')}:
        return node
    if node == symbol('preproc_arg'):
        return {'type': 'REPEAT1', 'content': symbol('preproc_arg')}
    return {key: ([macro_fragments(c) if isinstance(c, dict) else c for c in value]
                  if isinstance(value, list) else
                  macro_fragments(value) if isinstance(value, dict) else value)
            for key, value in node.items()}
for name in ['preproc_def', 'preproc_function_def']:
    rules[name] = macro_fragments(rules[name])
# decltype is a valid class base in this explicit type context.
def decltype_base(node):
    if node == symbol('_class_name'):
        return choice(symbol('_class_name'), symbol('decltype'))
    if node == choice(symbol('_class_name'), symbol('decltype')):
        return node
    return {key: ([decltype_base(c) if isinstance(c, dict) else c for c in value]
                  if isinstance(value, list) else
                  decltype_base(value) if isinstance(value, dict) else value)
            for key, value in node.items()}
rules['base_class_clause'] = decltype_base(rules['base_class_clause'])
# decltype takes a complete unevaluated expression, including a comma expression.
# Keep this choice local instead of widening general expression rules.
rules['decltype']['members'][2] = choice(symbol('expression'), symbol('comma_expression'))
# Proof-backed statement names come only from a scoped external scanner context.
# Without that context the new branch is unreachable, including ordinary calls.
external = symbol('_proven_statement_macro')
if external not in grammar['externals']:
    grammar['externals'].append(external)
rules['macro_statement'] = {'type': 'PREC_RIGHT', 'value': 1, 'content': seq(
    {'type': 'FIELD', 'name': 'name', 'content': alias_rule('_proven_statement_macro', 'identifier')},
    {'type': 'FIELD', 'name': 'arguments', 'content': symbol('argument_list')},
    optional({'type': 'STRING', 'value': ';'}))}
if symbol('macro_statement') not in rules['statement']['members']:
    rules['statement']['members'].append(symbol('macro_statement'))
# The audited native cpuid helper uses a brace-delimited MSVC assembly block.
# Admit its mov/cpuid instruction forms explicitly, not an opaque body token.
# Operands and commas cannot cross lines: otherwise an incomplete mov could
# silently absorb the following instruction as its missing operand.
rules['_ms_asm_newline'] = {'type': 'TOKEN', 'content': {'type': 'PREC', 'value': 1,
    'content': {'type': 'PATTERN', 'value': r'\r?\n'}}}
rules['_ms_asm_space'] = {'type': 'IMMEDIATE_TOKEN',
    'content': {'type': 'PATTERN', 'value': r'[ \t]+'}}
rules['_ms_asm_identifier'] = {'type': 'ALIAS', 'named': True, 'value': 'identifier',
    'content': {'type': 'IMMEDIATE_TOKEN',
                'content': {'type': 'PATTERN', 'value': '[A-Za-z_][A-Za-z_0-9]*'}}}
asm_number = {'type': 'ALIAS', 'named': True, 'value': 'number_literal',
    'content': {'type': 'IMMEDIATE_TOKEN',
                'content': {'type': 'PATTERN', 'value': '[0-9]+'}}}
rules['_ms_asm_operand'] = choice(symbol('_ms_asm_identifier'), asm_number)
rules['ms_asm_instruction'] = choice(seq(
    {'type': 'FIELD', 'name': 'opcode', 'content': {'type': 'STRING', 'value': 'mov'}},
    symbol('_ms_asm_space'),
    {'type': 'FIELD', 'name': 'destination', 'content': symbol('_ms_asm_identifier')},
    optional(symbol('_ms_asm_space')),
    {'type': 'IMMEDIATE_TOKEN', 'content': {'type': 'STRING', 'value': ','}},
    optional(symbol('_ms_asm_space')),
    {'type': 'FIELD', 'name': 'source', 'content': symbol('_ms_asm_operand')}),
    {'type': 'FIELD', 'name': 'opcode', 'content': {'type': 'STRING', 'value': 'cpuid'}})
rules['ms_asm_statement'] = seq({'type': 'STRING', 'value': '__asm'},
    {'type': 'STRING', 'value': '{'},
    repeat(choice(symbol('_ms_asm_newline'),
                  seq(symbol('ms_asm_instruction'), symbol('_ms_asm_newline')))),
    optional(symbol('ms_asm_instruction')), {'type': 'STRING', 'value': '}'})
if symbol('ms_asm_statement') not in rules['statement']['members']:
    rules['statement']['members'].append(symbol('ms_asm_statement'))
# Explicit operator member calls use '.' or '->'; do not admit operator names
# as pointer-to-member values after '.*'/'->*'. Existing template methods remain.
if rules['field_expression']['type'] == 'SEQ':
    ordinary_field = rules['field_expression']
    operator_field = json.loads(json.dumps(ordinary_field))
    operator_field['members'][0]['content']['members'][1]['content'] = choice(
        {'type': 'STRING', 'value': '.'}, {'type': 'STRING', 'value': '->'})
    operator_field['members'][1]['content'] = symbol('operator_name')
    rules['field_expression'] = choice(ordinary_field, operator_field)
conflict = ['field_expression', 'template_method']
if conflict not in grammar['conflicts']:
    grammar['conflicts'].append(conflict)


# Objective-C++ message syntax is local to the positive arm of an explicit
# #ifdef __OBJC__ group. Ordinary C++ expression rules and the external scanner
# are unchanged; the else/elif arms outside an enclosing ObjC scope stay C++.
def objc_copy(node, replacements):
    if isinstance(node, list):
        return [objc_copy(value, replacements) for value in node]
    if isinstance(node, dict):
        if node.get('type') == 'SYMBOL' and node.get('name') in replacements:
            return json.loads(json.dumps(replacements[node['name']]))
        return {key: objc_copy(value, replacements) for key, value in node.items()}
    return node

def objc_field(name, content): return {'type': 'FIELD', 'name': name, 'content': content}
def objc_string(value): return {'type': 'STRING', 'value': value}
objc_guard = alias_rule('_explicit_objc_guard', 'preproc_ifdef')
# On a second run do not copy our global entry point back into local bodies.
objc_base_items = json.loads(json.dumps(rules['_block_item']))
objc_base_items['members'] = [member for member in objc_base_items['members'] if member != objc_guard]
objc_expression = symbol('objc_message_expression')
rules['objc_message_expression'] = seq(objc_string('['),
    objc_field('receiver', choice(symbol('expression'), objc_expression)),
    choice(objc_field('selector', symbol('identifier')),
        {'type': 'REPEAT1', 'content': seq(objc_field('selector', symbol('identifier')),
            objc_string(':'), objc_field('argument', choice(symbol('expression'), objc_expression)))}),
    objc_string(']'))
for name in ['expression_statement', 'return_statement', 'condition_clause']:
    rules['_objc_' + name] = objc_copy(rules[name], {
        'expression': choice(symbol('expression'), objc_expression)})
rules['_objc_if_statement'] = objc_copy(rules['if_statement'], {
    'condition_clause': alias_rule('_objc_condition_clause', 'condition_clause'),
    'statement': symbol('_objc_statement'), 'else_clause': alias_rule('_objc_else_clause', 'else_clause')})
rules['_objc_else_clause'] = objc_copy(rules['else_clause'], {'statement': symbol('_objc_statement')})
rules['_objc_compound_statement'] = objc_copy(rules['compound_statement'], {'_block_item': symbol('_objc_body_item')})
rules['_objc_non_case_statement'] = objc_copy(rules['_non_case_statement'], {
    name: alias_rule('_objc_' + name, name) for name in
    ['if_statement', 'compound_statement', 'return_statement', 'expression_statement']})
rules['_objc_statement'] = objc_copy(rules['statement'], {'_non_case_statement': symbol('_objc_non_case_statement')})
rules['_objc_body_item'] = objc_copy(objc_base_items, {'statement': symbol('_objc_statement')})
rules['_objc_function_definition'] = objc_copy(rules['function_definition'], {
    'compound_statement': alias_rule('_objc_compound_statement', 'compound_statement')})
objc_preproc = {name: alias_rule('_objc_' + name, name) for name in
    ['preproc_if', 'preproc_ifdef', 'preproc_else', 'preproc_elif', 'preproc_elifdef']}
for name in objc_preproc:
    rules['_objc_' + name] = objc_copy(rules[name], {'_block_item': symbol('_objc_block_item'), **objc_preproc})
rules['_objc_block_item'] = objc_copy(objc_base_items, {
    'function_definition': alias_rule('_objc_function_definition', 'function_definition'),
    'statement': symbol('_objc_statement'), **objc_preproc})
objc_ifdef = rules['preproc_ifdef']['content']['members']
rules['_explicit_objc_guard'] = seq(
    json.loads(json.dumps(objc_ifdef[0]['members'][0])),
    objc_field('name', {'type': 'ALIAS', 'content': objc_string('__OBJC__'), 'named': True, 'value': 'identifier'}),
    objc_string(chr(10)), repeat(symbol('_objc_block_item')),
    objc_field('alternative', choice(symbol('preproc_else'), symbol('preproc_elif'), symbol('preproc_elifdef'), {'type': 'BLANK'})),
    json.loads(json.dumps(objc_ifdef[-1])))
for name in ['_top_level_item', '_block_item']:
    if objc_guard not in rules[name]['members']:
        rules[name]['members'].append(objc_guard)
for conflict in [['_objc_non_case_statement', '_objc_block_item'],
                 ['_objc_non_case_statement', '_objc_body_item'],
                 ['expression_statement', '_objc_expression_statement'],
                 ['condition_clause', '_objc_condition_clause'],
                 ['_objc_block_item', 'preproc_split_if_open']]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)

path.write_bytes((json.dumps(grammar, indent=2) + '\n').encode('utf-8'))
scanner = path.parent / 'scanner.c'
scanner.write_bytes((Path(__file__).parent / 'cpp_statement_macro_scanner.c').read_bytes())

if '--finalize' in sys.argv:
    # The generator replaces parser.h; retain Vorpal's existing ASCII lexer fast path.
    header = path.parent / 'tree_sitter/parser.h'
    source = header.read_text(encoding='utf-8')
    marker = 'static inline bool set_contains(const TSCharacterRange *ranges, uint32_t len, int32_t lookahead) {\n'
    fast_path = '''  // vorpal: preserve the ASCII fast path after grammar regeneration.
  if (lookahead < 0x80) {
    for (uint32_t i = 0; i < len; i++) {
      if (lookahead < ranges[i].start) return false;
      if (lookahead <= ranges[i].end) return true;
    }
    return false;
  }
'''
    if fast_path not in source:
        assert marker in source
        source = source.replace(marker, marker + fast_path, 1)
        header.write_bytes(source.encode('utf-8'))
