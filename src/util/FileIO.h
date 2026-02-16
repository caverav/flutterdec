#pragma once

#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

#include "util/Status.h"

namespace flutterdec::util {

StatusOr<std::vector<uint8_t>> ReadFile(const std::filesystem::path& path);
Status WriteFile(const std::filesystem::path& path, const std::string& data);
Status WriteFileBytes(const std::filesystem::path& path, const std::vector<uint8_t>& data);
Status EnsureDir(const std::filesystem::path& path);

}  // namespace flutterdec::util
