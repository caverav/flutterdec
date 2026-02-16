#include "core/decompiler/PseudocodeEmitter.h"

#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

#include "core/decompiler/ExprBuilder.h"
#include "core/decompiler/Structurer.h"
#include "util/FileIO.h"

namespace flutterdec::core::decompiler {

std::string decompile_to_pseudodart(const model::Program&, const ir::FunctionIR& fn_ir) {
  Structurer structurer;
  ExprBuilder expr;

  std::ostringstream out;
  out << "class " << fn_ir.meta.owner_class_display << " /* obf:" << fn_ir.meta.owner_class_obf << " */ {\n";
  out << "  dynamic " << fn_ir.meta.name_display << "(dynamic p0)";
  out << " { // obf:" << fn_ir.meta.name_obf << "\n";

  const auto body = structurer.BuildStructuredBody(fn_ir);
  for (const auto& line : expr.FoldSimpleExpressions(body)) {
    out << line << "\n";
  }

  out << "  }\n";
  out << "}\n";
  return out.str();
}

util::Status EmitProgramPseudocode(const model::Program& program,
                                   const std::vector<ir::FunctionIR>& irs,
                                   const std::filesystem::path& out_dir,
                                   const std::string& focus_glob) {
  auto st = util::EnsureDir(out_dir);
  if (!st.ok()) {
    return st;
  }

  std::unordered_map<std::string, std::ostringstream> by_class;

  for (const auto& fn_ir : irs) {
    if (!focus_glob.empty()) {
      const auto has_focus = fn_ir.meta.owner_class_display.find(focus_glob) != std::string::npos ||
                             fn_ir.meta.owner_class_obf.find(focus_glob) != std::string::npos;
      if (!has_focus) {
        continue;
      }
    }
    by_class[fn_ir.meta.owner_class_display] << decompile_to_pseudodart(program, fn_ir) << "\n";
  }

  for (const auto& [klass, ss] : by_class) {
    std::string file_name = klass.empty() ? "global" : klass;
    for (auto& c : file_name) {
      if (!std::isalnum(static_cast<unsigned char>(c)) && c != '_') {
        c = '_';
      }
    }
    const auto out_file = out_dir / (file_name + ".dart");
    auto write = util::WriteFile(out_file, ss.str());
    if (!write.ok()) {
      return write;
    }
  }

  return util::Status::Ok();
}

}  // namespace flutterdec::core::decompiler
