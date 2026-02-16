#include "core/export/JsonReport.h"

#include <nlohmann/json.hpp>

#include "util/FileIO.h"

namespace flutterdec::core::exporting {

util::Status WriteProgramReport(const model::Program& program, const std::filesystem::path& out_path) {
  nlohmann::json j;
  j["input_path"] = program.input_path;
  j["platform"] = program.platform;
  j["arch"] = program.arch;
  j["dart_version"] = program.dart_version;
  j["snapshot_hash"] = program.snapshot_hash;
  j["counts"] = {
      {"libraries", program.libraries.size()},
      {"classes", program.classes.size()},
      {"functions", program.functions.size()},
      {"object_pool", program.object_pool.size()},
  };

  j["classes"] = nlohmann::json::array();
  for (const auto& c : program.classes) {
    j["classes"].push_back({
        {"id", c.id},
        {"name_obf", c.name_obf},
        {"name_display", c.name_display},
        {"super", c.superclass},
        {"lib", c.library_uri},
    });
  }

  j["functions"] = nlohmann::json::array();
  for (const auto& f : program.functions) {
    j["functions"].push_back({
        {"id", f.id},
        {"name_obf", f.name_obf},
        {"name_display", f.name_display},
        {"owner_class_obf", f.owner_class_obf},
        {"owner_class_display", f.owner_class_display},
        {"entry_va", f.entry_va},
        {"size_bytes", f.size_bytes},
        {"size_estimated", f.size_estimated},
        {"calls", f.calls},
    });
  }

  auto st = util::EnsureDir(out_path.parent_path());
  if (!st.ok()) {
    return st;
  }
  return util::WriteFile(out_path, j.dump(2));
}

}  // namespace flutterdec::core::exporting
