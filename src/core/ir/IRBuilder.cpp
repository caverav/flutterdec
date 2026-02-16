#include "core/ir/IRBuilder.h"

#include <string>

namespace flutterdec::core::ir {
namespace {

std::string ExtractCallName(const disasm::AsmInstruction& ins) {
  static constexpr const char* kPrefix = "call ";
  if (ins.annotation.rfind(kPrefix, 0) != 0) {
    return "";
  }
  return ins.annotation.substr(std::char_traits<char>::length(kPrefix));
}

}  // namespace

std::vector<IRInstr> IRBuilder::BuildLlir(const std::vector<disasm::AsmInstruction>& instrs) const {
  std::vector<IRInstr> out;
  out.reserve(instrs.size());

  for (const auto& ins : instrs) {
    IRInstr ir;
    ir.va = ins.va;

    if (ins.is_call) {
      ir.op = IROp::Call;
      ir.target = ExtractCallName(ins);
      if (ir.target.empty()) {
        ir.target = ins.op_str;
      }
      out.push_back(std::move(ir));
      continue;
    }

    if (ins.is_return) {
      ir.op = IROp::Return;
      out.push_back(std::move(ir));
      continue;
    }

    if (ins.is_branch) {
      ir.op = ins.is_conditional_branch ? IROp::Branch : IROp::Jump;
      ir.target = ins.op_str;
      out.push_back(std::move(ir));
      continue;
    }

    if (ins.mnemonic == "ldr" && ins.op_str.find("[pp") != std::string::npos) {
      ir.op = IROp::LoadConst;
      ir.src = ins.op_str;
      out.push_back(std::move(ir));
      continue;
    }

    ir.op = IROp::Other;
    ir.src = ins.mnemonic + " " + ins.op_str;
    out.push_back(std::move(ir));
  }

  return out;
}

}  // namespace flutterdec::core::ir
