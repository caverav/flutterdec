#include "test_common.h"

#include <vector>

#include "core/dartvm/HeuristicProgramBuilder.h"

int main() {
  using namespace flutterdec;

  core::loader::BinaryImage image;
  image.arch = "arm64";
  image.elf_bytes.resize(4096, 0);

  core::loader::SegmentInfo text;
  text.name = ".text";
  text.va = 0x1000;
  text.file_offset = 0x100;
  text.size = 0x400;
  text.executable = true;
  image.segments.push_back(text);

  core::loader::SymbolInfo sym;
  sym.name = "Global::doWork";
  sym.va = 0x1010;
  sym.size = 32;
  image.symbols[sym.name] = sym;

  const char* pkg = "package:demo/main.dart";
  const size_t pkg_off = 0x500;
  for (size_t i = 0; pkg[i] != 0; ++i) {
    image.elf_bytes[pkg_off + i] = static_cast<uint8_t>(pkg[i]);
  }

  core::loader::SnapshotRegions regions;
  regions.backing_image = &image;
  regions.vm_data = core::loader::SnapshotSpan{pkg_off, 64, 0};
  regions.isolate_data = core::loader::SnapshotSpan{pkg_off, 64, 0};

  core::dartvm::DartVersionInfo vi;
  vi.hash = "h";
  vi.version = "3.x";

  auto p_or = core::dartvm::BuildHeuristicProgram(regions, vi, "/tmp/libapp.so");
  TASSERT(p_or.ok());
  TASSERT(p_or.value().model_source == "heuristic");
  TASSERT(!p_or.value().functions.empty());
  TASSERT(!p_or.value().classes.empty());
  TASSERT(p_or.value().object_pool.size() > 0);
  return 0;
}
