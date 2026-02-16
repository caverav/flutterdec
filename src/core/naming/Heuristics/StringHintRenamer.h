#pragma once

#include <string>
#include <unordered_map>

#include "core/model/Program.h"

namespace flutterdec::core::naming {

std::unordered_map<std::string, std::string> StringHintRenameFunctions(const model::Program& program);

}  // namespace flutterdec::core::naming
