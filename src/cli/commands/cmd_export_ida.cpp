#include "cli/commands/cmd_export_ida.h"

#include <iostream>
#include <optional>

#include "cli/commands/Common.h"
#include "core/export/IdaExporter.h"

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

}  // namespace

int RunExportIda(const std::vector<std::string>& args) {
  if (args.empty()) {
    std::cerr << "usage: flutterdec export ida <libapp.so|apk> -o ida.py\n";
    return 2;
  }
  auto out = ParseOut(args);
  if (!out.has_value()) {
    std::cerr << "error: missing -o output path\n";
    return 2;
  }

  auto ctx_or = BuildPipeline(args[0], true);
  if (!ctx_or.ok()) {
    std::cerr << "error: " << ctx_or.status().message << "\n";
    return 1;
  }

  auto st = core::exporting::export_ida(ctx_or.value().program, *out);
  if (!st.ok()) {
    std::cerr << "error: " << st.message << "\n";
    return 1;
  }

  std::cout << "ida export written: " << *out << "\n";
  return 0;
}

}  // namespace flutterdec::cli::commands
