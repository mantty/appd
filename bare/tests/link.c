#include <stdlib.h>

#include "worklet.h"

int
main(void) {
  bare_worklet_t *worklet = NULL;
  int result = bare_worklet_alloc(&worklet);
  free(worklet);
  return result;
}
