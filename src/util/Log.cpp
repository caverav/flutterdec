#include "util/Log.h"

#include <iostream>
#include <mutex>

namespace flutterdec::util {
namespace {
std::mutex g_log_mu;
LogLevel g_level = LogLevel::kInfo;

const char* ToTag(LogLevel level) {
  switch (level) {
    case LogLevel::kError:
      return "ERROR";
    case LogLevel::kWarn:
      return "WARN";
    case LogLevel::kInfo:
      return "INFO";
    case LogLevel::kDebug:
      return "DEBUG";
  }
  return "UNKNOWN";
}
}  // namespace

void SetLogLevel(LogLevel level) {
  std::scoped_lock lk(g_log_mu);
  g_level = level;
}

void Log(LogLevel level, const std::string& msg) {
  std::scoped_lock lk(g_log_mu);
  if (static_cast<int>(level) > static_cast<int>(g_level)) {
    return;
  }
  std::cerr << "[" << ToTag(level) << "] " << msg << "\n";
}

}  // namespace flutterdec::util
