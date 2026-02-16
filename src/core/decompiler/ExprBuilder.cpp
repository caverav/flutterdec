#include "core/decompiler/ExprBuilder.h"

#include <string>
#include <vector>

namespace flutterdec::core::decompiler {

std::vector<std::string> ExprBuilder::FoldSimpleExpressions(
    const std::vector<std::string>& body_lines) const {
  std::vector<std::string> out;
  out.reserve(body_lines.size());
  for (const auto& line : body_lines) {
    out.push_back(line);
  }
  return out;
}

}  // namespace flutterdec::core::decompiler
