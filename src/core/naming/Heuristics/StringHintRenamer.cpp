#include "core/naming/Heuristics/StringHintRenamer.h"

#include <algorithm>
#include <cctype>

namespace flutterdec::core::naming {
namespace {

std::string Sanitize(const std::string& s) {
  std::string out;
  out.reserve(s.size());
  for (char c : s) {
    if (std::isalnum(static_cast<unsigned char>(c))) {
      out.push_back(c);
    }
  }
  return out;
}

}  // namespace

std::unordered_map<std::string, std::string> StringHintRenameFunctions(const model::Program& program) {
  std::unordered_map<std::string, std::string> out;
  std::string hint;
  for (const auto& obj : program.object_pool.entries()) {
    if (obj.kind == model::ObjKind::String && obj.as_string.size() > 4) {
      hint = Sanitize(obj.as_string);
      if (!hint.empty()) {
        break;
      }
    }
  }
  if (hint.empty()) {
    return out;
  }

  for (const auto& fn : program.functions) {
    if (fn.name_obf.size() <= 2) {
      out[fn.name_obf] = "handle" + hint.substr(0, std::min<size_t>(hint.size(), 16));
    }
  }
  return out;
}

}  // namespace flutterdec::core::naming
