#include "core/decompiler/Structurer.h"

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <iomanip>
#include <optional>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

#include "core/ir/IR.h"

namespace flutterdec::core::decompiler {
namespace {

std::optional<uint64_t> ParseHexTarget(const std::string& target) {
  const auto pos = target.find("0x");
  if (pos == std::string::npos) {
    return std::nullopt;
  }
  std::stringstream ss;
  ss << std::hex << target.substr(pos);
  uint64_t out = 0;
  ss >> out;
  if (ss.fail()) {
    return std::nullopt;
  }
  return out;
}

std::string LabelForVa(uint64_t va) {
  std::ostringstream oss;
  oss << "block_0x" << std::hex << va;
  return oss.str();
}

bool IsRegisterName(const std::string& token) {
  if (token.size() < 2) {
    return false;
  }
  if (token[0] != 'x' && token[0] != 'w') {
    return false;
  }
  for (size_t i = 1; i < token.size(); ++i) {
    if (!std::isdigit(static_cast<unsigned char>(token[i]))) {
      return false;
    }
  }
  return true;
}

std::string NormalizeCallTarget(std::string t) {
  if (!t.empty() && t[0] == '#') {
    t.erase(t.begin());
  }
  return t;
}

}  // namespace

std::vector<std::string> Structurer::BuildStructuredBody(const ir::FunctionIR& fn_ir) const {
  std::vector<std::string> lines;
  if (fn_ir.blocks.empty()) {
    lines.push_back("    return /* empty */ null;");
    return lines;
  }

  std::unordered_map<uint64_t, std::string> labels;
  for (const auto& block : fn_ir.blocks) {
    labels[block.start_va] = LabelForVa(block.start_va);
  }

  for (size_t b = 0; b < fn_ir.blocks.size(); ++b) {
    const auto& block = fn_ir.blocks[b];
    lines.push_back("    " + labels[block.start_va] + ":");

    bool emitted_terminator = false;
    for (const auto& instr : block.instrs) {
      switch (instr.op) {
        case ir::IROp::Call:
          if (instr.target.empty()) {
            lines.push_back("    call(/*unknown*/);");
          } else {
            const std::string target = NormalizeCallTarget(instr.target);
            if (IsRegisterName(target)) {
              lines.push_back("    call_indirect(" + target + ");");
            } else {
              lines.push_back("    call(" + target + ");");
            }
          }
          break;
        case ir::IROp::Branch: {
          const auto t = ParseHexTarget(instr.target);
          if (block.succs.size() == 2) {
            const auto true_label = labels[fn_ir.blocks[block.succs[0]].start_va];
            const auto false_label = labels[fn_ir.blocks[block.succs[1]].start_va];
            std::ostringstream cond;
            cond << "cond_0x" << std::hex << instr.va;
            lines.push_back("    if (" + cond.str() + ") goto " + true_label + "; else goto " + false_label + ";");
          } else if (t.has_value() && labels.find(*t) != labels.end()) {
            lines.push_back("    if (cond) goto " + labels[*t] + ";");
          } else {
            lines.push_back("    if (cond) goto /*unknown*/;");
          }
          emitted_terminator = true;
          break;
        }
        case ir::IROp::Jump: {
          const auto t = ParseHexTarget(instr.target);
          if (t.has_value() && labels.find(*t) != labels.end()) {
            lines.push_back("    goto " + labels[*t] + ";");
          } else {
            lines.push_back("    goto /*unknown*/;");
          }
          emitted_terminator = true;
          break;
        }
        case ir::IROp::Return:
          lines.push_back("    return /* unknown */ null;");
          emitted_terminator = true;
          break;
        case ir::IROp::LoadConst:
          lines.push_back("    var t = " + instr.src + ";");
          break;
        default:
          break;
      }
    }

    if (!emitted_terminator && b + 1 < fn_ir.blocks.size()) {
      lines.push_back("    goto " + labels[fn_ir.blocks[b + 1].start_va] + ";");
    }
    lines.push_back("");
  }

  if (lines.empty()) {
    lines.push_back("    return /* empty */ null;");
  }
  return lines;
}

}  // namespace flutterdec::core::decompiler
