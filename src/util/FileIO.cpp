#include "util/FileIO.h"

#include <fstream>

namespace flutterdec::util {

StatusOr<std::vector<uint8_t>> ReadFile(const std::filesystem::path& path) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    return Status::Error(ErrorCode::kIoError, "failed to open file: " + path.string());
  }
  in.seekg(0, std::ios::end);
  std::streamsize size = in.tellg();
  in.seekg(0, std::ios::beg);
  std::vector<uint8_t> buf(static_cast<size_t>(size));
  if (size > 0 && !in.read(reinterpret_cast<char*>(buf.data()), size)) {
    return Status::Error(ErrorCode::kIoError, "failed to read file: " + path.string());
  }
  return buf;
}

Status WriteFile(const std::filesystem::path& path, const std::string& data) {
  std::ofstream out(path, std::ios::binary);
  if (!out) {
    return Status::Error(ErrorCode::kIoError, "failed to open output: " + path.string());
  }
  out.write(data.data(), static_cast<std::streamsize>(data.size()));
  return Status::Ok();
}

Status WriteFileBytes(const std::filesystem::path& path, const std::vector<uint8_t>& data) {
  std::ofstream out(path, std::ios::binary);
  if (!out) {
    return Status::Error(ErrorCode::kIoError, "failed to open output: " + path.string());
  }
  if (!data.empty()) {
    out.write(reinterpret_cast<const char*>(data.data()), static_cast<std::streamsize>(data.size()));
  }
  return Status::Ok();
}

Status EnsureDir(const std::filesystem::path& path) {
  std::error_code ec;
  std::filesystem::create_directories(path, ec);
  if (ec) {
    return Status::Error(ErrorCode::kIoError, "failed to create directory: " + path.string() + " : " + ec.message());
  }
  return Status::Ok();
}

}  // namespace flutterdec::util
