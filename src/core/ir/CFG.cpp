#include "core/ir/CFG.h"

#include <algorithm>
#include <cstddef>
#include <unordered_map>
#include <unordered_set>
#include <vector>
#include <sstream>

namespace flutterdec::core::ir {

FunctionIR CFGBuilder::Build(const model::FunctionInfo& fn_meta,
                             const std::vector<disasm::AsmInstruction>& asm_instrs,
                             const std::vector<IRInstr>& llir) const {
  FunctionIR out;
  out.meta = fn_meta;
  if (asm_instrs.empty() || llir.empty()) {
    return out;
  }

  std::unordered_set<uint64_t> block_starts;
  block_starts.insert(asm_instrs.front().va);

  for (size_t i = 0; i < asm_instrs.size(); ++i) {
    const auto& ins = asm_instrs[i];
    if (ins.is_branch || ins.is_call) {
      if (ins.branch_target != 0) {
        block_starts.insert(ins.branch_target);
      }
      if (ins.is_conditional_branch && i + 1 < asm_instrs.size()) {
        block_starts.insert(asm_instrs[i + 1].va);
      }
    }
  }

  std::vector<size_t> starts_idx;
  starts_idx.reserve(block_starts.size());
  for (size_t i = 0; i < asm_instrs.size(); ++i) {
    if (block_starts.count(asm_instrs[i].va) != 0) {
      starts_idx.push_back(i);
    }
  }
  std::sort(starts_idx.begin(), starts_idx.end());

  for (size_t s = 0; s < starts_idx.size(); ++s) {
    const size_t begin = starts_idx[s];
    const size_t end = (s + 1 < starts_idx.size()) ? starts_idx[s + 1] : asm_instrs.size();

    BasicBlock bb;
    bb.start_va = asm_instrs[begin].va;
    for (size_t i = begin; i < end && i < llir.size(); ++i) {
      bb.instrs.push_back(llir[i]);
    }
    out.blocks.push_back(std::move(bb));
  }

  std::unordered_map<uint64_t, size_t> va_to_block;
  for (size_t i = 0; i < out.blocks.size(); ++i) {
    va_to_block[out.blocks[i].start_va] = i;
  }

  for (size_t i = 0; i < out.blocks.size(); ++i) {
    if (out.blocks[i].instrs.empty()) {
      continue;
    }

    const auto& term = out.blocks[i].instrs.back();
    if (term.op == IROp::Branch || term.op == IROp::Jump) {
      auto pos = term.target.find("0x");
      if (pos != std::string::npos) {
        uint64_t target_va = 0;
        std::stringstream ss;
        ss << std::hex << term.target.substr(pos);
        ss >> target_va;
        auto it = va_to_block.find(target_va);
        if (it != va_to_block.end()) {
          out.blocks[i].succs.push_back(it->second);
        }
      }
      if (term.op == IROp::Branch && i + 1 < out.blocks.size()) {
        out.blocks[i].succs.push_back(i + 1);
      }
    } else if (term.op != IROp::Return && i + 1 < out.blocks.size()) {
      out.blocks[i].succs.push_back(i + 1);
    }
  }

  for (size_t i = 0; i < out.blocks.size(); ++i) {
    for (size_t succ : out.blocks[i].succs) {
      if (succ < out.blocks.size()) {
        out.blocks[succ].preds.push_back(i);
      }
    }
  }

  return out;
}

}  // namespace flutterdec::core::ir
