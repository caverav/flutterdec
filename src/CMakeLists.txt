set(FLUTTERDEC_CORE_SOURCES
  cli/commands/cmd_info.cpp
  cli/commands/cmd_decompile.cpp
  cli/commands/cmd_export_ida.cpp
  cli/commands/cmd_export_ghidra.cpp
  cli/commands/cmd_setup.cpp
  cli/commands/Common.cpp
  core/loader/ApkExtractor.cpp
  core/loader/ElfLoader.cpp
  core/loader/SnapshotLocator.cpp
  core/dartvm/DartVmManager.cpp
  core/model/Program.cpp
  core/model/ObjectPool.cpp
  core/disasm/CapstoneDisassembler.cpp
  core/disasm/DartAbiAnnotator.cpp
  core/ir/IRBuilder.cpp
  core/ir/CFG.cpp
  core/decompiler/Structurer.cpp
  core/decompiler/ExprBuilder.cpp
  core/decompiler/PseudocodeEmitter.cpp
  core/naming/NameResolver.cpp
  core/naming/Heuristics/FrameworkMatcher.cpp
  core/naming/Heuristics/StringHintRenamer.cpp
  core/naming/Heuristics/UsagePatternRenamer.cpp
  core/export/IdaExporter.cpp
  core/export/GhidraExporter.cpp
  core/export/JsonReport.cpp
  util/Log.cpp
  util/FileIO.cpp
  util/Hash.cpp
)

add_library(flutterdec_core ${FLUTTERDEC_CORE_SOURCES})

target_include_directories(flutterdec_core
  PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}
)

target_link_libraries(flutterdec_core
  PUBLIC
    nlohmann_json::nlohmann_json
)

if(FLUTTERDEC_ENABLE_CAPSTONE)
  if(TARGET capstone-static)
    target_link_libraries(flutterdec_core PUBLIC capstone-static)
    target_compile_definitions(flutterdec_core PUBLIC FLUTTERDEC_HAVE_CAPSTONE=1)
  elseif(TARGET capstone)
    target_link_libraries(flutterdec_core PUBLIC capstone)
    target_compile_definitions(flutterdec_core PUBLIC FLUTTERDEC_HAVE_CAPSTONE=1)
  endif()
endif()

if(TARGET ELFIO::elfio)
  target_link_libraries(flutterdec_core PUBLIC ELFIO::elfio)
endif()

add_executable(flutterdec cli/main.cpp)
target_link_libraries(flutterdec PRIVATE flutterdec_core)
