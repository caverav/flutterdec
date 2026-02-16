#pragma once

#include "core/dartvm/VmAdapter.h"

namespace flutterdec::core::dartvm {

class DartVmManager {
 public:
  explicit DartVmManager(AdapterConfig cfg = {});

  util::StatusOr<DartVersionInfo> DetectVersion(const loader::SnapshotRegions& regions) const;
  util::StatusOr<model::Program> ParseProgram(const loader::SnapshotRegions& regions,
                                              const DartVersionInfo& version_info,
                                              const std::string& input_path) const;

 private:
  AdapterConfig cfg_;
};

}  // namespace flutterdec::core::dartvm
