#include "util/Hash.h"

#include <iomanip>
#include <sstream>

namespace flutterdec::util {

std::string Fnv1a64Hex(const std::vector<uint8_t>& data) {
  constexpr uint64_t kOffsetBasis = 14695981039346656037ull;
  constexpr uint64_t kPrime = 1099511628211ull;
  uint64_t h = kOffsetBasis;
  for (uint8_t b : data) {
    h ^= b;
    h *= kPrime;
  }
  std::ostringstream oss;
  oss << std::hex << std::setfill('0') << std::setw(16) << h;
  return oss.str();
}

}  // namespace flutterdec::util
