#pragma once

#include <optional>
#include <string>

namespace flutterdec::util {

enum class ErrorCode {
  kOk = 0,
  kInvalidArgument,
  kNotFound,
  kParseError,
  kIoError,
  kExternalToolError,
  kUnsupported,
  kInternal,
};

struct Status {
  ErrorCode code = ErrorCode::kOk;
  std::string message;

  static Status Ok() { return {}; }
  static Status Error(ErrorCode c, std::string m) { return Status{c, std::move(m)}; }
  [[nodiscard]] bool ok() const { return code == ErrorCode::kOk; }
};

template <typename T>
class StatusOr {
 public:
  StatusOr(T value) : status_(Status::Ok()), value_(std::move(value)) {}
  StatusOr(Status status) : status_(std::move(status)), value_(std::nullopt) {}

  [[nodiscard]] bool ok() const { return status_.ok() && value_.has_value(); }
  [[nodiscard]] const Status& status() const { return status_; }
  [[nodiscard]] const T& value() const { return *value_; }
  [[nodiscard]] T& value() { return *value_; }

 private:
  Status status_;
  std::optional<T> value_;
};

}  // namespace flutterdec::util
