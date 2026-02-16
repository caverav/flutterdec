#pragma once

#include <cstddef>
#include <string>

namespace flutterdec::core::model {

struct LibraryInfo {
  size_t id = 0;
  std::string uri;
  std::string name_display;
};

struct ClassInfo {
  size_t id = 0;
  std::string name_obf;
  std::string name_display;
  std::string superclass;
  std::string library_uri;
};

}  // namespace flutterdec::core::model
