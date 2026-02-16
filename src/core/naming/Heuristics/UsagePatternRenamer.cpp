#include "core/naming/Heuristics/UsagePatternRenamer.h"

#include <unordered_map>

namespace flutterdec::core::naming {

std::unordered_map<std::string, std::string> UsagePatternRenameFunctions(const model::Program& program) {
  std::unordered_map<uint64_t, size_t> callee_count;
  for (const auto& fn : program.functions) {
    for (uint64_t c : fn.calls) {
      callee_count[c] += 1;
    }
  }

  std::unordered_map<std::string, std::string> out;
  for (const auto& fn : program.functions) {
    auto it = callee_count.find(fn.entry_va);
    if (it != callee_count.end() && it->second == 1 && fn.name_obf.size() <= 2) {
      out[fn.name_obf] = "helper_" + std::to_string(fn.id);
    }
  }
  return out;
}

}  // namespace flutterdec::core::naming
