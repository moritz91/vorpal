"""Apply conservative PowerShell literal/instance-invocation fixes; generate with ABI 15.

tree-sitter 0.25.10 generate --abi 15 src/grammar.json
"""
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / 'grammars/tree-sitter-powershell'
js_path = root / 'grammar.js'
source = js_path.read_text(encoding='utf-8')
source = source.replace("choice('kb', 'mb', 'gb', 'tb', 'pb')", '/[kKmMgGtTpP][bB]/')
source = source.replace(
    'invokation_expression: ($) =>\n      choice(\n        seq(',
    'invokation_expression: ($) =>\n      choice(\n        prec.left(PREC.PARAM + 1, seq(',
).replace(
    "$.member_name,\n          $.argument_list,\n        ),\n        seq($._primary_expression, '::'",
    "$.member_name,\n          $.argument_list,\n        )),\n        seq($._primary_expression, '::'",
)
js_path.write_text(source, encoding='utf-8', newline='\n')

json_path = root / 'src/grammar.json'
grammar = json.loads(json_path.read_text(encoding='utf-8'))
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
