#pragma once

#include <string>

namespace flutterdec::util {

enum class LogLevel { kError = 0, kWarn, kInfo, kDebug };

void SetLogLevel(LogLevel level);
void Log(LogLevel level, const std::string& msg);

}  // namespace flutterdec::util
