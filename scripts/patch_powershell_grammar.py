"""Apply conservative PowerShell literal fixes; generate with ABI 15.

tree-sitter 0.25.10 generate --abi 15 src/grammar.json
"""
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1] / 'grammars/tree-sitter-powershell'
js_path = root / 'grammar.js'
source = js_path.read_text(encoding='utf-8')
source = source.replace("choice('kb', 'mb', 'gb', 'tb', 'pb')", '/[kKmMgGtTpP][bB]/')
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
json_path.write_text(json.dumps(grammar, indent=2) + '\n', encoding='utf-8', newline='\n')
