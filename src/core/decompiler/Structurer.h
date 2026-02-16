#pragma once

#include <string>
#include <vector>

#include "core/ir/IR.h"

namespace flutterdec::core::decompiler {

class Structurer {
 public:
  std::vector<std::string> BuildStructuredBody(const ir::FunctionIR& fn_ir) const;
};

}  // namespace flutterdec::core::decompiler
