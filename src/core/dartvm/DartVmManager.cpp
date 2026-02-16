#include "core/dartvm/DartVmManager.h"

#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "core/dartvm/HeuristicProgramBuilder.h"
#include "util/FileIO.h"
#include "util/Hash.h"

namespace flutterdec::core::dartvm {
namespace {

using nlohmann::json;

std::string DefaultHomeDir() {
  const char* home = std::getenv("HOME");
  return home ? home : ".";
}

std::filesystem::path ResolveManifestPath(const AdapterConfig& cfg) {
  if (!cfg.manifest_path.empty()) {
    return cfg.manifest_path;
  }
  return std::filesystem::path("src/core/dartvm/versions/manifest.json");
}

std::filesystem::path ResolveAdapterDir(const AdapterConfig& cfg) {
  if (!cfg.adapter_dir.empty()) {
    return cfg.adapter_dir;
  }
  if (const char* env = std::getenv("FLUTTERDEC_ADAPTER_DIR")) {
    return env;
  }
  return std::filesystem::path(DefaultHomeDir()) / ".cache/flutterdec/adapters";
}

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

std::string SniffAsciiField(const std::vector<uint8_t>& data, const std::string& key) {
  if (data.empty()) {
    return "";
  }
  std::string s(data.begin(), data.begin() + static_cast<long>(std::min<size_t>(data.size(), 4096)));
  const auto pos = s.find(key);
  if (pos == std::string::npos) {
    return "";
  }
  size_t i = pos + key.size();
  size_t j = i;
  while (j < s.size() && (std::isalnum(static_cast<unsigned char>(s[j])) || s[j] == '.' || s[j] == '_' || s[j] == '-')) {
    ++j;
  }
  return s.substr(i, j - i);
}

model::ObjKind ParseKind(const std::string& s) {
  if (s == "String") return model::ObjKind::String;
  if (s == "Int") return model::ObjKind::Int;
  if (s == "Double") return model::ObjKind::Double;
  if (s == "Type") return model::ObjKind::Type;
  if (s == "FunctionRef") return model::ObjKind::FunctionRef;
  if (s == "ClassRef") return model::ObjKind::ClassRef;
  return model::ObjKind::Unknown;
}

util::StatusOr<json> ReadManifest(const std::filesystem::path& manifest_path) {
  if (!std::filesystem::exists(manifest_path)) {
    return util::Status::Error(util::ErrorCode::kNotFound, "adapter manifest not found: " + manifest_path.string());
  }
  std::ifstream in(manifest_path);
  if (!in) {
    return util::Status::Error(util::ErrorCode::kIoError, "failed to open adapter manifest");
  }
  json j;
  try {
    in >> j;
  } catch (const std::exception& e) {
    return util::Status::Error(util::ErrorCode::kParseError, std::string("invalid adapter manifest: ") + e.what());
  }
  return j;
}

util::StatusOr<std::filesystem::path> ResolveAdapterExecutable(const DartVersionInfo& version,
                                                               const std::filesystem::path& manifest_path,
                                                               const std::filesystem::path& adapter_dir) {
  auto manifest_or = ReadManifest(manifest_path);
  if (!manifest_or.ok()) {
    return manifest_or.status();
  }
  const auto& j = manifest_or.value();
  if (!j.contains("entries") || !j["entries"].is_array()) {
    return util::Status::Error(util::ErrorCode::kParseError, "adapter manifest missing entries array");
  }

  std::string adapter_name;
  for (const auto& e : j["entries"]) {
    if (e.value("snapshot_hash", "") == version.hash ||
        e.value("version", "") == version.version) {
      adapter_name = e.value("adapter", "");
      break;
    }
  }
  if (adapter_name.empty()) {
    const bool use_hash = version.version.empty() || version.version == "unknown";
    adapter_name = "dart_adapter_" + (use_hash ? version.hash : version.version);
  }

  auto path = adapter_dir / adapter_name;
  if (!std::filesystem::exists(path)) {
    return util::Status::Error(
        util::ErrorCode::kNotFound,
        "adapter missing for hash " + version.hash +
            ". Run: flutterdec setup --dart-hash " + version.hash);
  }
  return path;
}

util::StatusOr<model::Program> ParseProgramJson(const std::filesystem::path& path,
                                                const std::string& input_path,
                                                const loader::SnapshotRegions* regions) {
  std::ifstream in(path);
  if (!in) {
    return util::Status::Error(util::ErrorCode::kIoError, "failed to open adapter output: " + path.string());
  }

  json j;
  try {
    in >> j;
  } catch (const std::exception& e) {
    return util::Status::Error(util::ErrorCode::kParseError, std::string("invalid adapter output: ") + e.what());
  }

  if (j.value("schema_version", 1) != 1) {
    return util::Status::Error(util::ErrorCode::kUnsupported, "unsupported adapter schema_version");
  }

  model::Program program;
  program.input_path = input_path;
  program.arch = j.value("arch", "arm64");
  program.dart_version = j.value("dart_version", "unknown");
  program.snapshot_hash = j.value("snapshot_hash", "unknown");
  program.model_source = "adapter";

  for (const auto& o : j.value("object_pool", json::array())) {
    model::Obj obj;
    obj.kind = ParseKind(o.value("kind", "Unknown"));
    obj.as_string = o.value("s", o.value("as_string", ""));
    obj.as_int = o.value("n", o.value("as_int", 0));
    obj.as_double = o.value("d", o.value("as_double", 0.0));
    obj.ref_va = o.value("ref_va", 0ull);
    program.object_pool.Add(std::move(obj));
  }

  for (const auto& c : j.value("classes", json::array())) {
    model::ClassInfo ci;
    ci.id = c.value("id", 0ull);
    ci.name_obf = c.value("name", "");
    ci.name_display = ci.name_obf;
    ci.superclass = c.value("super", "");
    ci.library_uri = c.value("lib", "");
    program.classes.push_back(std::move(ci));
  }

  for (const auto& f : j.value("functions", json::array())) {
    model::FunctionInfo fi;
    fi.id = f.value("id", 0ull);
    fi.name_obf = f.value("name", "");
    fi.name_display = fi.name_obf;
    fi.owner_class_obf = f.value("owner_class", "");
    fi.owner_class_display = fi.owner_class_obf;
    fi.entry_va = f.value("entry_va", 0ull);
    fi.size_bytes = f.value("size", 0ull);
    fi.code_section_va = f.value("code_section_va", 0ull);
    program.functions.push_back(std::move(fi));
  }

  if (program.classes.empty()) {
    model::ClassInfo ci;
    ci.id = 0;
    ci.name_obf = "Global";
    ci.name_display = "Global";
    ci.superclass = "Object";
    ci.library_uri = "package:app/main.dart";
    program.classes.push_back(std::move(ci));
  }

  std::set<std::string> libs;
  for (const auto& c : program.classes) {
    if (!c.library_uri.empty()) {
      libs.insert(c.library_uri);
    }
  }
  if (libs.empty()) {
    libs.insert("package:app/main.dart");
  }
  program.libraries.clear();
  size_t lib_id = 0;
  for (const auto& uri : libs) {
    model::LibraryInfo li;
    li.id = lib_id++;
    li.uri = uri;
    li.name_display = uri;
    program.libraries.push_back(std::move(li));
  }

  uint64_t text_start = 0;
  uint64_t text_end = 0;
  if (regions && regions->backing_image) {
    const auto* text = regions->backing_image->FindSegmentByName(".text");
    if (!text) {
      for (const auto& seg : regions->backing_image->segments) {
        if (seg.executable) {
          text = &seg;
          break;
        }
      }
    }
    if (text) {
      text_start = text->va;
      text_end = text->va + text->size;
    }
  }

  std::sort(program.functions.begin(), program.functions.end(),
            [](const model::FunctionInfo& a, const model::FunctionInfo& b) { return a.entry_va < b.entry_va; });
  for (size_t i = 0; i < program.functions.size(); ++i) {
    auto& fn = program.functions[i];
    if (fn.owner_class_obf.empty()) {
      fn.owner_class_obf = "Global";
      fn.owner_class_display = "Global";
    }
    if (fn.name_obf.empty()) {
      fn.name_obf = "fn_" + std::to_string(i);
      fn.name_display = fn.name_obf;
    }
    if (fn.code_section_va == 0) {
      fn.code_section_va = text_start;
    }

    if (fn.size_bytes == 0) {
      const uint64_t next_va =
          (i + 1 < program.functions.size()) ? program.functions[i + 1].entry_va : text_end;
      if (next_va > fn.entry_va) {
        fn.size_bytes = next_va - fn.entry_va;
        fn.size_estimated = true;
      } else {
        fn.size_bytes = 128;
        fn.size_estimated = true;
      }
    }
  }

  program.StableSort();
  return program;
}

util::StatusOr<model::Program> TryHeuristicFallback(const loader::SnapshotRegions& regions,
                                                    const DartVersionInfo& version_info,
                                                    const std::string& input_path,
                                                    const util::Status& original_status,
                                                    const ParseOptions& options) {
  if (!options.allow_heuristic_fallback) {
    return original_status;
  }
  auto heuristic_or = BuildHeuristicProgram(regions, version_info, input_path);
  if (!heuristic_or.ok()) {
    return original_status;
  }
  return heuristic_or;
}

}  // namespace

util::StatusOr<DartVersionInfo> detect_dart_version(const loader::SnapshotRegions& regions) {
  auto vm_data = Slice(regions, regions.vm_data);
  auto iso_data = Slice(regions, regions.isolate_data);

  DartVersionInfo out;
  out.version = SniffAsciiField(vm_data, "ver:");
  if (out.version.empty()) {
    out.version = SniffAsciiField(iso_data, "ver:");
  }

  out.hash = SniffAsciiField(vm_data, "hash:");
  if (out.hash.empty()) {
    std::vector<uint8_t> merged;
    merged.reserve(vm_data.size() + std::min<size_t>(iso_data.size(), 4096));
    merged.insert(merged.end(), vm_data.begin(), vm_data.end());
    merged.insert(merged.end(), iso_data.begin(), iso_data.begin() + static_cast<long>(std::min<size_t>(iso_data.size(), 4096)));
    out.hash = util::Fnv1a64Hex(merged);
  }
  if (out.version.empty()) {
    out.version = "unknown";
  }
  return out;
}

util::StatusOr<model::Program> parse_snapshot_with_vm_adapter(const loader::SnapshotRegions& regions,
                                                              const DartVersionInfo& version_info,
                                                              const std::string& input_path,
                                                              const AdapterConfig& cfg,
                                                              const ParseOptions& options) {
  const auto manifest_path = ResolveManifestPath(cfg);
  const auto adapter_dir = ResolveAdapterDir(cfg);

  const char* fake_json = std::getenv("FLUTTERDEC_FAKE_ADAPTER_JSON");
  if (fake_json && std::filesystem::exists(fake_json)) {
    return ParseProgramJson(fake_json, input_path, &regions);
  }

  auto adapter_or = ResolveAdapterExecutable(version_info, manifest_path, adapter_dir);
  if (!adapter_or.ok()) {
    return TryHeuristicFallback(regions, version_info, input_path, adapter_or.status(), options);
  }

  const auto tmp_dir = std::filesystem::temp_directory_path() / ("flutterdec_adapter_" + version_info.hash);
  auto mk = util::EnsureDir(tmp_dir);
  if (!mk.ok()) {
    return mk;
  }

  const auto vm_data = Slice(regions, regions.vm_data);
  const auto iso_data = Slice(regions, regions.isolate_data);
  const auto vm_instr = Slice(regions, regions.vm_instr);
  const auto iso_instr = Slice(regions, regions.isolate_instr);

  const auto vm_data_path = tmp_dir / "vm_data.bin";
  const auto iso_data_path = tmp_dir / "isolate_data.bin";
  const auto vm_instr_path = tmp_dir / "vm_instr.bin";
  const auto iso_instr_path = tmp_dir / "isolate_instr.bin";
  const auto out_path = tmp_dir / "program.json";

  if (!util::WriteFileBytes(vm_data_path, vm_data).ok() ||
      !util::WriteFileBytes(iso_data_path, iso_data).ok() ||
      !util::WriteFileBytes(vm_instr_path, vm_instr).ok() ||
      !util::WriteFileBytes(iso_instr_path, iso_instr).ok()) {
    return util::Status::Error(util::ErrorCode::kIoError, "failed to write temporary adapter input files");
  }

  std::ostringstream cmd;
  cmd << "'" << adapter_or.value().string() << "'"
      << " --vm-data '" << vm_data_path.string() << "'"
      << " --isolate-data '" << iso_data_path.string() << "'"
      << " --vm-instr '" << vm_instr_path.string() << "'"
      << " --isolate-instr '" << iso_instr_path.string() << "'"
      << " --vm-instr-va " << regions.vm_instr_va
      << " --isolate-instr-va " << regions.isolate_instr_va
      << " --out '" << out_path.string() << "'";

  const int rc = std::system(cmd.str().c_str());
  if (rc != 0) {
    return TryHeuristicFallback(
        regions, version_info, input_path,
        util::Status::Error(util::ErrorCode::kExternalToolError,
                            "adapter execution failed: " + adapter_or.value().string()),
        options);
  }

  auto parsed_or = ParseProgramJson(out_path, input_path, &regions);
  if (!parsed_or.ok()) {
    return TryHeuristicFallback(regions, version_info, input_path, parsed_or.status(), options);
  }
  return parsed_or;
}

DartVmManager::DartVmManager(AdapterConfig cfg) : cfg_(std::move(cfg)) {}

util::StatusOr<DartVersionInfo> DartVmManager::DetectVersion(const loader::SnapshotRegions& regions) const {
  return detect_dart_version(regions);
}

util::StatusOr<model::Program> DartVmManager::ParseProgram(const loader::SnapshotRegions& regions,
                                                           const DartVersionInfo& version_info,
                                                           const std::string& input_path,
                                                           const ParseOptions& options) const {
  return parse_snapshot_with_vm_adapter(regions, version_info, input_path, cfg_, options);
}

}  // namespace flutterdec::core::dartvm
