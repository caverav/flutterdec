#include "cli/commands/cmd_export_ghidra.h"

#include <iostream>
#include <optional>

#include "cli/commands/Common.h"
#include "core/export/GhidraExporter.h"

namespace flutterdec::cli::commands {
namespace {

std::optional<std::string> ParseOut(const std::vector<std::string>& args) {
  for (size_t i = 1; i + 1 < args.size(); ++i) {
    if (args[i] == "-o") {
      return args[i + 1];
    }
  }
  return std::nullopt;
}

bool HasExperimentalHeuristic(const std::vector<std::string>& args) {
  for (const auto& arg : args) {
    if (arg == "--experimental-heuristic") {
      return true;
    }
  }
  return false;
}

}  // namespace

int RunExportGhidra(const std::vector<std::string>& args) {
  if (args.empty()) {
    std::cerr << "usage: flutterdec export ghidra <libapp.so|apk> -o ghidra.json\n";
    return 2;
  }
  auto out = ParseOut(args);
  if (!out.has_value()) {
    std::cerr << "error: missing -o output path\n";
    return 2;
  }

  auto ctx_or = BuildPipeline(args[0], true, HasExperimentalHeuristic(args));
  if (!ctx_or.ok()) {
    std::cerr << "error: " << ctx_or.status().message << "\n";
    return 1;
  }

  auto st = core::exporting::export_ghidra(ctx_or.value().program, *out);
  if (!st.ok()) {
    std::cerr << "error: " << st.message << "\n";
    return 1;
  }

  std::cout << "ghidra export written: " << *out << "\n";
  return 0;
}

}  // namespace flutterdec::cli::commands
