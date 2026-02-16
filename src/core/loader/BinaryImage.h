#pragma once

#include <cstddef>
#include <cstdint>
#include <string>
#include <unordered_map>
#include <vector>

#include "util/Status.h"

namespace flutterdec::core::loader {

struct SegmentInfo {
  std::string name;
  uint64_t va = 0;
  uint64_t size = 0;
  uint64_t file_offset = 0;
  bool readable = false;
  bool writable = false;
  bool executable = false;
};

struct SymbolInfo {
  std::string name;
  uint64_t va = 0;
  uint64_t size = 0;
};

struct BinaryImage {
  std::string input_path;
  std::string libapp_path;
  std::string platform = "android";
  std::string arch = "unknown";

  bool extracted_from_apk = false;
  bool has_symbol_table = false;

  std::vector<uint8_t> elf_bytes;
  std::vector<SegmentInfo> segments;
  std::unordered_map<std::string, SymbolInfo> symbols;

  [[nodiscard]] const SegmentInfo* FindSegmentByName(const std::string& needle) const {
    for (const auto& seg : segments) {
      if (seg.name == needle) {
        return &seg;
      }
    }
    return nullptr;
  }

  [[nodiscard]] const SegmentInfo* FindSegmentForVa(uint64_t va) const {
    for (const auto& seg : segments) {
      if (va >= seg.va && va < seg.va + seg.size) {
        return &seg;
      }
    }
    return nullptr;
  }

  [[nodiscard]] util::StatusOr<size_t> VaToFileOffset(uint64_t va) const {
    const SegmentInfo* seg = FindSegmentForVa(va);
    if (!seg) {
      return util::Status::Error(util::ErrorCode::kNotFound, "VA not in any segment");
    }
    const uint64_t delta = va - seg->va;
    const uint64_t file_off = seg->file_offset + delta;
    if (file_off >= elf_bytes.size()) {
      return util::Status::Error(util::ErrorCode::kParseError, "VA maps outside ELF bytes");
    }
    return static_cast<size_t>(file_off);
  }
};

util::StatusOr<BinaryImage> load_input(const std::string& path);

}  // namespace flutterdec::core::loader
