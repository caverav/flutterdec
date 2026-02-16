#include "cli/commands/Common.h"

#include "core/dartvm/DartVmManager.h"
#include "core/loader/BinaryImage.h"
#include "core/loader/SnapshotLocator.h"

namespace flutterdec::cli::commands {

util::StatusOr<PipelineContext> BuildPipeline(const std::string& input_path, bool require_adapter) {
  auto image_or = core::loader::load_input(input_path);
  if (!image_or.ok()) {
    return image_or.status();
  }

  auto regions_or = core::loader::locate_snapshots(image_or.value());
  if (!regions_or.ok()) {
    return regions_or.status();
  }

  core::dartvm::DartVmManager vm;
  auto version_or = vm.DetectVersion(regions_or.value());
  if (!version_or.ok()) {
    return version_or.status();
  }

  PipelineContext ctx;
  ctx.image = std::move(image_or.value());
  ctx.snapshots = std::move(regions_or.value());
  ctx.version = std::move(version_or.value());
  ctx.snapshots.backing_image = &ctx.image;

  auto program_or = vm.ParseProgram(ctx.snapshots, ctx.version, input_path);
  if (!program_or.ok()) {
    if (require_adapter) {
      return program_or.status();
    }
    return ctx;
  }
  ctx.program = std::move(program_or.value());
  return ctx;
}

}  // namespace flutterdec::cli::commands
