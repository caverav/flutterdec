#include "test_common.h"

#include "core/decompiler/Structurer.h"
#include "core/ir/IR.h"

int main() {
  using namespace flutterdec;

  core::ir::FunctionIR fn;
  fn.meta.name_display = "f";

  core::ir::BasicBlock b0;
  b0.start_va = 0x1000;
  b0.succs = {1, 2};

  core::ir::BasicBlock b1;
  b1.start_va = 0x1004;
  b1.preds = {0};

  core::ir::BasicBlock b2;
  b2.start_va = 0x1008;
  b2.preds = {0};

  fn.blocks = {b0, b1, b2};

  core::decompiler::Structurer s;
  auto lines = s.BuildStructuredBody(fn);

  bool has_if = false;
  for (const auto& l : lines) {
    if (l.find("if") != std::string::npos) {
      has_if = true;
      break;
    }
  }
  TASSERT(has_if);
  return 0;
}
