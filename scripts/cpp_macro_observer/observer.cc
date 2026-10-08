#include "clang/Frontend/CompilerInstance.h"
#include "clang/Frontend/CompilerInvocation.h"
#include "clang/Basic/Version.h"
#include "clang/Frontend/FrontendAction.h"
#include "clang/Lex/Lexer.h"
#include "clang/Lex/MacroInfo.h"
#include "clang/Lex/PPCallbacks.h"
#include "clang/Lex/Preprocessor.h"
#include "clang/Tooling/Tooling.h"
#include "llvm/ADT/StringExtras.h"
#include "llvm/ADT/SmallString.h"
#include "llvm/Support/FormatVariadic.h"
#include "llvm/Support/JSON.h"
#include "llvm/Support/SHA256.h"
#include "llvm/Support/VirtualFileSystem.h"

using namespace clang;
namespace vfs = llvm::vfs;
void emit(llvm::json::Object row) {
  llvm::outs() << llvm::formatv("{0}\n", llvm::json::Value(std::move(row)));
}
std::string hash(llvm::StringRef text) {
  return llvm::toHex(llvm::SHA256::hash(llvm::arrayRefFromStringRef(text)));
}
class ObservedFile : public vfs::File {
  std::unique_ptr<vfs::File> file;
  std::string path;
public:
  ObservedFile(std::unique_ptr<vfs::File> file, std::string path)
      : file(std::move(file)), path(std::move(path)) {}
  llvm::ErrorOr<vfs::Status> status() override {
    auto result = file->status();
    llvm::json::Object row{{"kind", "file_status"}, {"path", path}, {"ok", bool(result)}};
    if (result) {
      row["name"] = result->getName().str();
      row["type"] = int64_t(result->getType());
      row["bytes"] = int64_t(result->getSize());
      row["mtime"] = int64_t(result->getLastModificationTime().time_since_epoch().count());
      row["device"] = int64_t(result->getUniqueID().getDevice());
      row["file"] = int64_t(result->getUniqueID().getFile());
    } else row["error"] = result.getError().value();
    emit(std::move(row));
    return result;
  }
  llvm::ErrorOr<std::string> getName() override {
    auto result = file->getName();
    llvm::json::Object row{{"kind", "file_name"}, {"path", path}, {"ok", bool(result)}};
    if (result) row["name"] = *result; else row["error"] = result.getError().value();
    emit(std::move(row));
    return result;
  }
  std::error_code close() override { return file->close(); }
  llvm::ErrorOr<std::unique_ptr<llvm::MemoryBuffer>> getBuffer(
      const llvm::Twine &name, int64_t size, bool null, bool vol) override {
    auto result = file->getBuffer(name, size, null, vol);
    llvm::json::Object row{{"kind", "buffer"}, {"path", path}, {"ok", bool(result)}};
    if (result) {
      row["sha256"] = hash((*result)->getBuffer());
      row["bytes"] = int64_t((*result)->getBufferSize());
    } else row["error"] = result.getError().value();
    emit(std::move(row));
    return result;
  }
};
class ObservedDirectory : public vfs::detail::DirIterImpl {
  std::vector<vfs::directory_entry> entries;
  size_t index = 0;
public:
  ObservedDirectory(std::vector<vfs::directory_entry> entries) : entries(std::move(entries)) {
    if (!this->entries.empty()) CurrentEntry = this->entries[0];
  }
  std::error_code increment() override {
    ++index;
    CurrentEntry = index < entries.size() ? entries[index] : vfs::directory_entry();
    return {};
  }
};
class ObservedFS : public vfs::ProxyFileSystem {
public:
  ObservedFS() : ProxyFileSystem(vfs::getRealFileSystem()) {}
  vfs::directory_iterator dir_begin(const llvm::Twine &path, std::error_code &error) override {
    auto iterator = ProxyFileSystem::dir_begin(path, error);
    std::vector<vfs::directory_entry> entries;
    llvm::json::Array observed;
    for (; !error && iterator != vfs::directory_iterator(); iterator.increment(error)) {
      entries.push_back(*iterator);
      observed.push_back(llvm::json::Object{{"path", iterator->path().str()}, {"type", int64_t(iterator->type())}});
      if (entries.size() > 16384) { error = std::make_error_code(std::errc::value_too_large); break; }
    }
    emit(llvm::json::Object{{"kind", "directory"}, {"path", path.str()}, {"ok", !bool(error)},
      {"error", error.value()}, {"entries", std::move(observed)}});
    if (error) return {};
    return vfs::directory_iterator(std::make_shared<ObservedDirectory>(std::move(entries)));
  }
  bool exists(const llvm::Twine &path) override {
    // Use our observed status rather than ProxyFileSystem's bypass to the child.
    return bool(status(path));
  }
  llvm::ErrorOr<std::string> getCurrentWorkingDirectory() const override {
    auto result = ProxyFileSystem::getCurrentWorkingDirectory();
    llvm::json::Object row{{"kind", "get_cwd"}, {"ok", bool(result)}};
    if (result) row["path"] = *result; else row["error"] = result.getError().value();
    emit(std::move(row)); return result;
  }
  std::error_code setCurrentWorkingDirectory(const llvm::Twine &path) override {
    auto result = ProxyFileSystem::setCurrentWorkingDirectory(path);
    emit(llvm::json::Object{{"kind", "set_cwd"}, {"path", path.str()}, {"error", result.value()}});
    return result;
  }
  std::error_code makeAbsolute(llvm::SmallVectorImpl<char> &path) const override {
    auto before = std::string(path.begin(), path.end());
    auto result = ProxyFileSystem::makeAbsolute(path);
    emit(llvm::json::Object{{"kind", "absolute"}, {"path", before},
      {"result", std::string(path.begin(), path.end())}, {"error", result.value()}});
    return result;
  }
  std::error_code isLocal(const llvm::Twine &path, bool &local) override {
    auto result = ProxyFileSystem::isLocal(path, local);
    emit(llvm::json::Object{{"kind", "local"}, {"path", path.str()},
      {"local", !result && local}, {"error", result.value()}});
    return result;
  }
  void visitChildFileSystems(VisitCallbackTy callback) override {
    emit(llvm::json::Object{{"kind", "child_visit"}, {"observation_complete", false}});
    ProxyFileSystem::visitChildFileSystems(callback);
  }
  llvm::ErrorOr<vfs::Status> status(const llvm::Twine &path) override {
    auto result = ProxyFileSystem::status(path);
    llvm::json::Object row{{"kind", "status"}, {"path", path.str()}, {"ok", bool(result)}};
    if (result) {
      row["type"] = int64_t(result->getType());
      row["bytes"] = int64_t(result->getSize());
      row["name"] = result->getName().str();
      row["mtime"] = int64_t(result->getLastModificationTime().time_since_epoch().count());
      row["device"] = int64_t(result->getUniqueID().getDevice());
      row["file"] = int64_t(result->getUniqueID().getFile());
    } else row["error"] = result.getError().value();
    emit(std::move(row));
    return result;
  }
  llvm::ErrorOr<std::unique_ptr<vfs::File>> openFileForRead(const llvm::Twine &path) override {
    auto result = ProxyFileSystem::openFileForRead(path);
    llvm::json::Object row{{"kind", "open"}, {"path", path.str()}, {"ok", bool(result)}};
    if (!result) row["error"] = result.getError().value();
    emit(std::move(row));
    if (!result) return result.getError();
    return std::unique_ptr<vfs::File>(new ObservedFile(std::move(*result), path.str()));
  }
  std::error_code getRealPath(const llvm::Twine &path, llvm::SmallVectorImpl<char> &out) override {
    auto error = ProxyFileSystem::getRealPath(path, out);
    emit(llvm::json::Object{{"kind", "realpath"}, {"path", path.str()}, {"ok", !bool(error)},
          {"result", error ? "" : std::string(out.begin(), out.end())}});
    return error;
  }
};
class Observer : public PPCallbacks {
  Preprocessor &pp;
  SourceManager &sm;
  llvm::json::Object location(SourceLocation loc) {
    auto spelling = sm.getSpellingLoc(loc);
    llvm::json::Object row{{"nested", loc.isMacroID()}};
    if (spelling.isValid()) {
      row["path"] = sm.getFilename(spelling).str();
      row["offset"] = int64_t(sm.getFileOffset(spelling));
      row["buffer_sha256"] = hash(sm.getBufferData(sm.getFileID(spelling)));
    }
    return row;
  }
public:
  Observer(Preprocessor &pp) : pp(pp), sm(pp.getSourceManager()) {}
  void MacroExpands(const Token &token, const MacroDefinition &definition,
                    SourceRange range, const MacroArgs *) override {
    auto name = pp.getSpelling(token);
    if (name == "__DATE__" || name == "__TIME__" || name == "__TIMESTAMP__")
      emit(llvm::json::Object{{"kind", "volatile_macro"}, {"name", name}, {"location", location(range.getBegin())}});
    // Observe every expansion rooted in the original translation unit, including
    // nested replacement/argument effects. Never infer macro status from a name.
    if (!sm.isWrittenInMainFile(sm.getExpansionLoc(token.getLocation()))) return;
    llvm::json::Object row{{"kind", "macro"}, {"name", name},
        {"begin", location(range.getBegin())}, {"end", location(range.getEnd())},
        {"end_exclusive", location(Lexer::getLocForEndOfToken(range.getEnd(), 0, sm, pp.getLangOpts()))}};
    if (auto *info = definition.getMacroInfo()) {
      row["definition"] = location(info->getDefinitionLoc());
      row["definition_end"] = location(info->getDefinitionEndLoc());
      row["definition_end_exclusive"] = location(Lexer::getLocForEndOfToken(info->getDefinitionEndLoc(), 0, sm, pp.getLangOpts()));
      row["parameters"] = int64_t(info->getNumParams());
      row["function_like"] = info->isFunctionLike();
      llvm::json::Array tokens;
      for (const auto &part : info->tokens()) tokens.push_back(pp.getSpelling(part));
      row["replacement_tokens"] = std::move(tokens);
    }
    emit(std::move(row));
  }
  void HasInclude(SourceLocation loc, llvm::StringRef name, bool angled,
                  OptionalFileEntryRef file, SrcMgr::CharacteristicKind) override {
    emit(llvm::json::Object{{"kind", "has_include"}, {"name", name.str()}, {"angled", angled},
          {"present", bool(file)}, {"location", location(loc)}});
  }
  bool FileNotFound(llvm::StringRef name) override {
    emit(llvm::json::Object{{"kind", "file_not_found"}, {"name", name.str()}});
    return false;
  }
};
class ObserveAction : public PreprocessorFrontendAction {
  bool BeginSourceFileAction(CompilerInstance &ci) override {
    std::string cc1;
    ci.getInvocation().generateCC1CommandLine([&](const llvm::Twine &argument) {
      cc1 += argument.str(); cc1.push_back('\0');
    });
    emit(llvm::json::Object{{"kind", "compiler_context"},
      {"clang_version", getClangFullVersion()}, {"effective_cc1_sha256", hash(cc1)},
      {"predefines_sha256", hash(ci.getPreprocessor().getPredefines())},
      {"ms_compatibility_version", int64_t(ci.getLangOpts().MSCompatibilityVersion)},
      {"cxx20", bool(ci.getLangOpts().CPlusPlus20)},
      {"production_proof_contract", false}});
    ci.getPreprocessor().addPPCallbacks(std::make_unique<Observer>(ci.getPreprocessor()));
    return true;
  }
  void ExecuteAction() override {
    auto &pp = getCompilerInstance().getPreprocessor();
    pp.EnterMainSourceFile();
    Token token;
    do {
      pp.Lex(token);
      auto &sm = pp.getSourceManager();
      auto expansion = sm.getExpansionLoc(token.getLocation());
      if (token.isNot(tok::eof) && expansion.isValid() && sm.isWrittenInMainFile(expansion)) {
        emit(llvm::json::Object{{"kind", "root_token"}, {"spelling", pp.getSpelling(token)},
          {"path", sm.getFilename(expansion).str()}, {"offset", int64_t(sm.getFileOffset(expansion))},
          {"from_macro", token.getLocation().isMacroID()}});
      }
    } while (token.isNot(tok::eof));
  }
};
int main(int argc, char **argv) {
  if (argc < 2) return 2;
  std::vector<std::string> args{VORPAL_CLANG_DRIVER, "--driver-mode=cl"};
  for (int i = 1; i < argc; ++i) args.emplace_back(argv[i]);
  auto fs = llvm::makeIntrusiveRefCnt<ObservedFS>();
  if (argc == 3 && std::string(argv[1]) == "--fs-audit") {
    llvm::SmallString<256> cwd;
    fs->getCurrentWorkingDirectory();
    fs->setCurrentWorkingDirectory(argv[2]);
    cwd = "./optional.h";
    fs->makeAbsolute(cwd);
    fs->exists(cwd);
    bool local = false; fs->isLocal(argv[2],local);
    llvm::SmallString<256> real; fs->getRealPath(argv[2],real);
    std::error_code error; auto iterator = fs->dir_begin(argv[2],error);
    for (; !error && iterator != vfs::directory_iterator(); iterator.increment(error)) {}
    auto file = fs->openFileForRead("fallback.h");
    if (file) { (*file)->getName(); (*file)->status(); (*file)->getBuffer("fallback.h"); }
    emit(llvm::json::Object{{"kind", "fs_audit_complete"}, {"production_backend", false}});
    return error ? 1 : 0;
  }
  auto files = llvm::makeIntrusiveRefCnt<FileManager>(FileSystemOptions{}, fs);
  tooling::ToolInvocation invocation(std::move(args), std::make_unique<ObserveAction>(), files.get());
  bool ok = invocation.run();
  emit(llvm::json::Object{{"kind", "complete"}, {"success", ok}, {"production_backend", false}});
  return ok ? 0 : 1;
}
