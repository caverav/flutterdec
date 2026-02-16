#pragma once

#include <filesystem>
#include <string>

#include "core/model/Program.h"
#include "util/Status.h"

namespace flutterdec::core::naming {

struct NamingConfig {
  bool enabled = true;
  std::filesystem::path mapping_path;
};

void apply_naming(model::Program& program, const NamingConfig& cfg);
util::Status WriteNamesMap(const model::Program& program, const std::filesystem::path& out_path);

}  // namespace flutterdec::core::naming
