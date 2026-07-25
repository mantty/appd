#include "appd_bare.h"

#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <uv.h>

#include "worklet.h"

struct appd_bare_runtime_s {
  bare_worklet_t worklet;
};

#define APPD_BARE_START_ATTEMPTS 100
#define APPD_BARE_START_DELAY_MS 10

typedef struct appd_bare_start_s {
  uv_mutex_t mutex;
  uv_cond_t condition;
  bool done;
  char error[512];
  char reply[32];
} appd_bare_start_t;

static void
appd_bare_on_start(bare_worklet_push_t *request, const char *error, const uv_buf_t *reply) {
  appd_bare_start_t *start = bare_worklet_push_get_data(request);

  uv_mutex_lock(&start->mutex);

  if (error != NULL) snprintf(start->error, sizeof(start->error), "%s", error);

  if (reply != NULL) {
    size_t len = reply->len < sizeof(start->reply) - 1 ? reply->len : sizeof(start->reply) - 1;
    memcpy(start->reply, reply->base, len);
    start->reply[len] = '\0';
  }

  start->done = true;
  uv_cond_signal(&start->condition);
  uv_mutex_unlock(&start->mutex);
}

static int
appd_bare_wait_for_start(appd_bare_start_t *start) {
  uv_mutex_lock(&start->mutex);
  while (!start->done) uv_cond_wait(&start->condition, &start->mutex);
  uv_mutex_unlock(&start->mutex);
  return 0;
}

static void
appd_bare_reset_start(appd_bare_start_t *start) {
  start->done = false;
  start->error[0] = '\0';
  start->reply[0] = '\0';
}

static int
appd_bare_parse_port(const appd_bare_start_t *start, uint16_t *port) {
  char *end;
  errno = 0;
  unsigned long value = strtoul(start->reply, &end, 10);

  if (errno != 0 || end == start->reply || *end != '\0' || value == 0 || value > UINT16_MAX) {
    return UV_EINVAL;
  }

  *port = (uint16_t) value;
  return 0;
}

static void
appd_bare_copy_error(char *destination, size_t len, const char *source) {
  if (destination == NULL || len == 0) return;
  snprintf(destination, len, "%s", source);
}

int
appd_bare_runtime_start(
  const uint8_t *bundle,
  size_t bundle_len,
  const uint8_t *config,
  size_t config_len,
  appd_bare_runtime_t **runtime,
  uint16_t *port,
  char *error,
  size_t error_len
) {
  if (
    bundle == NULL ||
    bundle_len == 0 ||
    bundle_len > UINT_MAX ||
    config == NULL ||
    config_len > UINT_MAX ||
    runtime == NULL ||
    port == NULL
  ) {
    appd_bare_copy_error(error, error_len, "Invalid Bare startup arguments");
    return UV_EINVAL;
  }

  appd_bare_runtime_t *instance = calloc(1, sizeof(*instance));
  if (instance == NULL) return UV_ENOMEM;

  bare_worklet_options_t options = {0};
  int result = bare_worklet_init(&instance->worklet, &options);
  if (result != 0) goto fail;

  uv_buf_t source = uv_buf_init((char *) bundle, (unsigned int) bundle_len);
  result = bare_worklet_start(&instance->worklet, "appd.bundle", &source, 0, NULL);
  if (result != 0) goto fail_initialized;

  appd_bare_start_t start = {0};
  uv_mutex_init(&start.mutex);
  uv_cond_init(&start.condition);

  uv_buf_t payload = uv_buf_init((char *) config, (unsigned int) config_len);
  result = UV_EINVAL;

  for (int attempt = 0; attempt < APPD_BARE_START_ATTEMPTS; attempt++) {
    bare_worklet_push_t request = {0};
    bare_worklet_push_set_data(&request, &start);
    appd_bare_reset_start(&start);

    result = bare_worklet_push(&instance->worklet, &request, &payload, appd_bare_on_start);
    if (result != 0) break;

    appd_bare_wait_for_start(&start);
    if (start.error[0] != '\0') {
      result = UV_EINVAL;
      break;
    }
    if (start.reply[0] != '\0') {
      result = appd_bare_parse_port(&start, port);
      break;
    }

    uv_sleep(APPD_BARE_START_DELAY_MS);
  }

  if (result != 0) {
    appd_bare_copy_error(error, error_len, start.error[0] == '\0' ? "Bare startup timed out" : start.error);
  }

  uv_cond_destroy(&start.condition);
  uv_mutex_destroy(&start.mutex);
  if (result != 0) goto fail_started;

  *runtime = instance;
  return 0;

fail_started:
  bare_worklet_terminate(&instance->worklet);
  bare_worklet_destroy(&instance->worklet);
  free(instance);
  return result;

fail_initialized:
  bare_worklet_destroy(&instance->worklet);
fail:
  free(instance);
  appd_bare_copy_error(error, error_len, "Bare worklet initialization failed");
  return result;
}

int
appd_bare_runtime_suspend(appd_bare_runtime_t *runtime, int linger) {
  if (runtime == NULL) return UV_EINVAL;
  return bare_worklet_suspend(&runtime->worklet, linger);
}

int
appd_bare_runtime_resume(appd_bare_runtime_t *runtime) {
  if (runtime == NULL) return UV_EINVAL;
  return bare_worklet_resume(&runtime->worklet);
}

void
appd_bare_runtime_terminate(appd_bare_runtime_t *runtime) {
  if (runtime == NULL) return;
  bare_worklet_terminate(&runtime->worklet);
  bare_worklet_destroy(&runtime->worklet);
  free(runtime);
}
