#pragma once

#include <cstdlib>
#include <iostream>

#define TASSERT(cond)                                                                 \
  do {                                                                                \
    if (!(cond)) {                                                                    \
      std::cerr << "assertion failed: " #cond << " at " << __FILE__ << ":" << __LINE__ \
                << "\n";                                                            \
      return 1;                                                                       \
    }                                                                                 \
  } while (0)
