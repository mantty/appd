#include "appd_bare.h"

int
main(void) {
  return appd_bare_runtime_resume(NULL) == 0;
}
