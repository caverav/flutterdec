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
  const auto pos = target.rfind("0x");
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
    size_t omitted_ops = 0;
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
          std::ostringstream cond;
          cond << "cond_0x" << std::hex << instr.va;
          const auto parsed_target = ParseHexTarget(instr.target);
          if (block.succs.size() == 2) {
            const auto true_label = labels[fn_ir.blocks[block.succs[0]].start_va];
            const auto false_label = labels[fn_ir.blocks[block.succs[1]].start_va];
            lines.push_back("    if (" + cond.str() + ") goto " + true_label + "; else goto " + false_label + ";");
          } else if (block.succs.size() == 1) {
            if (parsed_target.has_value()) {
              if (labels.find(*parsed_target) != labels.end()) {
                lines.push_back("    if (" + cond.str() + ") goto " + labels[*parsed_target] + ";");
              } else {
                std::ostringstream target;
                target << "0x" << std::hex << *parsed_target;
                lines.push_back("    if (" + cond.str() + ") goto " + target.str() + ";");
              }
            } else {
              lines.push_back("    /* unresolved branch at 0x" + [&] {
                               std::ostringstream oss;
                               oss << std::hex << instr.va;
                               return oss.str();
                             }() + " */");
            }
          } else {
            if (parsed_target.has_value()) {
              if (labels.find(*parsed_target) != labels.end()) {
                lines.push_back("    if (" + cond.str() + ") goto " + labels[*parsed_target] + ";");
              } else {
                std::ostringstream target;
                target << "0x" << std::hex << *parsed_target;
                lines.push_back("    if (" + cond.str() + ") goto " + target.str() + ";");
              }
            } else {
              lines.push_back("    /* unresolved branch at 0x" + [&] {
                               std::ostringstream oss;
                               oss << std::hex << instr.va;
                               return oss.str();
                             }() + " */");
            }
          }
          emitted_terminator = true;
          break;
        }
        case ir::IROp::Jump: {
          const auto t = ParseHexTarget(instr.target);
          if (t.has_value()) {
            if (labels.find(*t) != labels.end()) {
              lines.push_back("    goto " + labels[*t] + ";");
            } else {
              std::ostringstream target;
              target << "0x" << std::hex << *t;
              lines.push_back("    goto " + target.str() + ";");
            }
          } else if (block.succs.size() == 1) {
            lines.push_back("    goto " + labels[fn_ir.blocks[block.succs[0]].start_va] + ";");
          } else {
            lines.push_back("    /* unresolved jump at 0x" + [&] {
                             std::ostringstream oss;
                             oss << std::hex << instr.va;
                             return oss.str();
                           }() + " */");
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
          omitted_ops += 1;
          break;
      }
    }

    if (omitted_ops > 0) {
      lines.push_back("    /* " + std::to_string(omitted_ops) + " low-level ops omitted */");
    }

    if (!emitted_terminator) {
      if (block.succs.size() == 1 && (b + 1 >= fn_ir.blocks.size() || block.succs[0] != b + 1)) {
        lines.push_back("    goto " + labels[fn_ir.blocks[block.succs[0]].start_va] + ";");
      } else if (block.succs.size() > 1) {
        lines.push_back("    /* multiple successors; CFG kept unstructured */");
      }
    }
    lines.push_back("");
  }

  if (lines.empty()) {
    lines.push_back("    return /* empty */ null;");
  }
  return lines;
}

}  // namespace flutterdec::core::decompiler
