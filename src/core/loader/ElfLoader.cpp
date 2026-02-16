#include "core/loader/ElfLoader.h"

#include <algorithm>
#include <filesystem>
#include <string>
#include <vector>

#include "core/loader/ApkExtractor.h"
#include "util/FileIO.h"

#if __has_include(<elfio/elfio.hpp>)
#include <elfio/elfio.hpp>
#define FLUTTERDEC_HAVE_ELFIO 1
#else
#define FLUTTERDEC_HAVE_ELFIO 0
#endif

namespace flutterdec::core::loader {
namespace {

std::string DetectArchFromElfMachine(uint16_t machine) {
  switch (machine) {
    case 183:
      return "arm64";
    case 40:
      return "arm";
    case 62:
      return "x86_64";
    default:
      return "unknown";
  }
}

bool LooksLikeApkPath(const std::filesystem::path& p) {
  return p.extension() == ".apk";
}

}  // namespace

util::StatusOr<BinaryImage> LoadElfImage(const std::filesystem::path& libapp_path) {
  auto bytes_or = util::ReadFile(libapp_path);
  if (!bytes_or.ok()) {
    return bytes_or.status();
  }

  BinaryImage image;
  image.libapp_path = libapp_path.string();
  image.elf_bytes = std::move(bytes_or.value());

#if FLUTTERDEC_HAVE_ELFIO
  ELFIO::elfio reader;
  if (!reader.load(libapp_path.string())) {
    return util::Status::Error(util::ErrorCode::kParseError, "ELFIO failed to parse ELF: " + libapp_path.string());
  }

  image.arch = DetectArchFromElfMachine(reader.get_machine());

  for (const auto& seg : reader.segments) {
    SegmentInfo s;
    s.name = "PT_LOAD";
    s.va = seg->get_virtual_address();
    s.size = seg->get_memory_size();
    s.file_offset = seg->get_offset();
    const auto flags = seg->get_flags();
    s.readable = (flags & ELFIO::PF_R) != 0;
    s.writable = (flags & ELFIO::PF_W) != 0;
    s.executable = (flags & ELFIO::PF_X) != 0;
    image.segments.push_back(s);
  }

  for (const auto& sec : reader.sections) {
    if (sec->get_name() == ".text") {
      SegmentInfo t;
      t.name = ".text";
      t.va = sec->get_address();
      t.size = sec->get_size();
      t.file_offset = sec->get_offset();
      t.readable = true;
      t.executable = true;
      image.segments.push_back(t);
    } else if (sec->get_name() == ".rodata") {
      SegmentInfo r;
      r.name = ".rodata";
      r.va = sec->get_address();
      r.size = sec->get_size();
      r.file_offset = sec->get_offset();
      r.readable = true;
      image.segments.push_back(r);
    }
  }

  for (const auto& sec : reader.sections) {
    const auto sec_name = sec->get_name();
    if (sec_name != ".symtab" && sec_name != ".dynsym") {
      continue;
    }
    image.has_symbol_table = true;
    ELFIO::symbol_section_accessor symbols(reader, sec.get());
    for (unsigned int i = 0; i < symbols.get_symbols_num(); ++i) {
      std::string name;
      ELFIO::Elf64_Addr value = 0;
      ELFIO::Elf_Xword size = 0;
      unsigned char bind = 0;
      unsigned char type = 0;
      ELFIO::Elf_Half section = 0;
      unsigned char other = 0;
      symbols.get_symbol(i, name, value, size, bind, type, section, other);
      if (name.empty()) {
        continue;
      }
      image.symbols[name] = SymbolInfo{name, static_cast<uint64_t>(value), static_cast<uint64_t>(size)};
    }
  }
#else
  image.arch = "arm64";
#endif

  return image;
}

util::StatusOr<BinaryImage> load_input(const std::string& path) {
  std::filesystem::path input(path);
  if (!std::filesystem::exists(input)) {
    return util::Status::Error(util::ErrorCode::kNotFound, "input path not found: " + path);
  }

  if (LooksLikeApkPath(input)) {
    auto extracted_or = ExtractLibappFromApk(input);
    if (!extracted_or.ok()) {
      return extracted_or.status();
    }
    auto image_or = LoadElfImage(extracted_or.value());
    if (!image_or.ok()) {
      return image_or.status();
    }
    image_or.value().input_path = path;
    image_or.value().extracted_from_apk = true;
    return image_or.value();
  }

  auto image_or = LoadElfImage(input);
  if (!image_or.ok()) {
    return image_or.status();
  }
  image_or.value().input_path = path;
  return image_or.value();
}

}  // namespace flutterdec::core::loader
