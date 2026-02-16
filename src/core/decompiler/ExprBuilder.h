#pragma once

#include <string>
#include <vector>

namespace flutterdec::core::decompiler {

class ExprBuilder {
 public:
  std::vector<std::string> FoldSimpleExpressions(const std::vector<std::string>& body_lines) const;
};

}  // namespace flutterdec::core::decompiler
