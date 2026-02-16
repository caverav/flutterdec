#include "core/disasm/CapstoneDisassembler.h"

#include <algorithm>
#include <cctype>
#include <iomanip>
#include <sstream>
#include <string>
#include <vector>

#if defined(FLUTTERDEC_HAVE_CAPSTONE)
#include <capstone/capstone.h>
#endif

namespace flutterdec::core::disasm {
namespace {

util::StatusOr<std::vector<uint8_t>> SliceFunctionBytes(const loader::BinaryImage& image,
                                                        const model::FunctionInfo& fn,
                                                        uint64_t* start_va) {
  *start_va = fn.entry_va;
  auto off_or = image.VaToFileOffset(fn.entry_va);
  if (!off_or.ok()) {
    return off_or.status();
  }
  const size_t file_off = off_or.value();
  if (file_off >= image.elf_bytes.size()) {
    return util::Status::Error(util::ErrorCode::kParseError, "function entry outside ELF byte range");
  }

  uint64_t size = fn.size_bytes;
  if (size == 0) {
    size = 256;
  }
  size = std::min<uint64_t>(size, image.elf_bytes.size() - file_off);

  std::vector<uint8_t> out;
  out.insert(out.end(), image.elf_bytes.begin() + static_cast<long>(file_off),
             image.elf_bytes.begin() + static_cast<long>(file_off + size));
  return out;
}

bool ParseTarget(const std::string& s, uint64_t* target) {
  const auto pos = s.find("0x");
  if (pos == std::string::npos) {
    return false;
  }
  std::stringstream ss;
  ss << std::hex << s.substr(pos);
  ss >> *target;
  return !ss.fail();
}

}  // namespace

util::StatusOr<std::vector<AsmInstruction>> CapstoneDisassembler::DisassembleFunction(
    const loader::BinaryImage& image, const model::FunctionInfo& fn) const {
  uint64_t start_va = 0;
  auto bytes_or = SliceFunctionBytes(image, fn, &start_va);
  if (!bytes_or.ok()) {
    return bytes_or.status();
  }
  const auto& bytes = bytes_or.value();

  std::vector<AsmInstruction> out;

#if defined(FLUTTERDEC_HAVE_CAPSTONE)
  csh handle;
  if (cs_open(CS_ARCH_ARM64, CS_MODE_ARM, &handle) != CS_ERR_OK) {
    return util::Status::Error(util::ErrorCode::kInternal, "capstone cs_open failed");
  }
  cs_option(handle, CS_OPT_DETAIL, CS_OPT_OFF);

  cs_insn* insn = nullptr;
  const size_t count = cs_disasm(handle, bytes.data(), bytes.size(), start_va, 0, &insn);
  if (count == 0) {
    cs_close(&handle);
    return util::Status::Error(util::ErrorCode::kParseError, "capstone failed to disassemble function bytes");
  }

  for (size_t i = 0; i < count; ++i) {
    AsmInstruction ai;
    ai.va = insn[i].address;
    ai.mnemonic = insn[i].mnemonic;
    ai.op_str = insn[i].op_str;

    const std::string m = ai.mnemonic;
    ai.is_call = (m == "bl" || m == "blr");
    ai.is_return = (m == "ret");
    ai.is_branch = (m == "b" || m.rfind("b.", 0) == 0 || m == "cbz" || m == "cbnz" || m == "tbz" || m == "tbnz");
    ai.is_conditional_branch = (m.rfind("b.", 0) == 0 || m == "cbz" || m == "cbnz" || m == "tbz" || m == "tbnz");

    if ((ai.is_branch || ai.is_call) && ParseTarget(ai.op_str, &ai.branch_target)) {
      // parsed branch target
    }

    out.push_back(std::move(ai));
  }
  cs_free(insn, count);
  cs_close(&handle);
#else
  for (size_t i = 0; i + 4 <= bytes.size(); i += 4) {
    std::ostringstream op;
    op << "0x" << std::hex << std::setfill('0') << std::setw(8)
       << static_cast<unsigned int>(bytes[i]) << static_cast<unsigned int>(bytes[i + 1])
       << static_cast<unsigned int>(bytes[i + 2]) << static_cast<unsigned int>(bytes[i + 3]);
    AsmInstruction ai;
    ai.va = start_va + i;
    ai.mnemonic = ".word";
    ai.op_str = op.str();
    out.push_back(std::move(ai));
  }
#endif

  return out;
}

}  // namespace flutterdec::core::disasm
