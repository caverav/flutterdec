#pragma once

#include <vector>

#include "core/disasm/CapstoneDisassembler.h"
#include "core/model/Program.h"

namespace flutterdec::core::disasm {

struct AnnotationResult {
  std::vector<uint64_t> call_targets;
};

class DartAbiAnnotator {
 public:
  AnnotationResult Annotate(const model::Program& program, const model::FunctionInfo& fn,
                            std::vector<AsmInstruction>* instrs) const;
};

}  // namespace flutterdec::core::disasm
