#include "core/disasm/DartAbiAnnotator.h"

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <optional>
#include <sstream>
#include <string>

namespace flutterdec::core::disasm {
namespace {

std::optional<uint64_t> ParseHexAfter(const std::string& s, const std::string& needle) {
  const auto pos = s.find(needle);
  if (pos == std::string::npos) {
    return std::nullopt;
  }
  const auto hx = s.find("0x", pos);
  if (hx == std::string::npos) {
    return std::nullopt;
  }
  std::stringstream ss;
  ss << std::hex << s.substr(hx);
  uint64_t out = 0;
  ss >> out;
  if (ss.fail()) {
    return std::nullopt;
  }
  return out;
}

std::string ObjToString(const model::Obj& o) {
  switch (o.kind) {
    case model::ObjKind::String:
      return "String(\"" + o.as_string + "\")";
    case model::ObjKind::Int:
      return "Int(" + std::to_string(o.as_int) + ")";
    case model::ObjKind::Double:
      return "Double(" + std::to_string(o.as_double) + ")";
    case model::ObjKind::Type:
      return "Type(" + o.as_string + ")";
    case model::ObjKind::FunctionRef:
      return "FunctionRef(0x" + std::to_string(o.ref_va) + ")";
    case model::ObjKind::ClassRef:
      return "ClassRef(" + o.as_string + ")";
    case model::ObjKind::Unknown:
      return "Unknown";
  }
  return "Unknown";
}

}  // namespace

AnnotationResult DartAbiAnnotator::Annotate(const model::Program& program, const model::FunctionInfo&,
                                            std::vector<AsmInstruction>* instrs) const {
  AnnotationResult result;
  if (!instrs) {
    return result;
  }

  for (auto& ins : *instrs) {
    if ((ins.mnemonic == "ldr" || ins.mnemonic == "ldur") && ins.op_str.find("[pp") != std::string::npos) {
      auto off = ParseHexAfter(ins.op_str, "#");
      if (off) {
        auto obj = program.object_pool.ResolveByOffset(*off);
        if (obj) {
          ins.annotation = "ObjPool[" + std::to_string(*off / 8) + "] = " + ObjToString(*obj);
        } else {
          ins.annotation = "ObjPool unresolved offset #0x" + std::to_string(*off);
        }
      }
    }

    if (ins.is_call && ins.branch_target != 0) {
      result.call_targets.push_back(ins.branch_target);
      auto it = program.addr_to_function.find(ins.branch_target);
      if (it != program.addr_to_function.end()) {
        const auto& callee = program.functions[it->second];
        ins.annotation = "call " + callee.owner_class_display + "." + callee.name_display;
      }
    }
  }

  std::sort(result.call_targets.begin(), result.call_targets.end());
  result.call_targets.erase(std::unique(result.call_targets.begin(), result.call_targets.end()),
                            result.call_targets.end());
  return result;
}

}  // namespace flutterdec::core::disasm
