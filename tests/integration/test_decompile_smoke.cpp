#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>

int main() {
  namespace fs = std::filesystem;

  const fs::path fixture = "tests/fixtures/sample_app_unobf_arm64/libapp.so";
  if (!fs::exists(fixture)) {
    std::cout << "skipping integration test: fixture missing\n";
    return 0;
  }

  const fs::path fake_json = fs::temp_directory_path() / "flutterdec_fake_adapter.json";
  {
    std::ofstream out(fake_json);
    out << R"({
      "schema_version":1,
      "dart_version":"3.7.2",
      "snapshot_hash":"fixture_hash_v1",
      "arch":"arm64",
      "object_pool":[{"i":0,"kind":"String","s":"Hello"}],
      "classes":[{"id":1,"name":"A","super":"Object","lib":"package:app/main.dart"}],
      "functions":[{"id":1,"name":"a","owner_class":"A","entry_va":4096,"size":64}]
    })";
  }

  const fs::path out_dir = fs::temp_directory_path() / "flutterdec_smoke_out";
  std::string cmd = std::string("FLUTTERDEC_FAKE_ADAPTER_JSON='") + fake_json.string() +
                    "' " + FLUTTERDEC_BIN_PATH +
                    " decompile '" + fixture.string() + "' -o '" + out_dir.string() + "' --emit-asm --emit-ir";

  const int rc = std::system(cmd.c_str());
  if (rc != 0) {
    std::cerr << "decompile command failed\n";
    return 1;
  }

  if (!fs::exists(out_dir / "report.json")) {
    std::cerr << "missing report.json\n";
    return 1;
  }
  if (!fs::exists(out_dir / "maps" / "names.json")) {
    std::cerr << "missing names.json\n";
    return 1;
  }

  return 0;
}
