#pragma once

#include <string>

#include "core/dartvm/VmAdapter.h"
#include "core/loader/BinaryImage.h"
#include "core/loader/SnapshotLocator.h"
#include "core/model/Program.h"
#include "util/Status.h"

namespace flutterdec::cli::commands {

struct PipelineContext {
  core::loader::BinaryImage image;
  core::loader::SnapshotRegions snapshots;
  core::dartvm::DartVersionInfo version;
  core::model::Program program;
};

util::StatusOr<PipelineContext> BuildPipeline(const std::string& input_path, bool require_adapter);
util::StatusOr<PipelineContext> BuildPipeline(const std::string& input_path, bool require_adapter,
                                              bool allow_heuristic_fallback);

}  // namespace flutterdec::cli::commands
