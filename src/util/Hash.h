#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace flutterdec::util {

std::string Fnv1a64Hex(const std::vector<uint8_t>& data);

}  // namespace flutterdec::util
