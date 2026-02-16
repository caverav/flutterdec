#include "test_common.h"

#include <vector>

#include "core/disasm/DartAbiAnnotator.h"

int main() {
  using namespace flutterdec;

  core::model::Program p;
  core::model::Obj o;
  o.kind = core::model::ObjKind::Type;
  o.as_string = "void?";
  p.object_pool.Add(o);

  core::model::FunctionInfo fn;
  fn.id = 1;
  fn.name_display = "f";
  fn.owner_class_display = "A";

  core::disasm::AsmInstruction i1;
  i1.va = 0x100;
  i1.mnemonic = "ldr";
  i1.op_str = "x0, [pp, #0x0]";

  core::disasm::AsmInstruction i2;
  i2.va = 0x104;
  i2.mnemonic = "bl";
  i2.op_str = "0x200";
  i2.is_call = true;
  i2.branch_target = 0x200;

  std::vector<core::disasm::AsmInstruction> v{i1, i2};
  core::disasm::DartAbiAnnotator ann;
  auto res = ann.Annotate(p, fn, &v);

  TASSERT(v[0].annotation.find("ObjPool") != std::string::npos);
  TASSERT(res.call_targets.size() == 1);
  return 0;
}
