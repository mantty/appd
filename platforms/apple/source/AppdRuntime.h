#ifndef APPD_RUNTIME_H
#define APPD_RUNTIME_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
  APPD_DECISION_DEFAULT = 0,
  APPD_DECISION_CANCEL = 1,
  APPD_DECISION_USE = 2,
};

typedef struct {
  uint8_t *data;
  size_t len;
} AppdBytes;

typedef struct {
  AppdBytes certificate;
  AppdBytes private_key;
} AppdIdentity;

void *appd_runtime_start(const char *packaged_dir, const char *state_dir,
                         const char *host, char *error, size_t error_len);
uint16_t appd_runtime_port(const void *runtime);
uint16_t appd_runtime_restore_gateway(const void *runtime, char *error,
                                      size_t error_len);
bool appd_runtime_suspend(const void *runtime);
bool appd_runtime_resume(const void *runtime);
void appd_runtime_stop(void *runtime);

int32_t appd_runtime_server_authority(const void *runtime, const char *host,
                                      AppdBytes *authority);
int32_t appd_runtime_client_identity(const void *runtime, const char *host,
                                     size_t previous_failures,
                                     AppdIdentity *identity);
void appd_bytes_free(AppdBytes bytes);
void appd_identity_free(AppdIdentity identity);

#endif
