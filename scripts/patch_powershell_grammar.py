"""Apply conservative PowerShell literal/command fixes; generate with ABI 15.

tree-sitter 0.25.10 generate --abi 15 src/grammar.json
"""
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / 'grammars/tree-sitter-powershell'
js_path = root / 'grammar.js'
source = js_path.read_text(encoding='utf-8')
source = source.replace('externals: ($) => [$._statement_terminator],',
    'externals: ($) => [$._statement_terminator, $._native_argument_separator],')
source = source.replace('externals: ($) => [$._statement_terminator, $._native_argument_separator],',
    'externals: ($) => [$._statement_terminator, $._native_argument_separator, $._native_assignment_prefix],')
if 'alias($._native_argument_separator, $.command_parameter)' not in source:
    source = source.replace('$.command_parameter,\n          seq($._command_argument',
        '$.command_parameter,\n          alias($._native_argument_separator, $.command_parameter),\n          seq($._command_argument')
source = source.replace("choice('kb', 'mb', 'gb', 'tb', 'pb')", '/[kKmMgGtTpP][bB]/')
source = source.replace(
    'invokation_expression: ($) =>\n      choice(\n        seq(',
    'invokation_expression: ($) =>\n      choice(\n        prec.left(PREC.PARAM + 1, seq(',
).replace(
    "$.member_name,\n          $.argument_list,\n        ),\n        seq($._primary_expression, '::'",
    "$.member_name,\n          $.argument_list,\n        )),\n        seq($._primary_expression, '::'",
)
json_path = root / 'src/grammar.json'
grammar = json.loads(json_path.read_text(encoding='utf-8'))
# Cmdlet parameter names admit digits after the initial name character.
# Keep leading-digit arguments numeric rather than reclassifying -256 as a name.
parameter = grammar['rules']['command_parameter']['content']['members'][0]
old_parameter = r'-+[a-zA-Z_?\-`]+'
new_parameter = r'-+[a-zA-Z_?\-`][a-zA-Z0-9_?\-`]*'
if parameter['value'] == old_parameter:
    parameter['value'] = new_parameter
source = source.replace(old_parameter, new_parameter)

separator = {'type': 'SYMBOL', 'name': '_native_argument_separator'}
if separator not in grammar['externals']:
    grammar['externals'].append(separator)
alias = {'type': 'ALIAS', 'content': separator, 'named': True, 'value': 'command_parameter'}
elements = grammar['rules']['_command_element']['content']['members']
if alias not in elements:
    elements.insert(1, alias)

def symbol(name): return {'type': 'SYMBOL', 'name': name}
def seq(*members): return {'type': 'SEQ', 'members': list(members)}
def choice(*members): return {'type': 'CHOICE', 'members': list(members)}
def pattern(value): return {'type': 'PATTERN', 'value': value}
def named_alias(content, name): return {'type': 'ALIAS', 'content': content, 'named': True, 'value': name}
prefix = symbol('_native_assignment_prefix')
if prefix not in grammar['externals']:
    grammar['externals'].append(prefix)
variable_pattern = r'\$(?:[a-zA-Z0-9_]+:)?[a-zA-Z0-9_]+'
grammar['rules']['_native_assignment_argument'] = choice(
    {'type': 'TOKEN', 'content': seq(pattern(r'--[a-zA-Z_][a-zA-Z0-9_-]*='),
        choice(pattern(r"'([^']|'')*'"), pattern(r'''[^\s;,|&(){}"'$`]+'''), {'type': 'BLANK'}))},
    seq(prefix, named_alias({'type': 'IMMEDIATE_TOKEN', 'content': pattern(variable_pattern)}, 'variable')),
)
array_element = choice(symbol('generic_token'), symbol('unary_expression'))
grammar['rules']['_native_array_argument'] = {'type': 'PREC_RIGHT', 'value': 7,
    'content': seq(array_element, {'type': 'REPEAT1', 'content': seq({'type': 'STRING', 'value': ','}, array_element)})}
conflict = ['array_literal_expression', '_native_array_argument']
if conflict not in grammar['conflicts']:
    grammar['conflicts'].append(conflict)
if '[$.array_literal_expression, $._native_array_argument]' not in source:
    source = source.replace('    [$.path_command_name, $._value],',
        '    [$.path_command_name, $._value],\n    [$.array_literal_expression, $._native_array_argument],')
arguments = grammar['rules']['_command_argument']['content']['members']
for rule, name in [('_native_assignment_argument', 'generic_token'), ('_native_array_argument', 'array_literal_expression')]:
    alternative = seq(symbol('command_argument_sep'), named_alias(symbol(rule), name))
    if alternative not in arguments:
        arguments.append(alternative)
if '_native_assignment_argument:' not in source:
    source = source.replace('    verbatim_command_argument: ($) =>', r'''    _native_assignment_argument: ($) => choice(
      token(seq(/--[a-zA-Z_][a-zA-Z0-9_-]*=/,
        choice(/'([^']|'')*'/, /[^\s;,|&(){}"'$`]+/, ''))),
      seq($._native_assignment_prefix,
        alias(token.immediate(/\$(?:[a-zA-Z0-9_]+:)?[a-zA-Z0-9_]+/), $.variable)),
    ),

    _native_array_argument: ($) => prec.right(7, seq(
      choice($.generic_token, $.unary_expression),
      repeat1(seq(',', choice($.generic_token, $.unary_expression))),
    )),

    verbatim_command_argument: ($) =>''')
    source = source.replace('          $.script_block_expression,\n        ),\n      ),\n\n    _native_assignment_argument:',
        '          $.script_block_expression,\n          seq($.command_argument_sep, alias($._native_assignment_argument, $.generic_token)),\n          seq($.command_argument_sep, alias($._native_array_argument, $.array_literal_expression)),\n        ),\n      ),\n\n    _native_assignment_argument:')
source = source.replace(r'''\&\s][^''', r'''\&\s,;][^''')
generic = grammar['rules']['generic_token']['content']
generic['value'] = generic['value'].replace(r'\&\s][^', r'\&\s,;][^')
js_path.write_text(source, encoding='utf-8', newline='\n')
units = {'type': 'CHOICE', 'members': [
    {'type': 'STRING', 'value': unit} for unit in ['kb', 'mb', 'gb', 'tb', 'pb']
]}

def patch(node):
    if node == units:
        node.clear()
        node.update({'type': 'PATTERN', 'value': '[kKmMgGtTpP][bB]'})
    for value in node.values():
        if isinstance(value, dict):
            patch(value)
        elif isinstance(value, list):
            for child in value:
                if isinstance(child, dict):
                    patch(child)

for name in ['decimal_integer_literal', 'hexadecimal_integer_literal', 'real_literal']:
    patch(grammar['rules'][name])
invocation = grammar['rules']['invokation_expression']
if invocation['members'][0]['type'] == 'SEQ':
    invocation['members'][0] = {
        'type': 'PREC_LEFT', 'value': 7, 'content': invocation['members'][0],
    }
json_path.write_text(json.dumps(grammar, indent=2) + '\n', encoding='utf-8', newline='\n')
(root / 'src/scanner.c').write_bytes(Path(__file__).with_name('powershell_scanner.c').read_bytes())
