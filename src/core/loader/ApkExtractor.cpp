#include "core/loader/ApkExtractor.h"

#include <array>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <sstream>
#include <string>
#include <vector>

namespace flutterdec::core::loader {
namespace {

util::StatusOr<std::vector<std::string>> RunLines(const std::string& cmd) {
  std::array<char, 1024> buf{};
  std::vector<std::string> lines;
  FILE* pipe = popen(cmd.c_str(), "r");
  if (!pipe) {
    return util::Status::Error(util::ErrorCode::kExternalToolError, "failed to run command: " + cmd);
  }
  while (fgets(buf.data(), static_cast<int>(buf.size()), pipe) != nullptr) {
    std::string line(buf.data());
    if (!line.empty() && line.back() == '\n') {
      line.pop_back();
    }
    lines.push_back(std::move(line));
  }
  const int rc = pclose(pipe);
  if (rc != 0) {
    return util::Status::Error(util::ErrorCode::kExternalToolError, "command failed: " + cmd);
  }
  return lines;
}

}  // namespace

util::StatusOr<std::filesystem::path> ExtractLibappFromApk(const std::filesystem::path& apk_path) {
  if (!std::filesystem::exists(apk_path)) {
    return util::Status::Error(util::ErrorCode::kNotFound, "APK not found: " + apk_path.string());
  }

  const std::string list_cmd = "unzip -Z1 '" + apk_path.string() + "'";
  auto list_or = RunLines(list_cmd);
  if (!list_or.ok()) {
    return list_or.status();
  }

  static const std::vector<std::string> kAbiOrder = {
      "lib/arm64-v8a/libapp.so",
      "lib/armeabi-v7a/libapp.so",
      "lib/x86_64/libapp.so",
      "lib/x86/libapp.so",
  };

  std::string selected;
  for (const auto& abi : kAbiOrder) {
    for (const auto& line : list_or.value()) {
      if (line == abi) {
        selected = line;
        break;
      }
    }
    if (!selected.empty()) {
      break;
    }
  }

  if (selected.empty()) {
    for (const auto& line : list_or.value()) {
      if (line.size() >= 9 && line.find("/libapp.so") != std::string::npos) {
        selected = line;
        break;
      }
    }
  }

  if (selected.empty()) {
    return util::Status::Error(util::ErrorCode::kNotFound, "libapp.so not found in APK");
  }

  const auto tmp_dir = std::filesystem::temp_directory_path() / "flutterdec";
  std::error_code ec;
  std::filesystem::create_directories(tmp_dir, ec);
  const auto out_path = tmp_dir / (apk_path.stem().string() + "_libapp.so");

  const std::string extract_cmd = "unzip -p '" + apk_path.string() + "' '" + selected + "' > '" + out_path.string() + "'";
  int rc = std::system(extract_cmd.c_str());
  if (rc != 0 || !std::filesystem::exists(out_path)) {
    return util::Status::Error(util::ErrorCode::kExternalToolError,
                               "failed to extract libapp.so from APK via unzip");
  }
  return out_path;
}

}  // namespace flutterdec::core::loader
