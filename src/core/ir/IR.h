#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "core/model/FunctionInfo.h"

namespace flutterdec::core::ir {

enum class IROp {
  LoadConst,
  LoadMem,
  StoreMem,
  Call,
  Branch,
  Jump,
  Return,
  Other,
};

struct IRInstr {
  IROp op = IROp::Other;
  uint64_t va = 0;

  std::string dst;
  std::string src;
  std::string target;
  uint64_t imm = 0;
  std::vector<std::string> args;
};

struct BasicBlock {
  uint64_t start_va = 0;
  std::vector<IRInstr> instrs;
  std::vector<size_t> succs;
  std::vector<size_t> preds;
};

struct FunctionIR {
  model::FunctionInfo meta;
  std::vector<BasicBlock> blocks;
};

}  // namespace flutterdec::core::ir
