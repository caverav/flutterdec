#include "cli/commands/cmd_info.h"

#include <iostream>

#include "cli/commands/Common.h"

namespace flutterdec::cli::commands {

int RunInfo(const std::vector<std::string>& args) {
  if (args.empty()) {
    std::cerr << "usage: flutterdec info <libapp.so|apk>\n";
    return 2;
  }

  auto ctx_or = BuildPipeline(args[0], false);
  if (!ctx_or.ok()) {
    std::cerr << "error: " << ctx_or.status().message << "\n";
    return 1;
  }

  const auto& ctx = ctx_or.value();
  std::cout << "input: " << args[0] << "\n";
  std::cout << "arch: " << ctx.image.arch << "\n";
  std::cout << "snapshot_hash: " << ctx.version.hash << "\n";
  std::cout << "dart_version: " << ctx.version.version << "\n";
  std::cout << "symbol_table: " << (ctx.image.has_symbol_table ? "present" : "missing") << "\n";
  std::cout << "regions.vm_data.offset: " << ctx.snapshots.vm_data.file_offset << "\n";
  std::cout << "regions.isolate_data.offset: " << ctx.snapshots.isolate_data.file_offset << "\n";
  std::cout << "regions.vm_instr.offset: " << ctx.snapshots.vm_instr.file_offset << "\n";
  std::cout << "regions.isolate_instr.offset: " << ctx.snapshots.isolate_instr.file_offset << "\n";
  std::cout << "regions.vm_instr_va: 0x" << std::hex << ctx.snapshots.vm_instr_va << std::dec << "\n";
  std::cout << "regions.isolate_instr_va: 0x" << std::hex << ctx.snapshots.isolate_instr_va << std::dec << "\n";

  if (!ctx.program.functions.empty() || !ctx.program.classes.empty()) {
    std::cout << "libraries: " << ctx.program.libraries.size() << "\n";
    std::cout << "classes: " << ctx.program.classes.size() << "\n";
    std::cout << "functions: " << ctx.program.functions.size() << "\n";
    std::cout << "object_pool: " << ctx.program.object_pool.size() << "\n";
    std::cout << "program_model: " << ctx.program.model_source << "\n";
  } else {
    std::cout << "adapter: unavailable (run setup with hash)\n";
  }

  return 0;
}

}  // namespace flutterdec::cli::commands
