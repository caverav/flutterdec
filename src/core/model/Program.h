#pragma once

#include <cstdint>
#include <string>
#include <unordered_map>
#include <vector>

#include "core/model/ClassInfo.h"
#include "core/model/FunctionInfo.h"
#include "core/model/ObjectPool.h"

namespace flutterdec::core::model {

struct Program {
  std::string input_path;
  std::string platform = "android";
  std::string arch = "arm64";
  std::string dart_version;
  std::string snapshot_hash;

  ObjectPool object_pool;

  std::vector<LibraryInfo> libraries;
  std::vector<ClassInfo> classes;
  std::vector<FunctionInfo> functions;

  std::unordered_map<uint64_t, size_t> addr_to_function;

  void RebuildIndexes();
  void StableSort();
};

}  // namespace flutterdec::core::model
