#pragma once

#include <string>

#include "core/model/Program.h"
#include "util/Status.h"

namespace flutterdec::core::exporting {

util::Status export_ida(const model::Program& program, const std::string& out_py);

}  // namespace flutterdec::core::exporting
