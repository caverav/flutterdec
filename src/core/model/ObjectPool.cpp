#include "core/model/ObjectPool.h"

namespace flutterdec::core::model {

void ObjectPool::Add(Obj obj) { objects_.push_back(std::move(obj)); }

std::optional<Obj> ObjectPool::ResolveByOffset(uint64_t pp_offset) const {
  if (pp_offset % 8 != 0) {
    return std::nullopt;
  }
  return ResolveByIndex(static_cast<size_t>(pp_offset / 8));
}

std::optional<Obj> ObjectPool::ResolveByIndex(size_t i) const {
  if (i >= objects_.size()) {
    return std::nullopt;
  }
  return objects_[i];
}

}  // namespace flutterdec::core::model
