#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace flutterdec::core::model {

struct FunctionInfo {
  size_t id = 0;
  std::string name_obf;
  std::string name_display;
  std::string owner_class_obf;
  std::string owner_class_display;

  uint64_t entry_va = 0;
  uint64_t size_bytes = 0;
  uint64_t code_section_va = 0;

  size_t object_pool_base = 0;
  std::vector<uint64_t> calls;
  bool size_estimated = false;
};

}  // namespace flutterdec::core::model
