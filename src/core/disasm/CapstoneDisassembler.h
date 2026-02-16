#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "core/loader/BinaryImage.h"
#include "core/model/FunctionInfo.h"
#include "util/Status.h"

namespace flutterdec::core::disasm {

struct AsmInstruction {
  uint64_t va = 0;
  std::string mnemonic;
  std::string op_str;
  std::string annotation;

  bool is_branch = false;
  bool is_conditional_branch = false;
  bool is_call = false;
  bool is_return = false;
  uint64_t branch_target = 0;
};

class CapstoneDisassembler {
 public:
  util::StatusOr<std::vector<AsmInstruction>> DisassembleFunction(
      const loader::BinaryImage& image, const model::FunctionInfo& fn) const;
};

}  // namespace flutterdec::core::disasm
