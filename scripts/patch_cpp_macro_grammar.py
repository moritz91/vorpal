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

operators = rules['field_expression']['members'][0]['content']['members'][1]['content']['members']
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
# Abstract member-function pointers are admitted only as explicit type descriptors.
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
