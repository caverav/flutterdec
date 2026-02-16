#include "core/naming/NameResolver.h"

#include <fstream>
#include <unordered_map>

#include <nlohmann/json.hpp>

#include "core/naming/Heuristics/FrameworkMatcher.h"
#include "core/naming/Heuristics/StringHintRenamer.h"
#include "core/naming/Heuristics/UsagePatternRenamer.h"
#include "util/FileIO.h"

namespace flutterdec::core::naming {
namespace {

void MergeSuggestions(std::unordered_map<std::string, std::string>* base,
                      const std::unordered_map<std::string, std::string>& incoming) {
  for (const auto& [k, v] : incoming) {
    if (base->find(k) == base->end()) {
      (*base)[k] = v;
    }
  }
}

void ApplyUserMapping(std::unordered_map<std::string, std::string>* names,
                      const std::filesystem::path& mapping_path) {
  if (mapping_path.empty() || !std::filesystem::exists(mapping_path)) {
    return;
  }
  std::ifstream in(mapping_path);
  if (!in) {
    return;
  }
  nlohmann::json j;
  try {
    in >> j;
  } catch (...) {
    return;
  }

  for (auto& [obf, mapped] : j.value("functions", nlohmann::json::object()).items()) {
    const auto display = mapped.value("display", "");
    if (!display.empty()) {
      (*names)[obf] = display;
    }
  }
}

}  // namespace

void apply_naming(model::Program& program, const NamingConfig& cfg) {
  if (!cfg.enabled) {
    return;
  }

  std::unordered_map<std::string, std::string> suggestions;
  MergeSuggestions(&suggestions, FrameworkMatchFunctionNames(program));
  MergeSuggestions(&suggestions, StringHintRenameFunctions(program));
  MergeSuggestions(&suggestions, UsagePatternRenameFunctions(program));
  ApplyUserMapping(&suggestions, cfg.mapping_path);

  for (auto& fn : program.functions) {
    auto it = suggestions.find(fn.name_obf);
    if (it != suggestions.end()) {
      fn.name_display = it->second;
    }
  }

  for (auto& cls : program.classes) {
    cls.name_display = cls.name_obf;
  }

  program.StableSort();
}

util::Status WriteNamesMap(const model::Program& program, const std::filesystem::path& out_path) {
  nlohmann::json j;
  j["classes"] = nlohmann::json::object();
  j["functions"] = nlohmann::json::object();

  for (const auto& cls : program.classes) {
    j["classes"][cls.name_obf] = {
        {"display", cls.name_display},
        {"confidence", 1.0},
        {"reason", "preserved"},
    };
  }

  for (const auto& fn : program.functions) {
    j["functions"][fn.name_obf] = {
        {"display", fn.name_display},
        {"confidence", fn.name_display == fn.name_obf ? 0.0 : 0.5},
        {"reason", fn.name_display == fn.name_obf ? "unresolved" : "heuristic"},
    };
  }

  auto parent = out_path.parent_path();
  if (!parent.empty()) {
    auto st = util::EnsureDir(parent);
    if (!st.ok()) {
      return st;
    }
  }
  return util::WriteFile(out_path, j.dump(2));
}

}  // namespace flutterdec::core::naming
