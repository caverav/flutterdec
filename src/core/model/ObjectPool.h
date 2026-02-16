#pragma once

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace flutterdec::core::model {

enum class ObjKind { String, Int, Double, Type, FunctionRef, ClassRef, Unknown };

struct Obj {
  ObjKind kind = ObjKind::Unknown;
  std::string as_string;
  int64_t as_int = 0;
  double as_double = 0.0;
  uint64_t ref_va = 0;
};

class ObjectPool {
 public:
  void Add(Obj obj);
  std::optional<Obj> ResolveByOffset(uint64_t pp_offset) const;
  std::optional<Obj> ResolveByIndex(size_t i) const;
  [[nodiscard]] size_t size() const { return objects_.size(); }
  [[nodiscard]] const std::vector<Obj>& entries() const { return objects_; }

 private:
  std::vector<Obj> objects_;
};

}  // namespace flutterdec::core::model
