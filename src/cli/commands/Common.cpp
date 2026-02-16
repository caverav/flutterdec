#include "cli/commands/Common.h"

#include "core/dartvm/DartVmManager.h"
#include "core/loader/BinaryImage.h"
#include "core/loader/SnapshotLocator.h"

namespace flutterdec::cli::commands {

util::StatusOr<PipelineContext> BuildPipeline(const std::string& input_path, bool require_adapter) {
  return BuildPipeline(input_path, require_adapter, !require_adapter);
}

util::StatusOr<PipelineContext> BuildPipeline(const std::string& input_path, bool require_adapter,
                                              bool allow_heuristic_fallback) {
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

  core::dartvm::ParseOptions parse_options;
  parse_options.allow_heuristic_fallback = allow_heuristic_fallback;
  auto program_or = vm.ParseProgram(ctx.snapshots, ctx.version, input_path, parse_options);
  if (!program_or.ok()) {
    if (require_adapter) {
      return program_or.status();
    }
    return ctx;
  }
  ctx.program = std::move(program_or.value());
  if (require_adapter && !allow_heuristic_fallback && ctx.program.model_source != "adapter") {
    return util::Status::Error(
        util::ErrorCode::kUnsupported,
        "adapter-backed model required for this command. Install adapter: flutterdec setup --dart-hash " +
            ctx.version.hash + " or run with --experimental-heuristic where supported.");
  }
  return ctx;
}

}  // namespace flutterdec::cli::commands
