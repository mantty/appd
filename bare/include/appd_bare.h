#ifndef APPD_BARE_H
#define APPD_BARE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct appd_bare_runtime_s appd_bare_runtime_t;

int appd_bare_runtime_start(
  const uint8_t *bundle,
  size_t bundle_len,
  const uint8_t *config,
  size_t config_len,
  appd_bare_runtime_t **runtime,
  uint16_t *port,
  char *error,
  size_t error_len
);

int appd_bare_runtime_suspend(appd_bare_runtime_t *runtime, int linger);

int appd_bare_runtime_resume(appd_bare_runtime_t *runtime);

void appd_bare_runtime_terminate(appd_bare_runtime_t *runtime);

#ifdef __cplusplus
}
#endif

#endif
