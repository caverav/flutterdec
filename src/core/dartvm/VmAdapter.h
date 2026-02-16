#pragma once

#include <filesystem>
#include <string>

#include "core/loader/SnapshotLocator.h"
#include "core/model/Program.h"
#include "util/Status.h"

namespace flutterdec::core::dartvm {

struct DartVersionInfo {
  std::string hash;
  std::string version;
};

struct AdapterConfig {
  std::filesystem::path manifest_path;
  std::filesystem::path adapter_dir;
};

util::StatusOr<DartVersionInfo> detect_dart_version(const loader::SnapshotRegions& regions);
util::StatusOr<model::Program> parse_snapshot_with_vm_adapter(
    const loader::SnapshotRegions& regions, const DartVersionInfo& version_info,
    const std::string& input_path, const AdapterConfig& cfg = {});

}  // namespace flutterdec::core::dartvm
