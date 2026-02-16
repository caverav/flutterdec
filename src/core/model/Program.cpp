#include "core/model/Program.h"

#include <algorithm>

namespace flutterdec::core::model {

void Program::RebuildIndexes() {
  addr_to_function.clear();
  for (size_t i = 0; i < functions.size(); ++i) {
    addr_to_function[functions[i].entry_va] = i;
  }
}

void Program::StableSort() {
  std::sort(libraries.begin(), libraries.end(), [](const LibraryInfo& a, const LibraryInfo& b) {
    return a.id < b.id;
  });
  std::sort(classes.begin(), classes.end(), [](const ClassInfo& a, const ClassInfo& b) {
    return a.id < b.id;
  });
  std::sort(functions.begin(), functions.end(), [](const FunctionInfo& a, const FunctionInfo& b) {
    if (a.owner_class_display == b.owner_class_display) {
      return a.id < b.id;
    }
    return a.owner_class_display < b.owner_class_display;
  });
  RebuildIndexes();
}

}  // namespace flutterdec::core::model
