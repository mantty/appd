#include "appd_bare.h"

#include <errno.h>
#include <limits.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <uv.h>

#include "ipc.h"
#include "worklet.h"

struct appd_bare_runtime_s {
  bare_worklet_t worklet;
  bare_ipc_t ipc;
};

#define APPD_BARE_STARTUP_TIMEOUT_MS 30000
#define APPD_BARE_REPLY_CAPACITY 1024

static const char appd_bare_listening[] = "listening ";
static const char appd_bare_failed[] = "error ";

static void
appd_bare_copy_error(char *destination, size_t len, const char *source) {
  if (destination == NULL || len == 0) return;
  snprintf(destination, len, "%s", source);
}

static int
appd_bare_wait(int descriptor, short events) {
  struct pollfd waiting = {.fd = descriptor, .events = events, .revents = 0};

  for (;;) {
    int ready = poll(&waiting, 1, APPD_BARE_STARTUP_TIMEOUT_MS);

    if (ready > 0) return 0;
    if (ready == 0) return UV_ETIMEDOUT;
    if (errno != EINTR) return uv_translate_sys_error(errno);
  }
}

static int
appd_bare_write_all(bare_ipc_t *ipc, const char *data, size_t len) {
  size_t sent = 0;

  while (sent < len) {
    int result = bare_ipc_write(ipc, data + sent, len - sent);

    if (result == bare_ipc_would_block) {
      int err = appd_bare_wait(bare_ipc_get_outgoing(ipc), POLLOUT);
      if (err != 0) return err;
      continue;
    }

    if (result < 0) return UV_EIO;

    sent += (size_t) result;
  }

  return 0;
}

static int
appd_bare_read_line(bare_ipc_t *ipc, char *line, size_t capacity) {
  size_t used = 0;

  for (;;) {
    void *data;
    size_t len;
    int result = bare_ipc_read(ipc, &data, &len);

    if (result == bare_ipc_would_block) {
      int err = appd_bare_wait(bare_ipc_get_incoming(ipc), POLLIN);
      if (err != 0) return err;
      continue;
    }

    if (result != 0) return UV_EIO;
    if (len == 0) return UV_EPIPE;

    const char *bytes = data;

    for (size_t i = 0; i < len; i++) {
      if (bytes[i] == '\n') {
        line[used] = '\0';
        return 0;
      }

      if (used + 1 >= capacity) return UV_E2BIG;

      line[used++] = bytes[i];
    }
  }
}

static int
appd_bare_parse_port(const char *value, uint16_t *port) {
  char *end;
  errno = 0;
  unsigned long parsed = strtoul(value, &end, 10);

  if (errno != 0 || end == value || *end != '\0' || parsed == 0 || parsed > UINT16_MAX) {
    return UV_EINVAL;
  }

  *port = (uint16_t) parsed;

  return 0;
}

static int
appd_bare_send_config(bare_ipc_t *ipc, const uint8_t *config, size_t config_len) {
  char *request = malloc(config_len + 1);

  if (request == NULL) return UV_ENOMEM;

  memcpy(request, config, config_len);
  request[config_len] = '\n';

  int err = appd_bare_write_all(ipc, request, config_len + 1);

  free(request);

  return err;
}

static int
appd_bare_await_port(bare_ipc_t *ipc, uint16_t *port, char *error, size_t error_len) {
  char reply[APPD_BARE_REPLY_CAPACITY];
  int err = appd_bare_read_line(ipc, reply, sizeof(reply));

  if (err == UV_ETIMEDOUT) {
    appd_bare_copy_error(error, error_len, "Bare startup timed out");
    return err;
  }

  if (err != 0) {
    appd_bare_copy_error(error, error_len, "Bare stopped during startup");
    return err;
  }

  size_t failed_len = sizeof(appd_bare_failed) - 1;

  if (strncmp(reply, appd_bare_failed, failed_len) == 0) {
    appd_bare_copy_error(error, error_len, reply + failed_len);
    return UV_EINVAL;
  }

  size_t listening_len = sizeof(appd_bare_listening) - 1;

  if (strncmp(reply, appd_bare_listening, listening_len) != 0) {
    appd_bare_copy_error(error, error_len, "Bare sent an unexpected startup reply");
    return UV_EINVAL;
  }

  err = appd_bare_parse_port(reply + listening_len, port);

  if (err != 0) appd_bare_copy_error(error, error_len, "Bare sent an invalid port");

  return err;
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
    config_len == 0 ||
    config_len > UINT_MAX ||
    runtime == NULL ||
    port == NULL
  ) {
    appd_bare_copy_error(error, error_len, "Invalid Bare startup arguments");
    return UV_EINVAL;
  }

  appd_bare_runtime_t *instance = calloc(1, sizeof(*instance));

  if (instance == NULL) {
    appd_bare_copy_error(error, error_len, "Bare runtime allocation failed");
    return UV_ENOMEM;
  }

  bare_worklet_options_t options = {0};
  int err = bare_worklet_init(&instance->worklet, &options);

  if (err != 0) {
    appd_bare_copy_error(error, error_len, "Bare worklet initialization failed");
    goto fail;
  }

  uv_buf_t source = uv_buf_init((char *) bundle, (unsigned int) bundle_len);

  // Returns once the worklet's IPC is connected and before the app bundle
  // runs, so the configuration below is buffered rather than raced.
  err = bare_worklet_start(&instance->worklet, "appd.bundle", &source, 0, NULL);

  if (err != 0) {
    appd_bare_copy_error(error, error_len, "Bare worklet failed to start");
    goto fail_initialized;
  }

  err = bare_ipc_init(&instance->ipc, &instance->worklet);

  if (err != 0) {
    appd_bare_copy_error(error, error_len, "Bare IPC initialization failed");
    goto fail_started;
  }

  err = appd_bare_send_config(&instance->ipc, config, config_len);

  if (err != 0) {
    appd_bare_copy_error(error, error_len, "Bare startup configuration could not be sent");
    goto fail_connected;
  }

  err = appd_bare_await_port(&instance->ipc, port, error, error_len);

  if (err != 0) goto fail_connected;

  *runtime = instance;

  return 0;

fail_connected:
  bare_ipc_destroy(&instance->ipc);
fail_started:
  bare_worklet_terminate(&instance->worklet);
fail_initialized:
  bare_worklet_destroy(&instance->worklet);
fail:
  free(instance);

  return err;
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
  bare_ipc_destroy(&runtime->ipc);
  bare_worklet_terminate(&runtime->worklet);
  bare_worklet_destroy(&runtime->worklet);
  free(runtime);
}
