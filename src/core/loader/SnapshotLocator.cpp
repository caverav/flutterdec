#include "core/loader/SnapshotLocator.h"

#include <algorithm>
#include <optional>
#include <array>
#include <string>
#include <vector>

namespace flutterdec::core::loader {
namespace {

constexpr std::array<const char*, 4> kSymbolNames = {
    "_kDartVmSnapshotData",
    "_kDartVmSnapshotInstructions",
    "_kDartIsolateSnapshotData",
    "_kDartIsolateSnapshotInstructions",
};

constexpr std::array<uint8_t, 8> kSnapshotMagic = {'D', 'A', 'R', 'T', 'S', 'N', 'A', 'P'};

std::optional<SnapshotSpan> BuildSpanFromVa(const BinaryImage& image, uint64_t va, size_t requested_size) {
  auto off_or = image.VaToFileOffset(va);
  if (!off_or.ok()) {
    return std::nullopt;
  }
  const size_t offset = off_or.value();
  if (offset >= image.elf_bytes.size()) {
    return std::nullopt;
  }
  size_t max_size = image.elf_bytes.size() - offset;
  if (const auto* seg = image.FindSegmentForVa(va)) {
    const uint64_t delta = va - seg->va;
    if (delta < seg->size) {
      const uint64_t seg_remaining = seg->size - delta;
      max_size = std::min<size_t>(max_size, static_cast<size_t>(seg_remaining));
    }
  }
  if (max_size == 0) {
    return std::nullopt;
  }
  const size_t size = std::min(requested_size, max_size);
  return SnapshotSpan{offset, size, va};
}

std::vector<size_t> FindMagicOffsets(const BinaryImage& image) {
  std::vector<size_t> out;
  for (size_t i = 0; i + kSnapshotMagic.size() <= image.elf_bytes.size(); ++i) {
    if (std::equal(kSnapshotMagic.begin(), kSnapshotMagic.end(), image.elf_bytes.begin() + static_cast<long>(i))) {
      out.push_back(i);
    }
  }
  return out;
}

uint64_t OffsetToVa(const BinaryImage& image, size_t off) {
  for (const auto& seg : image.segments) {
    if (off >= seg.file_offset && off < seg.file_offset + seg.size) {
      return seg.va + (off - seg.file_offset);
    }
  }
  return 0;
}

}  // namespace

util::StatusOr<SnapshotRegions> locate_snapshots(const BinaryImage& image) {
  SnapshotRegions regions;
  regions.backing_image = &image;

  const auto vm_data_it = image.symbols.find(kSymbolNames[0]);
  const auto vm_instr_it = image.symbols.find(kSymbolNames[1]);
  const auto iso_data_it = image.symbols.find(kSymbolNames[2]);
  const auto iso_instr_it = image.symbols.find(kSymbolNames[3]);

  if (vm_data_it != image.symbols.end() && vm_instr_it != image.symbols.end() &&
      iso_data_it != image.symbols.end() && iso_instr_it != image.symbols.end()) {
    const auto vm_data_size =
        vm_data_it->second.size > 0 ? static_cast<size_t>(vm_data_it->second.size) : static_cast<size_t>(1 << 20);
    const auto vm_instr_size =
        vm_instr_it->second.size > 0 ? static_cast<size_t>(vm_instr_it->second.size) : static_cast<size_t>(1 << 20);
    const auto iso_data_size =
        iso_data_it->second.size > 0 ? static_cast<size_t>(iso_data_it->second.size) : static_cast<size_t>(4 << 20);
    const auto iso_instr_size =
        iso_instr_it->second.size > 0 ? static_cast<size_t>(iso_instr_it->second.size) : static_cast<size_t>(4 << 20);

    auto vm_data = BuildSpanFromVa(image, vm_data_it->second.va, vm_data_size);
    auto vm_instr = BuildSpanFromVa(image, vm_instr_it->second.va, vm_instr_size);
    auto iso_data = BuildSpanFromVa(image, iso_data_it->second.va, iso_data_size);
    auto iso_instr = BuildSpanFromVa(image, iso_instr_it->second.va, iso_instr_size);
    if (vm_data && vm_instr && iso_data && iso_instr) {
      regions.vm_data = *vm_data;
      regions.vm_instr = *vm_instr;
      regions.isolate_data = *iso_data;
      regions.isolate_instr = *iso_instr;
      regions.vm_instr_va = vm_instr->va;
      regions.isolate_instr_va = iso_instr->va;
      return regions;
    }
  }

  const auto magic = FindMagicOffsets(image);
  if (magic.size() >= 2) {
    regions.vm_data =
        SnapshotSpan{magic[0], std::min<size_t>(1 << 20, image.elf_bytes.size() - magic[0]), OffsetToVa(image, magic[0])};
    regions.isolate_data =
        SnapshotSpan{magic[1], std::min<size_t>(4 << 20, image.elf_bytes.size() - magic[1]), OffsetToVa(image, magic[1])};
  } else {
    const auto* rodata = image.FindSegmentByName(".rodata");
    if (!rodata) {
      uint64_t best_size = 0;
      for (const auto& seg : image.segments) {
        if (seg.readable && !seg.executable && seg.size > best_size) {
          rodata = &seg;
          best_size = seg.size;
        }
      }
    }
    if (!rodata) {
      for (const auto& seg : image.segments) {
        if (seg.size > 0) {
          rodata = &seg;
          break;
        }
      }
    }
    if (!rodata) {
      return util::Status::Error(util::ErrorCode::kNotFound, "unable to locate snapshot-like data segment");
    }

    if (rodata->file_offset >= image.elf_bytes.size()) {
      return util::Status::Error(util::ErrorCode::kParseError, "rodata segment points outside ELF bounds");
    }
    const size_t ro_size = static_cast<size_t>(std::min<uint64_t>(rodata->size, image.elf_bytes.size() - rodata->file_offset));
    const size_t vm_guess = std::min<size_t>(ro_size / 4, 1 << 20);
    const size_t iso_guess = std::min<size_t>(ro_size - vm_guess, 4 << 20);

    regions.vm_data = SnapshotSpan{rodata->file_offset, vm_guess, rodata->va};
    regions.isolate_data = SnapshotSpan{rodata->file_offset + vm_guess, iso_guess, rodata->va + vm_guess};
  }

  const auto* text = image.FindSegmentByName(".text");
  if (!text) {
    for (const auto& seg : image.segments) {
      if (seg.executable && seg.size > 0) {
        text = &seg;
        break;
      }
    }
  }
  if (!text) {
    return util::Status::Error(util::ErrorCode::kNotFound, "unable to locate executable code segment");
  }

  regions.vm_instr = SnapshotSpan{text->file_offset, static_cast<size_t>(std::min<uint64_t>(text->size, 1 << 20)), text->va};
  regions.isolate_instr = SnapshotSpan{text->file_offset, static_cast<size_t>(text->size), text->va};
  regions.vm_instr_va = text->va;
  regions.isolate_instr_va = text->va;
  return regions;
}

}  // namespace flutterdec::core::loader
