#pragma once

#include <vector>

#include "core/disasm/CapstoneDisassembler.h"
#include "core/ir/IR.h"

namespace flutterdec::core::ir {

class CFGBuilder {
 public:
  FunctionIR Build(const model::FunctionInfo& fn_meta,
                   const std::vector<disasm::AsmInstruction>& asm_instrs,
                   const std::vector<IRInstr>& llir) const;
};

}  // namespace flutterdec::core::ir
