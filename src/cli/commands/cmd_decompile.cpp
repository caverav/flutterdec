#include "cli/commands/cmd_decompile.h"

#include <cctype>
#include <filesystem>
#include <fstream>
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
  bool experimental_heuristic = false;
  bool no_quality_gate = false;
  size_t max_placeholder_ifs = 0;
  double max_indirect_call_ratio = 1.0;
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
    } else if (args[i] == "--experimental-heuristic") {
      opt.experimental_heuristic = true;
    } else if (args[i] == "--no-quality-gate") {
      opt.no_quality_gate = true;
    } else if (args[i] == "--max-placeholder-ifs" && i + 1 < args.size()) {
      opt.max_placeholder_ifs = static_cast<size_t>(std::stoull(args[++i]));
    } else if (args[i] == "--max-indirect-call-ratio" && i + 1 < args.size()) {
      opt.max_indirect_call_ratio = std::stod(args[++i]);
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

bool IsRegisterName(const std::string& token) {
  if (token.size() < 2) {
    return false;
  }
  if (token[0] != 'x' && token[0] != 'w') {
    return false;
  }
  for (size_t i = 1; i < token.size(); ++i) {
    if (!std::isdigit(static_cast<unsigned char>(token[i]))) {
      return false;
    }
  }
  return true;
}

std::string NormalizeTarget(std::string t) {
  if (!t.empty() && t[0] == '#') {
    t.erase(t.begin());
  }
  return t;
}

struct QualityMetrics {
  bool adapter_backed_model = false;
  size_t function_count = 0;
  size_t disassembled_function_count = 0;
  size_t total_calls = 0;
  size_t indirect_calls = 0;
  size_t placeholder_ifs = 0;
  size_t raw_register_calls = 0;
  bool passed = true;
  std::vector<std::string> failures;
};

QualityMetrics EvaluateQuality(const core::model::Program& program,
                               const std::vector<core::ir::FunctionIR>& irs,
                               const std::filesystem::path& pseudocode_dir,
                               const DecompileOptions& opt) {
  QualityMetrics m;
  m.adapter_backed_model = (program.model_source == "adapter");
  m.function_count = program.functions.size();
  m.disassembled_function_count = irs.size();

  for (const auto& fn : irs) {
    for (const auto& bb : fn.blocks) {
      for (const auto& instr : bb.instrs) {
        if (instr.op == core::ir::IROp::Call) {
          m.total_calls += 1;
          const auto t = NormalizeTarget(instr.target);
          if (IsRegisterName(t)) {
            m.indirect_calls += 1;
          }
        }
      }
    }
  }

  for (const auto& entry : std::filesystem::directory_iterator(pseudocode_dir)) {
    if (!entry.is_regular_file()) {
      continue;
    }
    std::ifstream in(entry.path());
    std::string line;
    while (std::getline(in, line)) {
      if (line.find("/* cond */") != std::string::npos) {
        m.placeholder_ifs += 1;
      }
      if (line.find("call(x") != std::string::npos || line.find("call(w") != std::string::npos) {
        m.raw_register_calls += 1;
      }
    }
  }

  if (!opt.experimental_heuristic && !m.adapter_backed_model) {
    m.failures.push_back("strict mode requires adapter-backed model");
  }
  if (m.placeholder_ifs > opt.max_placeholder_ifs) {
    m.failures.push_back("placeholder if-count exceeded threshold");
  }
  if (m.raw_register_calls > 0) {
    m.failures.push_back("raw register calls emitted in pseudocode");
  }
  const double indirect_ratio = m.total_calls == 0 ? 0.0 : static_cast<double>(m.indirect_calls) / m.total_calls;
  if (indirect_ratio > opt.max_indirect_call_ratio) {
    m.failures.push_back("indirect call ratio exceeded threshold");
  }

  m.passed = m.failures.empty();
  return m;
}

nlohmann::json QualityToJson(const QualityMetrics& q, const core::model::Program& program,
                             const DecompileOptions& opt) {
  const double indirect_ratio = q.total_calls == 0 ? 0.0 : static_cast<double>(q.indirect_calls) / q.total_calls;
  return {
      {"mode", opt.experimental_heuristic ? "experimental-heuristic" : "strict"},
      {"program_model", program.model_source},
      {"adapter_backed_model", q.adapter_backed_model},
      {"function_count", q.function_count},
      {"disassembled_function_count", q.disassembled_function_count},
      {"total_calls", q.total_calls},
      {"indirect_calls", q.indirect_calls},
      {"indirect_call_ratio", indirect_ratio},
      {"placeholder_ifs", q.placeholder_ifs},
      {"raw_register_calls", q.raw_register_calls},
      {"passed", q.passed},
      {"failures", q.failures},
  };
}

}  // namespace

int RunDecompile(const std::vector<std::string>& args) {
  auto opt = ParseOptions(args);
  if (!opt.has_value()) {
    std::cerr
        << "usage: flutterdec decompile <libapp.so|apk> -o out/ [options]\n"
        << "  --experimental-heuristic      allow fallback without adapter (lower correctness)\n"
        << "  --no-quality-gate             do not fail command on quality checks\n"
        << "  --max-placeholder-ifs N       strict threshold (default 0)\n"
        << "  --max-indirect-call-ratio R   strict threshold (default 1.0)\n";
    return 2;
  }

  auto ctx_or = BuildPipeline(opt->input, true, opt->experimental_heuristic);
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

  auto quality = EvaluateQuality(ctx.program, all_irs, pseudocode_dir, *opt);
  const auto quality_json = QualityToJson(quality, ctx.program, *opt);
  util::WriteFile(opt->out_dir / "quality.json", quality_json.dump(2));

  if (!opt->no_quality_gate && !quality.passed) {
    std::cerr << "error: quality gate failed:\n";
    for (const auto& reason : quality.failures) {
      std::cerr << "  - " << reason << "\n";
    }
    std::cerr << "See quality report: " << (opt->out_dir / "quality.json").string() << "\n";
    return 1;
  }

  core::naming::WriteNamesMap(ctx.program, map_dir / "names.json");
  core::exporting::WriteProgramReport(ctx.program, opt->out_dir / "report.json");

  std::cout << "decompile complete: " << opt->out_dir.string() << "\n";
  return 0;
}

}  // namespace flutterdec::cli::commands
