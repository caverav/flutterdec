#pragma once

#include <vector>

#include "core/disasm/CapstoneDisassembler.h"
#include "core/ir/IR.h"

namespace flutterdec::core::ir {

class IRBuilder {
 public:
  std::vector<IRInstr> BuildLlir(const std::vector<disasm::AsmInstruction>& instrs) const;
};

}  // namespace flutterdec::core::ir
