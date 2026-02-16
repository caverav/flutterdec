#pragma once

#include <string>

#include "core/dartvm/VmAdapter.h"
#include "core/loader/SnapshotLocator.h"
#include "core/model/Program.h"
#include "util/Status.h"

namespace flutterdec::core::dartvm {

util::StatusOr<model::Program> BuildHeuristicProgram(const loader::SnapshotRegions& regions,
                                                     const DartVersionInfo& version_info,
                                                     const std::string& input_path);

}  // namespace flutterdec::core::dartvm
