#pragma once

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
  APPD_WORKERD_OK = 0,
  APPD_WORKERD_ERROR = 1,
};

// Start workerd on a listener supplied by the native appd runtime.
//
// Blocks the calling thread until workerd exits. workerd takes ownership of
// listener_fd. On Unix this is a file descriptor; on Windows it is a SOCKET.
int appd_workerd_serve(
    const char* config_path, const char* working_dir, uintptr_t listener_fd);

// Wait until appd_workerd_serve has either finished startup or failed.
//
// Returns APPD_WORKERD_OK when the listener has been handed to workerd and the
// V8 runtime has been initialized, otherwise APPD_WORKERD_ERROR.
int appd_workerd_wait_ready(void);

#ifdef __cplusplus
}
#endif
