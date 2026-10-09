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
# Remove previous return-only additions before deriving dialect returns. The
# ordinary C++ addition below must not migrate into an Objective-C body clone.
return_rule = rules['return_statement']
if return_rule.get('type') == 'CHOICE' and any(
        member.get('type') == 'SEQ' and any(
            part.get('type') == 'SYMBOL' and part.get('name') in
            ['conditional_logical_expression', 'conditional_return_expression']
            for part in member['members']) for member in return_rule['members']):
    rules['return_statement'] = return_rule['members'][0]
for name in list(rules):
    if (name.startswith(('_conditional_logical_', '_conditional_return_')) or name in
            ['conditional_logical_expression', 'conditional_return_expression']):
        del rules[name]
# Rebuild conditional declaration scopes from the ordinary entries. They must
# never leak into a function's block context on a later reproduction run.
conditional_rule = lambda name: name.startswith(('conditional_function_', 'conditional_decl_')) or name == '_conditional_decl_item'
for entry in ['_top_level_item', '_block_item']:
    rules[entry]['members'] = [member for member in rules[entry]['members']
        if not conditional_rule(member.get('name', member.get('content', {}).get('name', '')))]
grammar['conflicts'] = [conflict for conflict in grammar['conflicts']
    if not any(conditional_rule(name) for name in conflict)]
for name in list(rules):
    if conditional_rule(name): del rules[name]

# Reproduction starts from the ordinary C++ entries: remove the prior managed
# guard alternatives before deriving any dialect containers below.
for entry in ['_top_level_item', '_block_item']:
    rules[entry]['members'] = [member for member in rules[entry]['members']
        if not (member.get('type') == 'ALIAS' and
            member.get('content', {}).get('name') in
            ['_explicit_cli_if_guard', '_explicit_cli_ifdef_guard',
             'explicit_cli_if_guard', 'explicit_cli_ifdef_guard'])]
for name in list(rules):
    if name.startswith(('_cli', 'cli_', '_explicit_cli_', 'explicit_cli_')):
        del rules[name]
def symbol(name): return {'type': 'SYMBOL', 'name': name}
def choice(*members): return {'type': 'CHOICE', 'members': list(members)}
def seq(*members): return {'type': 'SEQ', 'members': list(members)}
def repeat(content): return {'type': 'REPEAT', 'content': content}
def optional(content): return choice(content, {'type': 'BLANK'})

# C++ permits inline before friend inside a class. Keep the entire friend
# declaration intact rather than treating the keyword as a return type. Reuse
# the original alternatives and unwrap our previous copy on reproduction.
friend = rules['friend_declaration']
if friend.get('type') == 'CHOICE' and any(
        member.get('type') == 'SEQ' and member.get('members', [None])[0] ==
        {'type': 'STRING', 'value': 'inline'} for member in friend['members']):
    friend = friend['members'][0]
rules['friend_declaration'] = choice(friend, seq({'type': 'STRING', 'value': 'inline'}, friend))
conflict = ['storage_class_specifier', 'friend_declaration']
if conflict not in grammar['conflicts']: grammar['conflicts'].append(conflict)

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
# A literal language linkage can precede a native DLL prototype. Keep this
# whole declaration alternative shallow: no recursive declarator/function
# extension and no general calling-convention declaration modifier.
rules['_native_linkage_dll_declaration'] = {'type': 'PREC_DYNAMIC', 'value': 1, 'content': seq(
    symbol('ms_declspec_modifier'),
    {'type': 'FIELD', 'name': 'type', 'content': symbol('primitive_type')},
    symbol('ms_call_modifier'),
    {'type': 'FIELD', 'name': 'declarator', 'content':
        alias_rule('_sdk_pointer_function', 'function_declarator')},
    {'type': 'STRING', 'value': ';'})}
rules['_native_dll_linkage'] = {'type': 'PREC', 'value': 1, 'content': seq(
    {'type': 'STRING', 'value': 'extern'},
    {'type': 'FIELD', 'name': 'value', 'content': symbol('string_literal')},
    {'type': 'FIELD', 'name': 'body', 'content':
        alias_rule('_native_linkage_dll_declaration', 'declaration')})}
for name in ['_top_level_item', '_block_item']:
    native_linkage = alias_rule('_native_dll_linkage', 'linkage_specification')
    # Normalize its position before deriving the dialect/container copies.
    rules[name]['members'] = [member for member in rules[name]['members'] if member != native_linkage]
    rules[name]['members'].append(native_linkage)
for conflict in [
    ['_declaration_modifiers', '_native_linkage_dll_declaration'],
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
for name in ['_proven_statement_macro', '_proven_open_if_macro']:
    external = symbol(name)
    if external not in grammar['externals']:
        grammar['externals'].append(external)
closed_macro_statement = {'type': 'PREC_RIGHT', 'value': 1, 'content': seq(
    {'type': 'FIELD', 'name': 'name', 'content': alias_rule('_proven_statement_macro', 'identifier')},
    {'type': 'FIELD', 'name': 'arguments', 'content': symbol('argument_list')},
    optional({'type': 'STRING', 'value': ';'}))}
# An independently proven dangling if can consume else. A source semicolon
# terminates that attachment and remains a separate empty statement.
open_if_macro_statement = {'type': 'PREC_RIGHT', 'value': 1, 'content': seq(
    {'type': 'FIELD', 'name': 'name', 'content': alias_rule('_proven_open_if_macro', 'identifier')},
    {'type': 'FIELD', 'name': 'arguments', 'content': symbol('argument_list')},
    optional({'type': 'FIELD', 'name': 'alternative', 'content': symbol('else_clause')}))}
rules['macro_statement'] = choice(closed_macro_statement, open_if_macro_statement)
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


# Unqualified explicit operator calls are a separate low-precedence alternative.
# Keep ordinary function declarators viable (including alternative-token corpus
# cases); do not promote operator_name to a general expression or lexer token.
unqualified_operator_call = {'type': 'PREC_DYNAMIC', 'value': -1, 'content':
    {'type': 'PREC', 'value': 0, 'content': seq(
        {'type': 'FIELD', 'name': 'function', 'content': symbol('operator_name')},
        {'type': 'FIELD', 'name': 'arguments', 'content': symbol('argument_list')})}}
if unqualified_operator_call not in rules['call_expression']['members']:
    rules['call_expression']['members'].append(unqualified_operator_call)
for conflict in [['_declarator', 'call_expression'],
                 ['_declarator', 'call_expression', '_objc_call_expression']]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)

# Objective-C++ message syntax is local to the positive arm of an explicit
# #ifdef __OBJC__ group. Ordinary C++ expression rules and the external scanner
# are unchanged; ordinary else/elif arms stay C++. A separate exact negative
# guard below admits its complete else arm with declaration/body boundaries.
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
objc_base_items['members'] = [member for member in objc_base_items['members']
    if member not in [objc_guard, alias_rule('_explicit_objc_inverse_body_guard', 'preproc_ifdef')]]
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

# Objective-C exceptions and message-bearing C++ calls remain in that same
# explicit scope. Reuse existing C++ call/argument/try/catch kinds through aliases.
rules['_objc_argument_list'] = objc_copy(rules['argument_list'], {
    'expression': choice(symbol('expression'), objc_expression, alias_rule('_objc_call_expression', 'call_expression'))})
rules['_objc_call_expression'] = objc_copy(rules['call_expression'], {
    'argument_list': alias_rule('_objc_argument_list', 'argument_list')})
for name in ['expression_statement', 'return_statement', 'condition_clause']:
    rules['_objc_' + name] = objc_copy(rules[name], {
        'expression': choice(symbol('expression'), objc_expression, alias_rule('_objc_call_expression', 'call_expression'))})
rules['_objc_exception_statement'] = objc_copy(rules['try_statement'], {
    'compound_statement': alias_rule('_objc_compound_statement', 'compound_statement'),
    'catch_clause': alias_rule('_objc_catch_clause', 'catch_clause')})
# Objective-C catch must contain a declaration or an ellipsis; an empty
# ordinary C++ parameter list must not make @catch () appear clean.
rules['_objc_catch_parameters'] = json.loads(json.dumps(rules['parameter_list']))
catch_parameters = rules['_objc_catch_parameters']['members'][1]
assert catch_parameters['type'] == 'CHOICE' and catch_parameters['members'][1]['type'] == 'BLANK'
rules['_objc_catch_parameters']['members'][1] = catch_parameters['members'][0]
rules['_objc_catch_clause'] = objc_copy(rules['catch_clause'], {
    'parameter_list': alias_rule('_objc_catch_parameters', 'parameter_list'),
    'compound_statement': alias_rule('_objc_compound_statement', 'compound_statement')})
def objc_keyword(node, old, new):
    if isinstance(node, list): return [objc_keyword(value, old, new) for value in node]
    if isinstance(node, dict):
        if node.get('type') == 'STRING' and node.get('value') == old: return objc_string(new)
        return {key: objc_keyword(value, old, new) for key, value in node.items()}
    return node
rules['_objc_exception_statement'] = objc_keyword(rules['_objc_exception_statement'], 'try', '@try')
rules['_objc_catch_clause'] = objc_keyword(rules['_objc_catch_clause'], 'catch', '@catch')
rules['_objc_non_case_statement']['members'].append(alias_rule('_objc_exception_statement', 'try_statement'))
for conflict in [['type_specifier', 'call_expression', '_objc_call_expression'],
                 ['argument_list', '_objc_argument_list']]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)

# Carry the explicit dialect context through namespaces, templates and class
# bodies. Copy only local rules; global C++ declarations/expressions stay intact.
objc_named = ['class_specifier', 'struct_specifier', 'union_specifier',
    'field_declaration_list', 'declaration', 'template_declaration',
    'namespace_definition', 'declaration_list']
objc_hidden = ['_class_declaration', '_class_declaration_item',
    '_field_declaration_list_item', '_declaration_specifiers', 'type_specifier',
    '_empty_declaration']
objc_class_rules = {name: alias_rule('_objc_' + name, name) for name in objc_named}
objc_class_rules.update({name: symbol('_objc_' + name.lstrip('_')) for name in objc_hidden})
objc_class_rules['function_definition'] = alias_rule('_objc_function_definition', 'function_definition')
for name in ['inline_method_definition', 'constructor_or_destructor_definition', 'operator_cast_definition']:
    rules['_objc_' + name] = objc_copy(rules[name], {
        'compound_statement': alias_rule('_objc_compound_statement', 'compound_statement')})
    objc_class_rules[name] = symbol('_objc_' + name)
objc_class_rules['_block_item'] = symbol('_objc_block_item')
for name in objc_named + objc_hidden:
    rules['_objc_' + name.lstrip('_')] = objc_copy(rules[name], objc_class_rules)
rules['_objc_block_item'] = objc_copy(rules['_objc_block_item'], objc_class_rules)
rules['_objc_body_item'] = objc_copy(rules['_objc_body_item'], {
    '_empty_declaration': symbol('_objc_empty_declaration')})

# Binary operators retain the ordinary precedence/fields while allowing message
# operands locally. Calls and argument lists use the same structural expression.
objc_local_expression = choice(symbol('expression'), objc_expression,
    alias_rule('_objc_call_expression', 'call_expression'),
    alias_rule('_objc_binary_expression', 'binary_expression'))
rules['_objc_binary_expression'] = objc_copy(rules['binary_expression'], {
    'expression': objc_local_expression})
for name in ['expression_statement', 'return_statement', 'condition_clause', 'argument_list']:
    rules['_objc_' + name] = objc_copy(rules[name], {'expression': objc_local_expression})
rules['objc_message_expression'] = objc_copy(rules['objc_message_expression'], {
    'expression': objc_local_expression})
for conflict in [['type_specifier', 'call_expression', '_objc_call_expression', '_objc_type_specifier'],
 ['type_specifier', '_objc_type_specifier'],
 ['_declarator', 'type_specifier', 'expression', '_objc_type_specifier'],
 ['type_specifier', 'expression', '_objc_type_specifier'],
 ['_declarator', 'type_specifier', '_objc_type_specifier'],
 ['type_specifier', 'sdk_call_modifier', '_objc_type_specifier'],
 ['_class_declaration_item', '_objc_class_declaration_item'],
 ['_field_declaration_list_item', '_objc_field_declaration_list_item'],
 ['field_declaration_list', '_objc_field_declaration_list'],
 ['binary_expression', '_objc_binary_expression'],
 ['constructor_or_destructor_definition', '_objc_constructor_or_destructor_definition'],
 ['operator_cast_definition', '_objc_operator_cast_definition'],
 ['template_declaration', '_objc_template_declaration'],
 ['inline_method_definition', '_objc_inline_method_definition'],
 ['compound_statement', '_objc_compound_statement'],
 ['_block_item', '_objc_body_item'],
 ['_block_item', 'statement', '_objc_non_case_statement', '_objc_body_item'],
 ['statement', '_objc_non_case_statement'],
 ['statement', '_objc_statement'],
 ['return_statement', '_objc_return_statement'],
 ['function_definition', '_objc_function_definition'],
 ['expression', '_objc_type_specifier']]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)

# Initializers inside supported Objective-C bodies/fields retain their ordinary
# declarator kinds. Keep namespace/type/template declarations unchanged: sharing
# the broader local type rule here regresses Catch2's root recovery.
rules['_objc_init_declarator'] = objc_copy(rules['init_declarator'], {
    'expression': objc_local_expression})
rules['_objc_body_declaration'] = objc_copy(rules['declaration'], {
    'init_declarator': alias_rule('_objc_init_declarator', 'init_declarator')})
rules['_objc_field_declaration'] = objc_copy(rules['field_declaration'], {
    'expression': objc_local_expression})
rules['_objc_body_item'] = objc_copy(rules['_objc_body_item'], {
    'declaration': alias_rule('_objc_body_declaration', 'declaration')})
rules['_objc_field_declaration_list_item'] = objc_copy(rules['_objc_field_declaration_list_item'], {
    'field_declaration': alias_rule('_objc_field_declaration', 'field_declaration')})
for conflict in [['field_declaration', '_objc_field_declaration'],
 ['declaration', '_objc_body_declaration'],
 ['init_declarator', '_objc_init_declarator']]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)

# Objective-C string constants use ordinary quoted contents but not C++ wide,
# UTF or raw-string prefixes. Keep their object-literal kind and original span.
objc_literal_expression = choice(*objc_local_expression['members'], symbol('objc_string_literal'))
def objc_literals(node):
    if node == objc_local_expression:
        return json.loads(json.dumps(objc_literal_expression))
    if isinstance(node, list): return [objc_literals(value) for value in node]
    if isinstance(node, dict): return {key: objc_literals(value) for key, value in node.items()}
    return node
for name in list(rules):
    if name.startswith('_objc_') or name == 'objc_message_expression':
        rules[name] = objc_literals(rules[name])
rules['_objc_plain_string_literal'] = json.loads(json.dumps(rules['string_literal']))
rules['_objc_plain_string_literal']['members'][0] = objc_string('"')
rules['objc_string_literal'] = seq(objc_string('@'),
    objc_field('value', alias_rule('_objc_plain_string_literal', 'string_literal')))
# Only keyword messages can carry additional comma-separated variadic arguments.
# Preserve each argument's span; the selector does not become a C++ call node.
objc_message_keywords = rules['objc_message_expression']['members'][2]['members'][1]
objc_message_argument = objc_message_keywords['content']['members'][2]['content']
rules['objc_message_expression']['members'][2]['members'][1] = seq(
    objc_message_keywords, repeat(seq(objc_string(','), objc_field('argument', objc_message_argument))))
# Complete protocol blocks prevent their method declarations and @end from
# swallowing following namespaces as a recovered C++ function. Split '@' from
# keyword tokens so the word lexer rejects prefixes such as @protocolFixture.
def objc_at(keyword): return seq(objc_string('@'), objc_string(keyword))
rules['objc_method_declaration'] = seq(choice(objc_string('-'), objc_string('+')),
    objc_string('('), objc_field('return_type', symbol('type_descriptor')), objc_string(')'),
    choice(objc_field('selector', symbol('identifier')), {'type': 'REPEAT1', 'content': seq(
        objc_field('selector', symbol('identifier')), objc_string(':'), objc_string('('),
        objc_field('parameter_type', symbol('type_descriptor')), objc_string(')'),
        objc_field('parameter', symbol('identifier')))}), objc_string(';'))
rules['objc_protocol_declaration'] = seq(objc_at('protocol'), objc_field('name', symbol('identifier')),
    repeat(choice(objc_at('optional'), objc_at('required'), symbol('objc_method_declaration'))), objc_at('end'))
rules['_objc_block_item']['members'].append(symbol('objc_protocol_declaration'))

# Selector metadata carries names rather than runtime argument expressions.
# Keep it in the guarded local expression context and split the keyword token
# to reject prefixes such as @selectorSuffix.
objc_selector_expression = choice(*objc_literal_expression['members'], symbol('objc_selector_expression'))

def objc_selectors(node):
    if node == objc_literal_expression:
        return json.loads(json.dumps(objc_selector_expression))
    if isinstance(node, list): return [objc_selectors(value) for value in node]
    if isinstance(node, dict): return {key: objc_selectors(value) for key, value in node.items()}
    return node
for name in list(rules):
    if name.startswith('_objc_') or name == 'objc_message_expression':
        rules[name] = objc_selectors(rules[name])
rules['objc_selector_expression'] = seq(objc_string('@'), objc_string('selector'), objc_string('('),
    choice(objc_field('selector', symbol('identifier')), {'type': 'REPEAT1', 'content': seq(
        objc_field('selector', symbol('identifier')), objc_string(':'))}), objc_string(')'))

# Admit only named dotted fields rooted in a message (or a prior dotted field).
# Ordinary C++ field/member/template expressions keep their existing grammar.
objc_message_field_expression = choice(*objc_selector_expression['members'],
    alias_rule('_objc_message_field_expression', 'field_expression'))

def objc_message_fields(node):
    if node == objc_selector_expression:
        return json.loads(json.dumps(objc_message_field_expression))
    if isinstance(node, list): return [objc_message_fields(value) for value in node]
    if isinstance(node, dict): return {key: objc_message_fields(value) for key, value in node.items()}
    return node
for name in list(rules):
    if name.startswith('_objc_') or name == 'objc_message_expression':
        rules[name] = objc_message_fields(rules[name])
rules['_objc_message_field_expression'] = seq({'type': 'PREC', 'value': 16, 'content': seq(
    objc_field('argument', choice(objc_expression, alias_rule('_objc_message_field_expression', 'field_expression'))),
    objc_field('operator', objc_string('.')))}, objc_field('field', symbol('_field_identifier')))

# Constructor member-initializer arguments share the same bounded dialect
# context. Retain ordinary named aliases; namespace/type declarations stay intact.
rules['_objc_field_initializer'] = objc_copy(rules['field_initializer'], {
    'argument_list': alias_rule('_objc_argument_list', 'argument_list')})
rules['_objc_field_initializer_list'] = objc_copy(rules['field_initializer_list'], {
    'field_initializer': alias_rule('_objc_field_initializer', 'field_initializer')})
rules['_objc_constructor_or_destructor_definition'] = objc_copy(
    rules['_objc_constructor_or_destructor_definition'], {
    'field_initializer_list': alias_rule('_objc_field_initializer_list', 'field_initializer_list')})
conflict = ['field_initializer', '_objc_field_initializer']
if conflict not in grammar['conflicts']:
    grammar['conflicts'].append(conflict)

# Declaration scopes admit guarded dialect declarations and function bodies,
# but do not acquire standalone Objective-C statements. Preserve the upstream
# C++ top-level fragment rules; keep dialect statements inside body contexts.
objc_top_preproc = {name: name.replace('_objc_preproc', '_objc_top_preproc')
    for name in rules if name.startswith('_objc_preproc')}
objc_top_replacements = {name: symbol(top) for name, top in objc_top_preproc.items()}
objc_top_replacements['_objc_block_item'] = symbol('_objc_top_level_item')
rules['_objc_top_level_item'] = objc_copy(rules['_objc_block_item'], objc_top_replacements)
rules['_objc_top_level_item']['members'] = [symbol('_top_level_statement')
    if member == symbol('_objc_statement') else member
    for member in rules['_objc_top_level_item']['members']]
for name, top in objc_top_preproc.items():
    rules[top] = objc_copy(rules[name], objc_top_replacements)
objc_body_preproc = {name: name.replace('_objc_preproc', '_objc_body_preproc')
    for name in objc_top_preproc}
objc_body_replacements = {name: symbol(body) for name, body in objc_body_preproc.items()}
objc_body_replacements['_objc_block_item'] = symbol('_objc_body_item')
for name, body in objc_body_preproc.items():
    rules[body] = objc_copy(rules[name], objc_body_replacements)
rules['_objc_body_item'] = objc_copy(rules['_objc_body_item'], {
    name.removeprefix('_objc_'): alias_rule(body, name.removeprefix('_objc_'))
    for name, body in objc_body_preproc.items()})
rules['_objc_declaration_list'] = objc_copy(rules['_objc_declaration_list'], {
    '_objc_block_item': symbol('_objc_top_level_item')})
rules['_explicit_objc_top_guard'] = objc_copy(rules['_explicit_objc_guard'], {
    '_objc_block_item': symbol('_objc_top_level_item')})
rules['_top_level_item'] = objc_copy(rules['_top_level_item'], {
    '_explicit_objc_guard': symbol('_explicit_objc_top_guard')})
objc_top_members = []
for member in rules['_top_level_item']['members']:
    if member not in objc_top_members:
        objc_top_members.append(member)
rules['_top_level_item']['members'] = objc_top_members
for conflict in [['type_specifier', 'call_expression', '_objc_type_specifier'],
                 ['_top_level_statement', '_objc_top_level_item']]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)

for conflict in [
    ['preproc_ifdef', '_objc_body_preproc_ifdef'],
    ['preproc_else', '_objc_body_preproc_else'],
    ['preproc_ifdef', 'preproc_ifdef_in_initializer_list', '_objc_body_preproc_ifdef'],
    ['preproc_else', 'preproc_else_in_initializer_list', '_objc_body_preproc_else'],
    ['preproc_if', '_objc_body_preproc_if'],
    ['preproc_elifdef', '_objc_body_preproc_elifdef'],
    ['preproc_if', 'preproc_if_in_initializer_list', '_objc_body_preproc_if'],
    ['preproc_elifdef', 'preproc_elifdef_in_initializer_list', '_objc_body_preproc_elifdef'],
    ['preproc_elif', '_objc_body_preproc_elif'],
    ['preproc_ifdef_in_initializer_list', '_objc_body_preproc_ifdef'],
    ['preproc_else_in_initializer_list', '_objc_body_preproc_else'],
    ['preproc_elif', 'preproc_elif_in_initializer_list', '_objc_body_preproc_elif'],
    ['preproc_if_in_initializer_list', '_objc_body_preproc_if'],
    ['preproc_elifdef_in_initializer_list', '_objc_body_preproc_elifdef'],
    ['preproc_elif_in_initializer_list', '_objc_body_preproc_elif'],
]:
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)

# An exact negative dialect guard can expose Objective-C only in its complete
# else arm. Keep the first arm ordinary C++ and distinguish declaration/body
# contexts. No elif branch or condition evaluation supplies this dialect context.
for context, ordinary, dialect, entry in [
    ('top', '_top_level_item', '_objc_top_level_item', '_top_level_item'),
    ('body', '_block_item', '_objc_body_item', '_block_item'),
]:
    alternative = json.loads(json.dumps(rules['preproc_else']))
    alternative['content']['members'][1]['content'] = symbol(dialect)
    alternative['content']['members'].insert(1, objc_string(chr(10)))
    alternative_name = '_objc_inverse_' + context + '_else'
    rules[alternative_name] = alternative
    members = json.loads(json.dumps(rules['_explicit_objc_top_guard']['members']))
    members[0] = json.loads(json.dumps(objc_ifdef[0]['members'][1]))
    members[3]['content'] = symbol(ordinary)
    members[4] = objc_field('alternative', alias_rule(alternative_name, 'preproc_else'))
    guard_name = '_explicit_objc_inverse_' + context + '_guard'
    rules[guard_name] = {'type': 'SEQ', 'members': members}
    guard_alias = alias_rule(guard_name, 'preproc_ifdef')
    if guard_alias not in rules[entry]['members']:
        rules[entry]['members'].append(guard_alias)

# C++/CLI handles are structural dialect syntax, scoped to an exact positive
# _MANAGED guard. Require a handle in each additional definition/template path;
# global declarators, ordinary XOR and outer else/elif arms remain unchanged.
def sym(name): return symbol(name)
def alias(name, kind): return alias_rule(name, kind)
def field(name, content): return {'type': 'FIELD', 'name': name, 'content': content}
def string(text): return {'type': 'STRING', 'value': text}
names=['_top_level_item','_block_item','namespace_definition','declaration_list','template_declaration','preproc_if','preproc_ifdef','preproc_else','preproc_elif','preproc_elifdef','compound_statement']
mapping={n:('_cli'+n if n.startswith('_') else '_cli_'+n) for n in names}
def cp(n,aliased=False):
 if isinstance(n,list):return [cp(x,aliased) for x in n]
 if not isinstance(n,dict):return n
 if n.get('type')=='ALIAS':return {k:cp(v,True) if k=='content' else v for k,v in n.items()}
 if n.get('type')=='SYMBOL' and n['name'] in mapping:
  name=n['name'];return sym(mapping[name]) if aliased or name.startswith('_') else alias(mapping[name],name)
 return {k:cp(v,aliased) for k,v in n.items()}
for n in names:rules[mapping[n]]=cp(rules[n])
rules['managed_handle_declarator']=seq(string('^'),repeat(sym('type_qualifier')),field('declarator',sym('identifier')))
rules['abstract_managed_handle_declarator']=seq(string('^'),repeat(sym('type_qualifier')))
rules['_cli_parameter']=seq(sym('_declaration_specifiers'),field('declarator',choice(sym('managed_handle_declarator'),sym('abstract_managed_handle_declarator'))))
ordinary_param=choice(sym('parameter_declaration'),sym('optional_parameter_declaration'),sym('variadic_parameter_declaration'));managed_param=alias('_cli_parameter','parameter_declaration');any_param=choice(ordinary_param,managed_param)
rules['_cli_parameters']=seq(string('('),optional(seq(any_param,repeat(seq(string(','),any_param)))),string(')'))
rules['_cli_required_parameters']=seq(string('('),repeat(seq(ordinary_param,string(','))),managed_param,repeat(seq(string(','),any_param)),string(')'))
rules['_cli_function']=seq(field('declarator',sym('identifier')),field('parameters',alias('_cli_parameters','parameter_list')))
rules['_cli_required_function']=seq(field('declarator',sym('identifier')),field('parameters',alias('_cli_required_parameters','parameter_list')))
rules['_cli_handle_function']=seq(string('^'),repeat(sym('type_qualifier')),field('declarator',alias('_cli_function','function_declarator')))
rules['_cli_definition']=seq(sym('_declaration_specifiers'),field('declarator',choice(alias('_cli_handle_function','managed_handle_declarator'),alias('_cli_required_function','function_declarator'))),field('body',alias(mapping['compound_statement'],'compound_statement')))
# Template calls require a handle in the type argument, not an ordinary callee clone.
rules['_cli_handle_type']=seq(field('type',sym('type_specifier')),field('declarator',sym('abstract_managed_handle_declarator')))
rules['_cli_template_type']=seq(field('name',{'type':'ALIAS','content':sym('identifier'),'named':True,'value':'type_identifier'}),field('arguments',alias('_cli_handle_arguments','template_argument_list')))
rules['_cli_handle_arguments']=seq(string('<'),alias('_cli_handle_type','type_descriptor'),string('>'))
rules['_cli_scope_name']=seq(optional(string('::')),repeat(seq(sym('_namespace_identifier'),string('::'))),field('scope',alias('_cli_template_type','template_type')),string('::'),field('name',sym('identifier')))
rules['_cli_call']=seq(field('function',alias('_cli_scope_name','qualified_identifier')),field('arguments',sym('argument_list')))
rules['_cli_return']=seq(string('return'),alias('_cli_call','call_expression'),string(';'))
# Only statement bodies get this additional return form.
def statement(n):
 if isinstance(n,list):return [statement(x) for x in n]
 if not isinstance(n,dict):return n
 if n==sym('statement'):return choice(n,alias('_cli_return','return_statement'))
 return {k:statement(v) for k,v in n.items()}
rules[mapping['_block_item']]=statement(rules[mapping['_block_item']])
for name in [mapping['_top_level_item'],mapping['_block_item']]:rules[name]['members'].append(alias('_cli_definition','function_definition'))
# Existing template alternatives stay ordinary; only the additional definition contains ^.
rules[mapping['template_declaration']]['members'][-1]['members'].append(alias('_cli_definition','function_definition'))
start=rules['preproc_if']['content']['members'][0];finish=rules['preproc_if']['content']['members'][-1];alternatives=optional(choice(sym('preproc_else'),sym('preproc_elif'),sym('preproc_elifdef')))
rules['_cli_defined']=seq(string('defined'),string('('),field('name',{'type':'ALIAS','content':string('_MANAGED'),'named':True,'value':'identifier'}),string(')'))
rules['_explicit_cli_if_guard']=seq(start,field('condition',alias('_cli_defined','preproc_defined')),string('\n'),repeat(sym(mapping['_top_level_item'])),field('alternative',alternatives),finish)
startdef=rules['preproc_ifdef']['content']['members'][0]['members'][0];rules['_explicit_cli_ifdef_guard']=seq(startdef,field('name',{'type':'ALIAS','content':string('_MANAGED'),'named':True,'value':'identifier'}),string('\n'),repeat(sym(mapping['_top_level_item'])),field('alternative',alternatives),finish)
for name in ['_top_level_item','_block_item']:
 for guard,kind in [('_explicit_cli_if_guard','preproc_if'),('_explicit_cli_ifdef_guard','preproc_ifdef')]:rules[name]['members'].append(alias(guard,kind))
rules['_cli_call']={'type':'PREC_DYNAMIC','value':1,'content':rules['_cli_call']}
# Preserve ordinary qualified_identifier nesting: each namespace is a scope,
# with the handle-bearing template scope at the final inner level.
rules['_cli_scope_name']=seq(field('scope',alias_rule('_cli_template_type','template_type')),string('::'),field('name',symbol('identifier')))
rules['_cli_qualified_name']=choice(rules['_cli_scope_name'],seq(symbol('_scope_resolution'),field('name',alias_rule('_cli_qualified_name','qualified_identifier'))))
rules['_cli_call']['content']['members'][0]['content']=alias_rule('_cli_qualified_name','qualified_identifier')
rules['_cli_field_list']=seq(string('{'),{'type':'REPEAT','content':choice(symbol('_field_declaration_list_item'),alias_rule('_cli_definition','function_definition'))},string('}'))
rules['_cli_struct_specialization']=seq(string('struct'),field('name',alias_rule('_cli_template_type','template_type')),field('body',alias_rule('_cli_field_list','field_declaration_list')))
rules['_cli_template_declaration']['members'][-1]['members'].append(seq(alias_rule('_cli_struct_specialization','struct_specifier'),string(';')))
# Aliased named nodes must be named source rules, not hidden rules: otherwise
# inherited child fields (template `name`) leak into the enclosing callee's
# field lookup even though its S-expression shows the correct terminal name.
managed_names = {name: name.removeprefix('_') for name in rules
    if name.startswith(('_cli', '_explicit_cli_')) and
    name not in ['_cli_top_level_item', '_cli_block_item']}
def managed_named(node):
    if isinstance(node, list): return [managed_named(child) for child in node]
    if not isinstance(node, dict): return node
    if node.get('type') == 'SYMBOL' and node['name'] in managed_names:
        return symbol(managed_names[node['name']])
    return {key: managed_named(value) for key, value in node.items()}
for name in list(rules):
    content = managed_named(rules[name])
    if name in managed_names: del rules[name]
    rules[managed_names.get(name, name)] = content
grammar['conflicts'] = [[managed_names.get(name, name) for name in conflict]
    for conflict in grammar['conflicts']]
for conflict in [['_top_level_statement', '_cli_top_level_item'], ['_declarator', 'sdk_call_modifier', '_required_array_function', '_cli_required_function'], ['statement', '_cli_block_item'], ['expression', 'template_type', 'template_function', '_cli_template_type'], ['preproc_split_if_open', '_cli_block_item'], ['template_type', 'template_function', 'qualified_identifier', '_cli_template_type'], ['_field_declarator', 'sdk_call_modifier', '_cli_required_function']]:
    conflict = [managed_names.get(name, name) for name in conflict]
    if conflict not in grammar['conflicts']:
        grammar['conflicts'].append(conflict)

# Two complete alternative function heads can share the original trailing body.
# Keep the prefix and body fragments explicit: neither pretends to be a complete
# function_definition or a compound_statement with a nonexistent opening brace.
rules['conditional_function_prefix'] = seq(
    optional(seq(string('extern'), field('language', symbol('string_literal')))),
    symbol('_declaration_specifiers'), field('declarator', symbol('function_declarator')), string('{'))
preproc_head = rules['preproc_if']['content']['members']
rules['conditional_function_else'] = seq(rules['preproc_else']['content']['members'][0],
    string('\n'), field('declaration', symbol('conditional_function_prefix')))
rules['conditional_function_if'] = seq(*preproc_head[:3],
    field('declaration', symbol('conditional_function_prefix')),
    field('alternative', alias_rule('conditional_function_else', 'preproc_else')), preproc_head[-1])
rules['conditional_function_body'] = seq(repeat(symbol('_block_item')), string('}'))
rules['conditional_function_definition'] = seq(
    field('prefixes', alias_rule('conditional_function_if', 'preproc_if')),
    field('body', symbol('conditional_function_body')))

# Ordinary preprocessor groups share _block_item upstream. Declaration-scoped
# copies admit the split definition without accepting it inside function bodies.
conditional_preproc = {name: 'conditional_decl_' + name for name in
    ['preproc_if', 'preproc_ifdef', 'preproc_else', 'preproc_elif', 'preproc_elifdef']}
def conditional_copy(node, aliased=False):
    if isinstance(node, list): return [conditional_copy(value, aliased) for value in node]
    if not isinstance(node, dict): return node
    if node.get('type') == 'ALIAS':
        return {key: conditional_copy(value, True) if key == 'content' else value
            for key, value in node.items()}
    if node.get('type') == 'SYMBOL':
        name = node['name']
        if name == '_block_item': return symbol('_conditional_decl_item')
        if name in conditional_preproc:
            return symbol(conditional_preproc[name]) if aliased else alias_rule(conditional_preproc[name], name)
        if name == 'namespace_definition': return alias_rule('conditional_decl_namespace', 'namespace_definition')
        if name == '_explicit_objc_inverse_body_guard':
            return symbol('conditional_decl_inverse_objc_guard')
    return {key: conditional_copy(value, aliased) for key, value in node.items()}
rules['conditional_decl_list'] = seq(string('{'), repeat(symbol('_conditional_decl_item')), string('}'))
rules['conditional_decl_namespace'] = objc_copy(rules['namespace_definition'], {
    'declaration_list': alias_rule('conditional_decl_list', 'declaration_list')})
rules['_conditional_decl_item'] = conditional_copy(rules['_block_item'])
rules['_conditional_decl_item']['members'].append(symbol('conditional_function_definition'))
for name, derived in conditional_preproc.items():
    # Prefer the declaration-scoped parse in a declaration context. Penalizing
    # every nested copied group can select an ordinary-block recovery path and
    # widen a later local syntax error into an ERROR covering the outer guard.
    rules[derived] = {'type': 'PREC_DYNAMIC', 'value': 1, 'content': conditional_copy(rules[name])}
rules['conditional_decl_inverse_objc_guard'] = conditional_copy(rules['_explicit_objc_inverse_body_guard'])
# Declaration-scoped inverse guards need Objective-C function bodies after the
# ordinary split-head arm. Require a primitive return and an actual function
# declarator; general declaration specifiers could treat namespace as a type.
rules['conditional_decl_objc_function'] = json.loads(json.dumps(rules['_objc_function_definition']))
objc_head = rules['conditional_decl_objc_function']['members']
for i, member in enumerate(objc_head):
    if member == symbol('_declaration_specifiers'):
        objc_head[i] = field('type', symbol('primitive_type'))
    elif member.get('type') == 'FIELD' and member.get('name') == 'declarator':
        member['content'] = symbol('function_declarator')
rules['conditional_decl_objc_function_else'] = json.loads(json.dumps(rules['_objc_inverse_top_else']))
rules['conditional_decl_objc_function_else']['content']['members'][2] = {'type': 'REPEAT1', 'content':
    alias_rule('conditional_decl_objc_function', 'function_definition')}
rules['conditional_decl_inverse_objc_guard'] = objc_copy(
    rules['conditional_decl_inverse_objc_guard'], {
        '_objc_inverse_body_else': symbol('conditional_decl_objc_function_else')})
for conflict in [
        ['type_specifier', 'call_expression', '_objc_call_expression', 'conditional_decl_objc_function'],
        ['type_specifier', 'conditional_decl_objc_function'],
        ['_declarator', 'expression', 'sdk_call_modifier'], ['expression', 'sdk_call_modifier']]:
    if conflict not in grammar['conflicts']: grammar['conflicts'].append(conflict)
for name in ['preproc_if', 'preproc_ifdef']:
    rules['_top_level_item']['members'].append(alias_rule(conditional_preproc[name], name))
rules['_top_level_item']['members'].append(symbol('conditional_function_definition'))
rules['_top_level_item']['members'].append(alias_rule('conditional_decl_namespace', 'namespace_definition'))
for conflict in [['_conditional_decl_item', 'statement'], ['_conditional_decl_item', 'preproc_split_if_open'], ['preproc_if', 'conditional_decl_preproc_if'], ['preproc_ifdef', 'conditional_decl_preproc_ifdef'], ['preproc_else', 'conditional_decl_preproc_else'], ['preproc_elif', 'conditional_decl_preproc_elif'], ['preproc_elifdef', 'conditional_decl_preproc_elifdef'], ['_declarator', 'conditional_function_prefix'], ['_block_item', '_conditional_decl_item'], ['_block_item', 'preproc_split_if_open', '_conditional_decl_item'], ['_block_item', 'statement', '_conditional_decl_item'], ['_explicit_objc_inverse_body_guard', 'conditional_decl_inverse_objc_guard']]:
    if conflict not in grammar['conflicts']: grammar['conflicts'].append(conflict)

for conflict in [['declaration_list', 'conditional_decl_list']]:
    if conflict not in grammar['conflicts']: grammar['conflicts'].append(conflict)

# Complete preprocessing groups may add a logical suffix to a return value.
# This is return-only syntax, not a general expression/macro assumption. Require
# a real operand in each group and real directive line endings; keep conditions
# and all possible right operands as original preproc nodes, without evaluation.
rules['_conditional_logical_tail'] = seq(
    field('operator', choice(string('||'), string('&&'))), field('right', symbol('expression')))
if_head = rules['preproc_if']['content']['members']
rules['_conditional_logical_if'] = seq(*if_head[:3], symbol('_conditional_logical_tail'),
    if_head[-1], string('\n'))
ifdef_head = rules['preproc_ifdef']['content']['members']
rules['_conditional_logical_ifdef'] = seq(*ifdef_head[:2], string('\n'),
    symbol('_conditional_logical_tail'), ifdef_head[-1], string('\n'))
rules['conditional_logical_expression'] = seq(field('left', symbol('expression')),
    {'type': 'REPEAT1', 'content': choice(alias_rule('_conditional_logical_if', 'preproc_if'),
        alias_rule('_conditional_logical_ifdef', 'preproc_ifdef'))})
rules['return_statement'] = choice(rules['return_statement'],
    seq(string('return'), symbol('conditional_logical_expression'), string(';')))

# A return may instead start with complete optional logical prefixes and end
# in a complete value group with an explicit else operand. Every possible arm
# retains its original expression and condition; no condition is evaluated.
prefix = seq(field('left', symbol('expression')),
    field('operator', choice(string('&&'), string('||'))))
rules['_conditional_return_prefix_if'] = seq(*if_head[:3], prefix, if_head[-1], string('\n'))
rules['_conditional_return_prefix_ifdef'] = seq(*ifdef_head[:2], string('\n'),
    prefix, ifdef_head[-1], string('\n'))
else_head = rules['preproc_else']['content']['members'][0]
rules['_conditional_return_value_else'] = seq(else_head, string('\n'),
    field('right', symbol('expression')))
alternative = field('alternative', alias_rule('_conditional_return_value_else', 'preproc_else'))
rules['_conditional_return_value_if'] = seq(*if_head[:3], field('left', symbol('expression')),
    alternative, if_head[-1], string('\n'))
rules['_conditional_return_value_ifdef'] = seq(*ifdef_head[:2], string('\n'),
    field('left', symbol('expression')), alternative, ifdef_head[-1], string('\n'))
rules['conditional_return_expression'] = seq(repeat(choice(
    alias_rule('_conditional_return_prefix_if', 'preproc_if'),
    alias_rule('_conditional_return_prefix_ifdef', 'preproc_ifdef'))), choice(
    alias_rule('_conditional_return_value_if', 'preproc_if'),
    alias_rule('_conditional_return_value_ifdef', 'preproc_ifdef')))
rules['return_statement']['members'].append(seq(string('return'),
    symbol('conditional_return_expression'), string(';')))

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
