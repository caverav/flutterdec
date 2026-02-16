#include "core/export/GhidraExporter.h"

#include <nlohmann/json.hpp>

#include "util/FileIO.h"

namespace flutterdec::core::exporting {

util::Status export_ghidra(const model::Program& program, const std::string& out_json) {
  nlohmann::json j;
  j["functions"] = nlohmann::json::array();
  for (const auto& fn : program.functions) {
    j["functions"].push_back({
        {"va", fn.entry_va},
        {"name", fn.name_display},
        {"class", fn.owner_class_display},
    });
  }
  return util::WriteFile(out_json, j.dump(2));
}

}  // namespace flutterdec::core::exporting
