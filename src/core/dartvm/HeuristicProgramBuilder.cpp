#include "core/dartvm/HeuristicProgramBuilder.h"

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <map>
#include <optional>
#include <set>
#include <sstream>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

#if defined(FLUTTERDEC_HAVE_CAPSTONE)
#include <capstone/capstone.h>
#endif

namespace flutterdec::core::dartvm {
namespace {

std::vector<uint8_t> Slice(const loader::SnapshotRegions& regions, const loader::SnapshotSpan& span) {
  std::vector<uint8_t> out;
  if (!regions.backing_image) {
    return out;
  }
  const auto& bytes = regions.backing_image->elf_bytes;
  if (span.file_offset >= bytes.size()) {
    return out;
  }
  const size_t n = std::min(span.size, bytes.size() - span.file_offset);
  out.insert(out.end(), bytes.begin() + static_cast<long>(span.file_offset),
             bytes.begin() + static_cast<long>(span.file_offset + n));
  return out;
}

bool IsPrintableAscii(char c) {
  return c >= 32 && c <= 126;
}

std::vector<std::string> ExtractStrings(const std::vector<uint8_t>& data, size_t min_len, size_t max_strings) {
  std::vector<std::string> out;
  std::string current;
  for (uint8_t b : data) {
    const char c = static_cast<char>(b);
    if (IsPrintableAscii(c)) {
      current.push_back(c);
      continue;
    }

    if (current.size() >= min_len) {
      out.push_back(current);
      if (out.size() >= max_strings) {
        return out;
      }
    }
    current.clear();
  }

  if (current.size() >= min_len && out.size() < max_strings) {
    out.push_back(current);
  }
  return out;
}

void AddObjectPoolStrings(model::Program* program, const std::vector<std::string>& strings) {
  std::set<std::string> seen;
  for (const auto& s : strings) {
    if (s.size() > 200 || seen.count(s) != 0) {
      continue;
    }
    seen.insert(s);
    model::Obj obj;
    obj.kind = model::ObjKind::String;
    obj.as_string = s;
    program->object_pool.Add(std::move(obj));
  }
}

std::string ExtractLikelyPackageUri(const model::Program& program) {
  for (const auto& obj : program.object_pool.entries()) {
    if (obj.kind != model::ObjKind::String) {
      continue;
    }
    if (obj.as_string.rfind("package:", 0) == 0) {
      return obj.as_string;
    }
  }
  return "package:app/main.dart";
}

std::string SanitizeName(std::string s) {
  for (char& c : s) {
    if (!(std::isalnum(static_cast<unsigned char>(c)) || c == '_' || c == ':' || c == '.')) {
      c = '_';
    }
  }
  return s;
}

bool ShouldIgnoreSymbol(const std::string& name) {
  if (name.empty()) {
    return true;
  }
  static const std::vector<std::string> kIgnoredPrefixes = {
      "_kDart", "__cxa", "_Unwind", "__gnu", "_ZTV", "_ZTI", "_GLOBAL_", "_init", "_fini"};
  for (const auto& p : kIgnoredPrefixes) {
    if (name.rfind(p, 0) == 0) {
      return true;
    }
  }
  return false;
}

std::pair<std::string, std::string> ParseOwnerAndMethod(const std::string& symbol_name) {
  const auto cleaned = SanitizeName(symbol_name);
  const auto pos = cleaned.rfind("::");
  if (pos == std::string::npos || pos == 0 || pos + 2 >= cleaned.size()) {
    return {"Global", cleaned};
  }
  return {cleaned.substr(0, pos), cleaned.substr(pos + 2)};
}

struct FunctionSeed {
  uint64_t va = 0;
  uint64_t size = 0;
  std::string owner;
  std::string name;
  bool from_symbol = false;
};

void DiscoverFunctionsFromSymbols(const loader::BinaryImage& image,
                                  uint64_t text_start,
                                  uint64_t text_end,
                                  std::vector<FunctionSeed>* out) {
  for (const auto& [name, sym] : image.symbols) {
    if (ShouldIgnoreSymbol(name)) {
      continue;
    }
    if (sym.va < text_start || sym.va >= text_end) {
      continue;
    }
    auto [owner, method] = ParseOwnerAndMethod(name);
    out->push_back(FunctionSeed{sym.va, sym.size, owner, method, true});
  }
}

void DiscoverFunctionsFromCapstone(const loader::BinaryImage& image,
                                   const loader::SegmentInfo& text,
                                   std::vector<FunctionSeed>* out) {
#if defined(FLUTTERDEC_HAVE_CAPSTONE)
  if (text.file_offset >= image.elf_bytes.size()) {
    return;
  }
  const size_t n = static_cast<size_t>(std::min<uint64_t>(text.size, image.elf_bytes.size() - text.file_offset));
  if (n == 0) {
    return;
  }

  csh handle;
  if (cs_open(CS_ARCH_ARM64, CS_MODE_ARM, &handle) != CS_ERR_OK) {
    return;
  }
  cs_option(handle, CS_OPT_DETAIL, CS_OPT_OFF);

  size_t generated = 0;
  auto parse_target = [](const std::string& op) -> std::optional<uint64_t> {
    const auto pos = op.find("0x");
    if (pos == std::string::npos) {
      return std::nullopt;
    }
    std::stringstream ss;
    ss << std::hex << op.substr(pos);
    uint64_t target = 0;
    ss >> target;
    if (ss.fail()) {
      return std::nullopt;
    }
    return target;
  };

  std::set<uint64_t> discovered_calls;
  size_t off = 0;
  while (off + 4 <= n) {
    cs_insn* insn = nullptr;
    const uint8_t* ptr = image.elf_bytes.data() + text.file_offset + off;
    const uint64_t va = text.va + off;
    const size_t count = cs_disasm(handle, ptr, n - off, va, 1, &insn);
    if (count == 0 || !insn) {
      off += 4;
      continue;
    }

    const std::string mnemonic = insn[0].mnemonic;
    const std::string op_str = insn[0].op_str;

    if (mnemonic == "bl") {
      auto maybe_target = parse_target(op_str);
      if (maybe_target && *maybe_target >= text.va && *maybe_target < text.va + text.size) {
        discovered_calls.insert(*maybe_target);
      }
    }

    if (mnemonic == "stp" && op_str.rfind("x29, x30, [sp", 0) == 0) {
      out->push_back(FunctionSeed{insn[0].address, 0, "Global", "sub_" + std::to_string(insn[0].address), false});
      generated += 1;
      if (generated >= 5000) {
        cs_free(insn, count);
        break;
      }
    }
    const size_t step = std::max<size_t>(insn[0].size, 4);
    off += step;
    cs_free(insn, count);
  }

  for (uint64_t target : discovered_calls) {
    out->push_back(FunctionSeed{target, 0, "Global", "sub_" + std::to_string(target), false});
  }

  cs_close(&handle);
#else
  (void)image;
  (void)text;
  (void)out;
#endif
}

void DeduplicateAndSortSeeds(std::vector<FunctionSeed>* seeds) {
  std::sort(seeds->begin(), seeds->end(), [](const FunctionSeed& a, const FunctionSeed& b) {
    if (a.va == b.va) {
      return a.from_symbol > b.from_symbol;
    }
    return a.va < b.va;
  });

  std::vector<FunctionSeed> dedup;
  dedup.reserve(seeds->size());
  for (const auto& s : *seeds) {
    if (!dedup.empty() && dedup.back().va == s.va) {
      if (!dedup.back().from_symbol && s.from_symbol) {
        dedup.back() = s;
      }
      continue;
    }
    dedup.push_back(s);
  }
  *seeds = std::move(dedup);
}

void FinalizeFunctionSizes(std::vector<FunctionSeed>* seeds, uint64_t text_end) {
  for (size_t i = 0; i < seeds->size(); ++i) {
    auto& fn = (*seeds)[i];
    const uint64_t next_va = (i + 1 < seeds->size()) ? (*seeds)[i + 1].va : text_end;
    const uint64_t max_gap = next_va > fn.va ? next_va - fn.va : 0;
    if (fn.size == 0 || fn.size > max_gap) {
      fn.size = max_gap;
    }
    if (fn.size == 0) {
      fn.size = 4;
    }
    if (fn.size > 0x4000) {
      fn.size = 0x4000;
    }
  }
}

}  // namespace

util::StatusOr<model::Program> BuildHeuristicProgram(const loader::SnapshotRegions& regions,
                                                     const DartVersionInfo& version_info,
                                                     const std::string& input_path) {
  if (!regions.backing_image) {
    return util::Status::Error(util::ErrorCode::kInternal,
                               "heuristic recovery requires snapshot backing image");
  }
  const auto& image = *regions.backing_image;

  model::Program program;
  program.input_path = input_path;
  program.arch = image.arch;
  program.dart_version = version_info.version;
  program.snapshot_hash = version_info.hash;
  program.model_source = "heuristic";

  const auto vm_strings = ExtractStrings(Slice(regions, regions.vm_data), 5, 2048);
  const auto iso_strings = ExtractStrings(Slice(regions, regions.isolate_data), 5, 4096);
  AddObjectPoolStrings(&program, vm_strings);
  AddObjectPoolStrings(&program, iso_strings);

  if (const auto* ro = image.FindSegmentByName(".rodata")) {
    if (ro->file_offset < image.elf_bytes.size()) {
      const size_t n = static_cast<size_t>(std::min<uint64_t>(ro->size, image.elf_bytes.size() - ro->file_offset));
      std::vector<uint8_t> ro_bytes;
      ro_bytes.insert(ro_bytes.end(), image.elf_bytes.begin() + static_cast<long>(ro->file_offset),
                      image.elf_bytes.begin() + static_cast<long>(ro->file_offset + n));
      AddObjectPoolStrings(&program, ExtractStrings(ro_bytes, 5, 4096));
    }
  }

  std::vector<FunctionSeed> seeds;

  loader::SegmentInfo code_seg;
  bool have_code = false;
  if (regions.isolate_instr.size > 0) {
    code_seg.name = "isolate_instr";
    code_seg.va = regions.isolate_instr.va;
    code_seg.file_offset = regions.isolate_instr.file_offset;
    code_seg.size = regions.isolate_instr.size;
    code_seg.executable = true;
    have_code = true;
  }

  if (!have_code) {
    uint64_t best_size = 0;
    for (const auto& seg : image.segments) {
      if (seg.executable && seg.size > best_size) {
        code_seg = seg;
        best_size = seg.size;
        have_code = true;
      }
    }
  }

  uint64_t text_start = 0;
  uint64_t text_end = 0;
  if (have_code) {
    text_start = code_seg.va;
    text_end = code_seg.va + code_seg.size;
    DiscoverFunctionsFromSymbols(image, text_start, text_end, &seeds);
    if (seeds.size() < 10) {
      DiscoverFunctionsFromCapstone(image, code_seg, &seeds);
    }
  }

  if (seeds.empty() && have_code) {
    seeds.push_back(FunctionSeed{code_seg.va, std::min<uint64_t>(code_seg.size, 256), "Global", "entry", false});
  }

  DeduplicateAndSortSeeds(&seeds);
  FinalizeFunctionSizes(&seeds, text_end);

  std::map<std::string, size_t> class_ids;
  const std::string lib_uri = ExtractLikelyPackageUri(program);
  auto ensure_class = [&](const std::string& name) -> size_t {
    auto it = class_ids.find(name);
    if (it != class_ids.end()) {
      return it->second;
    }
    const size_t id = class_ids.size();
    class_ids[name] = id;
    model::ClassInfo ci;
    ci.id = id;
    ci.name_obf = name;
    ci.name_display = name;
    ci.superclass = "Object";
    ci.library_uri = lib_uri;
    program.classes.push_back(std::move(ci));
    return id;
  };

  ensure_class("Global");

  for (size_t i = 0; i < seeds.size(); ++i) {
    const auto& s = seeds[i];
    ensure_class(s.owner);

    model::FunctionInfo fi;
    fi.id = i;
    fi.name_obf = s.name.empty() ? ("fn_" + std::to_string(i)) : s.name;
    fi.name_display = fi.name_obf;
    fi.owner_class_obf = s.owner;
    fi.owner_class_display = s.owner;
    fi.entry_va = s.va;
    fi.size_bytes = s.size;
    fi.size_estimated = !s.from_symbol || s.size == 0;
    fi.code_section_va = have_code ? code_seg.va : 0;
    program.functions.push_back(std::move(fi));
  }

  model::LibraryInfo li;
  li.id = 0;
  li.uri = lib_uri;
  li.name_display = lib_uri;
  program.libraries.push_back(std::move(li));

  program.StableSort();
  return program;
}

}  // namespace flutterdec::core::dartvm
