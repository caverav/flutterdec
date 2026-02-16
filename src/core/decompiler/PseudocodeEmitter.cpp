#include "core/decompiler/PseudocodeEmitter.h"

#include <cctype>
#include <filesystem>
#include <fstream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

#include "core/decompiler/ExprBuilder.h"
#include "core/decompiler/Structurer.h"
#include "util/FileIO.h"

namespace flutterdec::core::decompiler {
namespace {

std::string RenderMethod(const ir::FunctionIR& fn_ir) {
  Structurer structurer;
  ExprBuilder expr;

  std::ostringstream out;
  out << "  dynamic " << fn_ir.meta.name_display << "(dynamic p0) { // obf:" << fn_ir.meta.name_obf << "\n";
  const auto body = structurer.BuildStructuredBody(fn_ir);
  for (const auto& line : expr.FoldSimpleExpressions(body)) {
    out << line << "\n";
  }
  out << "  }\n";
  return out.str();
}

}  // namespace

std::string decompile_to_pseudodart(const model::Program&, const ir::FunctionIR& fn_ir) {
  std::ostringstream out;
  out << "class " << fn_ir.meta.owner_class_display << " /* obf:" << fn_ir.meta.owner_class_obf << " */ {\n";
  out << RenderMethod(fn_ir);
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

  std::map<std::string, std::vector<std::string>> methods_by_class;
  std::map<std::string, std::string> obf_by_class;

  for (const auto& fn_ir : irs) {
    if (!focus_glob.empty()) {
      const auto has_focus = fn_ir.meta.owner_class_display.find(focus_glob) != std::string::npos ||
                             fn_ir.meta.owner_class_obf.find(focus_glob) != std::string::npos;
      if (!has_focus) {
        continue;
      }
    }
    methods_by_class[fn_ir.meta.owner_class_display].push_back(RenderMethod(fn_ir));
    if (obf_by_class.find(fn_ir.meta.owner_class_display) == obf_by_class.end()) {
      obf_by_class[fn_ir.meta.owner_class_display] = fn_ir.meta.owner_class_obf;
    }
  }

  for (const auto& [klass, methods] : methods_by_class) {
    std::string file_name = klass.empty() ? "global" : klass;
    for (auto& c : file_name) {
      if (!std::isalnum(static_cast<unsigned char>(c)) && c != '_') {
        c = '_';
      }
    }

    std::ostringstream ss;
    ss << "class " << (klass.empty() ? "Global" : klass) << " /* obf:" << obf_by_class[klass] << " */ {\n";
    for (const auto& method : methods) {
      ss << method << "\n";
    }
    ss << "}\n";

    const auto out_file = out_dir / (file_name + ".dart");
    auto write = util::WriteFile(out_file, ss.str());
    if (!write.ok()) {
      return write;
    }
  }

  return util::Status::Ok();
}

}  // namespace flutterdec::core::decompiler
