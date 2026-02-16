#pragma once

#include <filesystem>
#include <string>

#include "util/Status.h"

namespace flutterdec::core::loader {

util::StatusOr<std::filesystem::path> ExtractLibappFromApk(const std::filesystem::path& apk_path);

}  // namespace flutterdec::core::loader
