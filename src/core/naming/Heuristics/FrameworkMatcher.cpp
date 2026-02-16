#include "core/naming/Heuristics/FrameworkMatcher.h"

namespace flutterdec::core::naming {

std::unordered_map<std::string, std::string> FrameworkMatchFunctionNames(const model::Program& program) {
  std::unordered_map<std::string, std::string> out;
  for (const auto& cls : program.classes) {
    const bool widget_like = cls.superclass.find("Widget") != std::string::npos ||
                             cls.superclass.find("State") != std::string::npos;
    if (!widget_like) {
      continue;
    }
    for (const auto& fn : program.functions) {
      if (fn.owner_class_obf != cls.name_obf) {
        continue;
      }
      if (fn.name_obf.size() <= 2) {
        out[fn.name_obf] = "build";
      }
    }
  }
  return out;
}

}  // namespace flutterdec::core::naming
