// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License. See the LICENSE file in the project root for full license information.
// Fork: recognize whitespace-delimited native argument separators in command contexts.

#include "tree_sitter/parser.h"
#include <wctype.h>

enum TOKEN_TYPE {
    STATEMENT_TERMINATOR,
    NATIVE_ARGUMENT_SEPARATOR,
    NATIVE_ASSIGNMENT_PREFIX
};

static bool name_character(int32_t c)
{
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
        (c >= '0' && c <= '9') || c == '_';
}

static bool boundary(int32_t c)
{
    return c == 0 || iswspace(c) || c == ')' || c == '}' || c == ';' || c == '|';
}

bool tree_sitter_powershell_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols)
{
    (void)payload;
    if (!valid_symbols[STATEMENT_TERMINATOR] && !valid_symbols[NATIVE_ARGUMENT_SEPARATOR] &&
        !valid_symbols[NATIVE_ASSIGNMENT_PREFIX]) return false;
    // Preserve the original zero-width terminator, including its location before spaces.
    lexer->mark_end(lexer);
    bool whitespace = false;
    for (;;) {
        if (lexer->lookahead == 0 || lexer->lookahead == '}' || lexer->lookahead == ';' ||
            lexer->lookahead == ')' || lexer->lookahead == '\n') {
            if (!valid_symbols[STATEMENT_TERMINATOR]) return false;
            lexer->result_symbol = STATEMENT_TERMINATOR;
            return true;
        }
        if (!iswspace(lexer->lookahead)) break;
        whitespace = true;
        lexer->advance(lexer, true);
    }
    // Requiring preceding whitespace avoids consuming a suffix in $value--.
    // The grammar enables this token only in command elements, never expressions.
    if (lexer->lookahead != '-') return false;
    lexer->advance(lexer, false);
    if (lexer->lookahead != '-') return false;
    lexer->advance(lexer, false);
    if (valid_symbols[NATIVE_ARGUMENT_SEPARATOR] && whitespace && boundary(lexer->lookahead)) {
        lexer->mark_end(lexer);
        lexer->result_symbol = NATIVE_ARGUMENT_SEPARATOR;
        return true;
    }
    if (!valid_symbols[NATIVE_ASSIGNMENT_PREFIX] ||
        !((lexer->lookahead >= 'a' && lexer->lookahead <= 'z') ||
          (lexer->lookahead >= 'A' && lexer->lookahead <= 'Z') || lexer->lookahead == '_')) return false;
    do { lexer->advance(lexer, false); } while (name_character(lexer->lookahead) || lexer->lookahead == '-');
    if (lexer->lookahead != '=') return false;
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    // Look ahead beyond the prefix to validate an adjacent complete variable.
    // Tree-sitter records this lookahead range for incremental invalidation.
    if (lexer->lookahead != '$') return false;
    lexer->advance(lexer, false);
    if (!name_character(lexer->lookahead)) return false;
    do { lexer->advance(lexer, false); } while (name_character(lexer->lookahead));
    if (lexer->lookahead == ':') {
        lexer->advance(lexer, false);
        if (!name_character(lexer->lookahead)) return false;
        do { lexer->advance(lexer, false); } while (name_character(lexer->lookahead));
    }
    if (!boundary(lexer->lookahead)) return false;
    lexer->result_symbol = NATIVE_ASSIGNMENT_PREFIX;
    return true;
}

void *tree_sitter_powershell_external_scanner_create(void) { return NULL; }
void tree_sitter_powershell_external_scanner_destroy(void *payload) { (void)payload; }
unsigned tree_sitter_powershell_external_scanner_serialize(void *payload, char *buffer)
{
    (void)payload;
    (void)buffer;
    return 0;
}
void tree_sitter_powershell_external_scanner_deserialize(void *payload, const char *buffer, unsigned length)
{
    (void)payload;
    (void)buffer;
    (void)length;
}
