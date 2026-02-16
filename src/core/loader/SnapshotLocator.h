#pragma once

#include <cstddef>
#include <cstdint>
#include <string>

#include "core/loader/BinaryImage.h"
#include "util/Status.h"

namespace flutterdec::core::loader {

struct SnapshotSpan {
  size_t file_offset = 0;
  size_t size = 0;
  uint64_t va = 0;
};

struct SnapshotRegions {
  const BinaryImage* backing_image = nullptr;
  SnapshotSpan vm_data;
  SnapshotSpan isolate_data;
  SnapshotSpan vm_instr;
  SnapshotSpan isolate_instr;

  uint64_t vm_instr_va = 0;
  uint64_t isolate_instr_va = 0;
};

util::StatusOr<SnapshotRegions> locate_snapshots(const BinaryImage& image);

}  // namespace flutterdec::core::loader
