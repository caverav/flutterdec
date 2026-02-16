#include <iostream>
#include <string>
#include <vector>

#include "cli/commands/cmd_decompile.h"
#include "cli/commands/cmd_export_ghidra.h"
#include "cli/commands/cmd_export_ida.h"
#include "cli/commands/cmd_info.h"
#include "cli/commands/cmd_setup.h"

namespace {

std::vector<std::string> SliceArgs(int argc, char** argv, int start) {
  std::vector<std::string> out;
  for (int i = start; i < argc; ++i) {
    out.emplace_back(argv[i]);
  }
  return out;
}

void PrintUsage() {
  std::cout << "flutterdec commands:\n"
            << "  flutterdec decompile <libapp.so|apk> -o out/ [options]\n"
            << "  flutterdec info <libapp.so|apk>\n"
            << "  flutterdec export ida <libapp.so|apk> -o ida.py\n"
            << "  flutterdec export ghidra <libapp.so|apk> -o ghidra.json\n"
            << "  flutterdec setup --dart-hash <hash>\n";
}

}  // namespace

int main(int argc, char** argv) {
  if (argc < 2) {
    PrintUsage();
    return 2;
  }

  const std::string cmd = argv[1];
  if (cmd == "info") {
    return flutterdec::cli::commands::RunInfo(SliceArgs(argc, argv, 2));
  }
  if (cmd == "decompile") {
    return flutterdec::cli::commands::RunDecompile(SliceArgs(argc, argv, 2));
  }
  if (cmd == "setup") {
    return flutterdec::cli::commands::RunSetup(SliceArgs(argc, argv, 2));
  }
  if (cmd == "export") {
    if (argc < 4) {
      PrintUsage();
      return 2;
    }
    const std::string kind = argv[2];
    if (kind == "ida") {
      return flutterdec::cli::commands::RunExportIda(SliceArgs(argc, argv, 3));
    }
    if (kind == "ghidra") {
      return flutterdec::cli::commands::RunExportGhidra(SliceArgs(argc, argv, 3));
    }
  }

  PrintUsage();
  return 2;
}
