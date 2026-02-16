#include "test_common.h"

#include <vector>

#include "core/disasm/CapstoneDisassembler.h"
#include "core/ir/CFG.h"
#include "core/ir/IRBuilder.h"

int main() {
  using namespace flutterdec;

  core::model::FunctionInfo fn;
  fn.id = 1;
  fn.entry_va = 0x1000;

  core::disasm::AsmInstruction a1{.va = 0x1000, .mnemonic = "b.eq", .op_str = "0x1010", .is_branch = true, .is_conditional_branch = true, .branch_target = 0x1010};
  core::disasm::AsmInstruction a2{.va = 0x1004, .mnemonic = "ret", .is_return = true};
  core::disasm::AsmInstruction a3{.va = 0x1010, .mnemonic = "ret", .is_return = true};

  std::vector<core::disasm::AsmInstruction> asm_instrs{a1, a2, a3};
  core::ir::IRBuilder irb;
  auto llir = irb.BuildLlir(asm_instrs);

  core::ir::CFGBuilder cfg;
  auto f = cfg.Build(fn, asm_instrs, llir);

  TASSERT(!f.blocks.empty());
  TASSERT(f.blocks.size() >= 2);
  return 0;
}
