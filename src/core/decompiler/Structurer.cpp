#include "core/decompiler/Structurer.h"

#include <string>
#include <vector>

#include "core/ir/IR.h"

namespace flutterdec::core::decompiler {

std::vector<std::string> Structurer::BuildStructuredBody(const ir::FunctionIR& fn_ir) const {
  std::vector<std::string> lines;
  if (fn_ir.blocks.empty()) {
    lines.push_back("    return null;");
    return lines;
  }

  for (size_t b = 0; b < fn_ir.blocks.size(); ++b) {
    const auto& block = fn_ir.blocks[b];

    if (b > 0 && !block.preds.empty() && block.preds[0] > b) {
      lines.push_back("    while (true) {");
    }

    if (block.succs.size() == 2) {
      lines.push_back("    if (/* cond */) {");
      lines.push_back("      // branch true");
      lines.push_back("    } else {");
      lines.push_back("      // branch false");
      lines.push_back("    }");
    }

    for (const auto& instr : block.instrs) {
      switch (instr.op) {
        case ir::IROp::Call:
          lines.push_back("    " + instr.target + ";");
          break;
        case ir::IROp::Return:
          lines.push_back("    return null;");
          break;
        case ir::IROp::LoadConst:
          lines.push_back("    var t = " + instr.src + ";");
          break;
        default:
          break;
      }
    }

    if (b > 0 && !block.preds.empty() && block.preds[0] > b) {
      lines.push_back("    }");
    }
  }

  if (lines.empty()) {
    lines.push_back("    return null;");
  }
  return lines;
}

}  // namespace flutterdec::core::decompiler
