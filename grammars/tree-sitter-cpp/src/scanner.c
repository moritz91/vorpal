#include "tree_sitter/alloc.h"
#include "tree_sitter/parser.h"

#include <assert.h>
#include <string.h>
#include <wctype.h>

enum TokenType { RAW_STRING_DELIMITER, RAW_STRING_CONTENT, PROVEN_STATEMENT_MACRO, PROVEN_OPEN_IF_MACRO, PROVEN_FUNCTION_MACRO, PROVEN_DECLARATION_MACRO, PROVEN_CASE_LOOP_MACRO, PROVEN_INLINE_SPECIFIER_MACRO };

// vorpal: scoped statement-macro proof context; never serialized into a tree.
#if defined(_MSC_VER)
static __declspec(thread) const char *statement_macros;
#else
static __thread const char *statement_macros;
#endif
const char *tree_sitter_cpp_set_statement_macros(const char *names) {
    const char *previous = statement_macros;
    statement_macros = names;
    return previous;
}
typedef struct {
    uint32_t offset;
    const char *name;
    uint8_t kind;
} StatementMacroSite;
typedef struct {
    const StatementMacroSite *sites;
    size_t length;
    uint32_t (*byte_offset)(const void *lexer);
} StatementMacroSites;
#if defined(_MSC_VER)
static __declspec(thread) const StatementMacroSites *statement_sites;
#else
static __thread const StatementMacroSites *statement_sites;
#endif
const StatementMacroSites *tree_sitter_cpp_set_statement_macro_sites(const StatementMacroSites *sites) {
    const StatementMacroSites *previous = statement_sites;
    statement_sites = sites;
    return previous;
}
static bool statement_space(int32_t c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v';
}
static bool statement_comment_char(int32_t c, unsigned *questions) {
    if (c == '\\' || (c == '/' && *questions >= 2)) return false;
    *questions = c == '?' ? *questions + 1 : 0;
    if (*questions > 2) *questions = 2;
    return true;
}
// Match invocation_spacing in the Rust proof audit. Keep mark_end at the name:
// comments remain ordinary extra nodes with their original source spans.
static bool statement_spacing(TSLexer *lexer) {
    for (;;) {
        while (statement_space(lexer->lookahead)) lexer->advance(lexer, false);
        if (lexer->lookahead != '/') return true;
        lexer->advance(lexer, false);
        unsigned questions = 0;
        if (lexer->lookahead == '/') {
            while (!lexer->eof(lexer) && lexer->lookahead != '\r' && lexer->lookahead != '\n') {
                if (!statement_comment_char(lexer->lookahead, &questions)) return false;
                lexer->advance(lexer, false);
            }
        } else if (lexer->lookahead == '*') {
            lexer->advance(lexer, false);
            bool star = false;
            bool closed = false;
            while (!lexer->eof(lexer)) {
                if (!statement_comment_char(lexer->lookahead, &questions)) return false;
                if (star && lexer->lookahead == '/') {
                    lexer->advance(lexer, false);
                    closed = true;
                    break;
                }
                star = lexer->lookahead == '*';
                lexer->advance(lexer, false);
            }
            if (!closed) return false;
        } else {
            return false;
        }
    }
}
static bool scan_statement_macro(TSLexer *lexer, const bool *valid_symbols) {
    if (!statement_macros && !statement_sites) return false;
    while (statement_space(lexer->lookahead)) lexer->advance(lexer, true);
    const StatementMacroSite *site = NULL;
    if (statement_sites) {
        if (!statement_sites->byte_offset) return false;
        uint32_t offset = statement_sites->byte_offset(lexer);
        size_t low = 0, high = statement_sites->length;
        while (low < high) {
            size_t middle = low + (high - low) / 2;
            if (statement_sites->sites[middle].offset < offset) low = middle + 1;
            else high = middle;
        }
        if (low == statement_sites->length || statement_sites->sites[low].offset != offset) return false;
        site = &statement_sites->sites[low];
    }
    char name[128];
    unsigned length = 0;
    while ((lexer->lookahead >= 'a' && lexer->lookahead <= 'z') ||
           (lexer->lookahead >= 'A' && lexer->lookahead <= 'Z') ||
           (lexer->lookahead >= '0' && lexer->lookahead <= '9') || lexer->lookahead == '_') {
        if (length == sizeof(name)) return false;
        name[length++] = (char)lexer->lookahead;
        lexer->advance(lexer, false);
    }
    if (!length) return false;
    lexer->mark_end(lexer);
    if (site && site->kind == 5) {
        if (strlen(site->name) != length || memcmp(site->name, name, length)) return false;
        if (!valid_symbols[PROVEN_INLINE_SPECIFIER_MACRO]) return false;
        lexer->result_symbol = PROVEN_INLINE_SPECIFIER_MACRO;
        return true;
    }
    if (!statement_spacing(lexer) || lexer->lookahead != '(') return false;
    if (site) {
        if (strlen(site->name) != length || memcmp(site->name, name, length)) return false;
        enum TokenType kind;
        switch (site->kind) {
          case 0: kind = PROVEN_STATEMENT_MACRO; break;
          case 1: kind = PROVEN_OPEN_IF_MACRO; break;
          case 2: kind = PROVEN_FUNCTION_MACRO; break;
          case 3: kind = PROVEN_DECLARATION_MACRO; break;
          case 4: kind = PROVEN_CASE_LOOP_MACRO; break;
          default: return false;
        }
        if (!valid_symbols[kind]) return false;
        lexer->result_symbol = kind;
        return true;
    }
    for (const char *entry = statement_macros; *entry;) {
        bool open_if = *entry == '?';
        const char *name_entry = entry + (open_if ? 1 : 0);
        const char *end = strchr(name_entry, '\n');
        if (!end) return false;
        if ((unsigned)(end - name_entry) == length && !memcmp(name_entry, name, length)) {
            enum TokenType kind = open_if ? PROVEN_OPEN_IF_MACRO : PROVEN_STATEMENT_MACRO;
            if (!valid_symbols[kind]) return false;
            lexer->result_symbol = kind;
            return true;
        }
        entry = end + 1;
    }
    return false;
}

/// The spec limits delimiters to 16 chars
#define MAX_DELIMITER_LENGTH 16

typedef struct {
    uint8_t delimiter_length;
    wchar_t delimiter[MAX_DELIMITER_LENGTH];
} Scanner;

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

static inline void reset(Scanner *scanner) {
    scanner->delimiter_length = 0;
    memset(scanner->delimiter, 0, sizeof scanner->delimiter);
}

/// Scan the raw string delimiter in R"delimiter(content)delimiter"
static bool scan_raw_string_delimiter(Scanner *scanner, TSLexer *lexer) {
    if (scanner->delimiter_length > 0) {
        // Closing delimiter: must exactly match the opening delimiter.
        // We already checked this when scanning content, but this is how we
        // know when to stop. We can't stop at ", because R"""hello""" is valid.
        for (int i = 0; i < scanner->delimiter_length; ++i) {
            if (lexer->lookahead != scanner->delimiter[i]) {
                return false;
            }
            advance(lexer);
        }
        reset(scanner);
        return true;
    }

    // Opening delimiter: record the d-char-sequence up to (.
    // d-char is any basic character except parens, backslashes, and spaces.
    for (;;) {
        if (scanner->delimiter_length >= MAX_DELIMITER_LENGTH || lexer->eof(lexer) || lexer->lookahead == '\\' ||
            iswspace(lexer->lookahead)) {
            return false;
        }
        if (lexer->lookahead == '(') {
            // Rather than create a token for an empty delimiter, we fail and
            // let the grammar fall back to a delimiter-less rule.
            return scanner->delimiter_length > 0;
        }
        scanner->delimiter[scanner->delimiter_length++] = lexer->lookahead;
        advance(lexer);
    }
}

/// Scan the raw string content in R"delimiter(content)delimiter"
static bool scan_raw_string_content(Scanner *scanner, TSLexer *lexer) {
    // The progress made through the delimiter since the last ')'.
    // The delimiter may not contain ')' so a single counter suffices.
    for (int delimiter_index = -1;;) {
        // If we hit EOF, consider the content to terminate there.
        // This forms an incomplete raw_string_literal, and models the code
        // well.
        if (lexer->eof(lexer)) {
            lexer->mark_end(lexer);
            return true;
        }

        if (delimiter_index >= 0) {
            if (delimiter_index == scanner->delimiter_length) {
                if (lexer->lookahead == '"') {
                    return true;
                }
                delimiter_index = -1;
            } else {
                if (lexer->lookahead == scanner->delimiter[delimiter_index]) {
                    delimiter_index += 1;
                } else {
                    delimiter_index = -1;
                }
            }
        }

        if (delimiter_index == -1 && lexer->lookahead == ')') {
            // The content doesn't include the )delimiter" part.
            // We must still scan through it, but exclude it from the token.
            lexer->mark_end(lexer);
            delimiter_index = 0;
        }

        advance(lexer);
    }
}

void *tree_sitter_cpp_external_scanner_create() {
    Scanner *scanner = (Scanner *)ts_calloc(1, sizeof(Scanner));
    memset(scanner, 0, sizeof(Scanner));
    return scanner;
}

bool tree_sitter_cpp_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
    Scanner *scanner = (Scanner *)payload;

    if (valid_symbols[RAW_STRING_DELIMITER] && valid_symbols[RAW_STRING_CONTENT]) {
        // we're in error recovery
        return false;
    }

    if (valid_symbols[PROVEN_STATEMENT_MACRO] || valid_symbols[PROVEN_OPEN_IF_MACRO] || valid_symbols[PROVEN_FUNCTION_MACRO] || valid_symbols[PROVEN_DECLARATION_MACRO]) {
        return scan_statement_macro(lexer, valid_symbols);
    }

    // No skipping leading whitespace: raw-string grammar is space-sensitive.
    if (valid_symbols[RAW_STRING_DELIMITER]) {
        lexer->result_symbol = RAW_STRING_DELIMITER;
        return scan_raw_string_delimiter(scanner, lexer);
    }

    if (valid_symbols[RAW_STRING_CONTENT]) {
        lexer->result_symbol = RAW_STRING_CONTENT;
        return scan_raw_string_content(scanner, lexer);
    }

    return false;
}

unsigned tree_sitter_cpp_external_scanner_serialize(void *payload, char *buffer) {
    static_assert(MAX_DELIMITER_LENGTH * sizeof(wchar_t) < TREE_SITTER_SERIALIZATION_BUFFER_SIZE,
                  "Serialized delimiter is too long!");

    Scanner *scanner = (Scanner *)payload;
    size_t size = scanner->delimiter_length * sizeof(wchar_t);
    memcpy(buffer, scanner->delimiter, size);
    return (unsigned)size;
}

void tree_sitter_cpp_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
    assert(length % sizeof(wchar_t) == 0 && "Can't decode serialized delimiter!");

    Scanner *scanner = (Scanner *)payload;
    scanner->delimiter_length = length / sizeof(wchar_t);
    if (length > 0) {
        memcpy(&scanner->delimiter[0], buffer, length);
    }
}

void tree_sitter_cpp_external_scanner_destroy(void *payload) {
    Scanner *scanner = (Scanner *)payload;
    ts_free(scanner);
}
