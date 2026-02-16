#include "cli/commands/cmd_decompile.h"

#include <filesystem>
#include <iostream>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "cli/commands/Common.h"
#include "core/decompiler/PseudocodeEmitter.h"
#include "core/disasm/CapstoneDisassembler.h"
#include "core/disasm/DartAbiAnnotator.h"
#include "core/export/JsonReport.h"
#include "core/ir/CFG.h"
#include "core/ir/IRBuilder.h"
#include "core/naming/NameResolver.h"
#include "util/FileIO.h"

namespace flutterdec::cli::commands {
namespace {

struct DecompileOptions {
  std::string input;
  std::filesystem::path out_dir;
  int threads = 1;
  bool emit_asm = false;
  bool emit_ir = false;
  bool no_naming = false;
  std::filesystem::path mapping;
  std::string focus;
  size_t max_functions = 0;
};

std::optional<DecompileOptions> ParseOptions(const std::vector<std::string>& args) {
  if (args.size() < 3) {
    return std::nullopt;
  }
  DecompileOptions opt;
  opt.input = args[0];

  for (size_t i = 1; i < args.size(); ++i) {
    if (args[i] == "-o" && i + 1 < args.size()) {
      opt.out_dir = args[++i];
    } else if (args[i] == "--threads" && i + 1 < args.size()) {
      opt.threads = std::stoi(args[++i]);
    } else if (args[i] == "--emit-asm") {
      opt.emit_asm = true;
    } else if (args[i] == "--emit-ir") {
      opt.emit_ir = true;
    } else if (args[i] == "--no-naming") {
      opt.no_naming = true;
    } else if (args[i] == "--mapping" && i + 1 < args.size()) {
      opt.mapping = args[++i];
    } else if (args[i] == "--focus" && i + 1 < args.size()) {
      opt.focus = args[++i];
      if (!opt.focus.empty() && opt.focus.back() == '*') {
        opt.focus.pop_back();
      }
    } else if (args[i] == "--max-functions" && i + 1 < args.size()) {
      opt.max_functions = static_cast<size_t>(std::stoull(args[++i]));
    }
  }

  if (opt.out_dir.empty()) {
    return std::nullopt;
  }
  return opt;
}

std::string SanitizeFileStem(const core::model::FunctionInfo& fn) {
  std::ostringstream oss;
  oss << fn.id << "_" << fn.owner_class_display << "_" << fn.name_display;
  std::string s = oss.str();
  for (auto& c : s) {
    if (!std::isalnum(static_cast<unsigned char>(c)) && c != '_') {
      c = '_';
    }
  }
  return s;
}

std::string RenderAsm(const std::vector<core::disasm::AsmInstruction>& instrs) {
  std::ostringstream out;
  for (const auto& ins : instrs) {
    out << "0x" << std::hex << ins.va << std::dec << ": " << ins.mnemonic;
    if (!ins.op_str.empty()) {
      out << " " << ins.op_str;
    }
    if (!ins.annotation.empty()) {
      out << " ; " << ins.annotation;
    }
    out << "\n";
  }
  return out.str();
}

nlohmann::json RenderIr(const core::ir::FunctionIR& fn_ir) {
  nlohmann::json j;
  j["function_id"] = fn_ir.meta.id;
  j["name"] = fn_ir.meta.name_display;
  j["blocks"] = nlohmann::json::array();
  for (const auto& b : fn_ir.blocks) {
    nlohmann::json jb;
    jb["start_va"] = b.start_va;
    jb["succs"] = b.succs;
    jb["preds"] = b.preds;
    jb["instrs"] = nlohmann::json::array();
    for (const auto& i : b.instrs) {
      jb["instrs"].push_back({
          {"va", i.va},
          {"op", static_cast<int>(i.op)},
          {"src", i.src},
          {"target", i.target},
      });
    }
    j["blocks"].push_back(std::move(jb));
  }
  return j;
}

}  // namespace

int RunDecompile(const std::vector<std::string>& args) {
  auto opt = ParseOptions(args);
  if (!opt.has_value()) {
    std::cerr << "usage: flutterdec decompile <libapp.so|apk> -o out/ [options]\n";
    return 2;
  }

  auto ctx_or = BuildPipeline(opt->input, true);
  if (!ctx_or.ok()) {
    std::cerr << "error: " << ctx_or.status().message << "\n";
    return 1;
  }
  auto ctx = std::move(ctx_or.value());

  core::naming::NamingConfig naming_cfg;
  naming_cfg.enabled = !opt->no_naming;
  naming_cfg.mapping_path = opt->mapping;
  core::naming::apply_naming(ctx.program, naming_cfg);

  const auto pseudocode_dir = opt->out_dir / "pseudocode";
  const auto asm_dir = opt->out_dir / "asm";
  const auto ir_dir = opt->out_dir / "ir";
  const auto map_dir = opt->out_dir / "maps";

  util::EnsureDir(opt->out_dir);
  util::EnsureDir(pseudocode_dir);
  util::EnsureDir(map_dir);
  if (opt->emit_asm) util::EnsureDir(asm_dir);
  if (opt->emit_ir) util::EnsureDir(ir_dir);

  core::disasm::CapstoneDisassembler dis;
  core::disasm::DartAbiAnnotator annot;
  core::ir::IRBuilder ir_builder;
  core::ir::CFGBuilder cfg_builder;

  std::vector<core::ir::FunctionIR> all_irs;
  all_irs.reserve(ctx.program.functions.size());

  const size_t limit = opt->max_functions > 0 ? std::min(opt->max_functions, ctx.program.functions.size())
                                              : ctx.program.functions.size();

  for (size_t i = 0; i < limit; ++i) {
    auto& fn = ctx.program.functions[i];
    auto dis_or = dis.DisassembleFunction(ctx.image, fn);
    if (!dis_or.ok()) {
      continue;
    }

    auto instrs = std::move(dis_or.value());
    auto anno = annot.Annotate(ctx.program, fn, &instrs);
    fn.calls = anno.call_targets;

    auto llir = ir_builder.BuildLlir(instrs);
    auto fn_ir = cfg_builder.Build(fn, instrs, llir);
    all_irs.push_back(fn_ir);

    if (opt->emit_asm) {
      const auto asm_path = asm_dir / (SanitizeFileStem(fn) + ".asm");
      util::WriteFile(asm_path, RenderAsm(instrs));
    }

    if (opt->emit_ir) {
      const auto ir_path = ir_dir / (SanitizeFileStem(fn) + ".json");
      util::WriteFile(ir_path, RenderIr(fn_ir).dump(2));
    }
  }

  auto pseudo_st = core::decompiler::EmitProgramPseudocode(ctx.program, all_irs, pseudocode_dir, opt->focus);
  if (!pseudo_st.ok()) {
    std::cerr << "error: " << pseudo_st.message << "\n";
    return 1;
  }

  core::naming::WriteNamesMap(ctx.program, map_dir / "names.json");
  core::exporting::WriteProgramReport(ctx.program, opt->out_dir / "report.json");

  std::cout << "decompile complete: " << opt->out_dir.string() << "\n";
  return 0;
}

}  // namespace flutterdec::cli::commands
