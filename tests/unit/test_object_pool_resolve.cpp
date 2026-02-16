#include "test_common.h"

#include "core/model/ObjectPool.h"

int main() {
  flutterdec::core::model::ObjectPool pool;
  flutterdec::core::model::Obj a;
  a.kind = flutterdec::core::model::ObjKind::String;
  a.as_string = "hello";
  pool.Add(a);

  auto by_idx = pool.ResolveByIndex(0);
  TASSERT(by_idx.has_value());
  TASSERT(by_idx->as_string == "hello");

  auto by_off = pool.ResolveByOffset(0);
  TASSERT(by_off.has_value());
  TASSERT(by_off->as_string == "hello");

  auto bad = pool.ResolveByOffset(3);
  TASSERT(!bad.has_value());
  return 0;
}
