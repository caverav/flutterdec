#pragma once

#include <filesystem>

#include "core/model/Program.h"
#include "util/Status.h"

namespace flutterdec::core::exporting {

util::Status WriteProgramReport(const model::Program& program, const std::filesystem::path& out_path);

}  // namespace flutterdec::core::exporting
