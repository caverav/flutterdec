#include "cli/commands/cmd_setup.h"

#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <string>

namespace flutterdec::cli::commands {
namespace {

std::string ParseHash(const std::vector<std::string>& args) {
  for (size_t i = 0; i + 1 < args.size(); ++i) {
    if (args[i] == "--dart-hash") {
      return args[i + 1];
    }
  }
  return "";
}

}  // namespace

int RunSetup(const std::vector<std::string>& args) {
  const std::string hash = ParseHash(args);
  if (hash.empty()) {
    std::cerr << "usage: flutterdec setup --dart-hash <hash>\n";
    return 2;
  }

  const auto fetch_script = std::filesystem::path("scripts/fetch_dart_sdk.py");
  const auto build_script = std::filesystem::path("scripts/build_dart_adapter.py");
  if (!std::filesystem::exists(fetch_script) || !std::filesystem::exists(build_script)) {
    std::cerr << "error: setup scripts missing in scripts/\n";
    return 1;
  }

  const std::string fetch_cmd = "python3 scripts/fetch_dart_sdk.py --dart-hash '" + hash + "'";
  const std::string build_cmd = "python3 scripts/build_dart_adapter.py --dart-hash '" + hash + "'";

  if (std::system(fetch_cmd.c_str()) != 0) {
    std::cerr << "error: fetch_dart_sdk.py failed\n";
    return 1;
  }
  if (std::system(build_cmd.c_str()) != 0) {
    std::cerr << "error: build_dart_adapter.py failed\n";
    return 1;
  }

  std::cout << "setup complete for hash: " << hash << "\n";
  return 0;
}

}  // namespace flutterdec::cli::commands
