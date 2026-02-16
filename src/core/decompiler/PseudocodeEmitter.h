#pragma once

#include <filesystem>
#include <string>
#include <vector>

#include "core/ir/IR.h"
#include "core/model/Program.h"
#include "util/Status.h"

namespace flutterdec::core::decompiler {

std::string decompile_to_pseudodart(const model::Program& program, const ir::FunctionIR& fn_ir);

util::Status EmitProgramPseudocode(const model::Program& program,
                                   const std::vector<ir::FunctionIR>& irs,
                                   const std::filesystem::path& out_dir,
                                   const std::string& focus_glob);

}  // namespace flutterdec::core::decompiler
