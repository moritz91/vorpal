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
path.write_bytes((json.dumps(grammar, indent=2) + '\n').encode('utf-8'))

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
