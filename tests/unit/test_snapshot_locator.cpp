#include "test_common.h"

#include <vector>

#include "core/loader/BinaryImage.h"
#include "core/loader/SnapshotLocator.h"

int main() {
  flutterdec::core::loader::BinaryImage img;
  img.elf_bytes.resize(4096, 0);

  const std::vector<uint8_t> magic = {'D', 'A', 'R', 'T', 'S', 'N', 'A', 'P'};
  std::copy(magic.begin(), magic.end(), img.elf_bytes.begin() + 128);
  std::copy(magic.begin(), magic.end(), img.elf_bytes.begin() + 512);

  flutterdec::core::loader::SegmentInfo text;
  text.name = ".text";
  text.va = 0x1000;
  text.file_offset = 1024;
  text.size = 512;
  img.segments.push_back(text);

  auto regions_or = flutterdec::core::loader::locate_snapshots(img);
  TASSERT(regions_or.ok());
  TASSERT(regions_or.value().vm_data.file_offset == 128);
  TASSERT(regions_or.value().isolate_data.file_offset == 512);
  TASSERT(regions_or.value().vm_instr.va == 0x1000);
  return 0;
}
