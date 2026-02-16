cmake_minimum_required(VERSION 3.24)
project(flutterdec VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

option(FLUTTERDEC_BUILD_TESTS "Build tests" ON)
option(FLUTTERDEC_FETCH_DEPS "Fetch third-party dependencies" ON)
option(FLUTTERDEC_ENABLE_CAPSTONE "Enable Capstone disassembly" ON)

include(FetchContent)

if(FLUTTERDEC_FETCH_DEPS)
  FetchContent_Declare(
    nlohmann_json
    GIT_REPOSITORY https://github.com/nlohmann/json.git
    GIT_TAG v3.11.3
  )
  FetchContent_MakeAvailable(nlohmann_json)

  FetchContent_Declare(
    elfio
    GIT_REPOSITORY https://github.com/serge1/ELFIO.git
    GIT_TAG Release_3.12
  )
  FetchContent_MakeAvailable(elfio)

  if(FLUTTERDEC_ENABLE_CAPSTONE)
    FetchContent_Declare(
      capstone
      GIT_REPOSITORY https://github.com/capstone-engine/capstone.git
      GIT_TAG 5.0.1
    )
    set(CAPSTONE_BUILD_TESTS OFF CACHE BOOL "" FORCE)
    set(CAPSTONE_BUILD_CSTOOL OFF CACHE BOOL "" FORCE)
    set(CAPSTONE_BUILD_SHARED OFF CACHE BOOL "" FORCE)
    FetchContent_MakeAvailable(capstone)
  endif()
endif()

add_subdirectory(src)

if(FLUTTERDEC_BUILD_TESTS)
  enable_testing()
  add_subdirectory(tests)
endif()
