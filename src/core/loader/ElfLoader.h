#pragma once

#include <filesystem>

#include "core/loader/BinaryImage.h"

namespace flutterdec::core::loader {

util::StatusOr<BinaryImage> LoadElfImage(const std::filesystem::path& libapp_path);

}  // namespace flutterdec::core::loader
