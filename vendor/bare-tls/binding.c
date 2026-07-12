#if !defined(_WIN32) && !defined(_GNU_SOURCE)
#define _GNU_SOURCE
#endif

#include <assert.h>
#include <bare.h>
#include <js.h>
#include <limits.h>
#include <openssl/base.h>
#include <openssl/bio.h>
#include <openssl/err.h>
#include <openssl/evp.h>
#include <openssl/mem.h>
#include <openssl/ssl.h>
#include <openssl/x509.h>
#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32)
#include <windows.h>
#else
#include <dlfcn.h>
#endif

#include "certificates.h"

static void
bare_tls__pin(void) {
#if defined(_WIN32)
  HMODULE module;
  GetModuleHandleExA(
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
    (LPCSTR) &bare_tls__pin,
    &module
  );
#else
  Dl_info info;
  if (dladdr((void *) &bare_tls__pin, &info) && info.dli_fname) {
    dlopen(info.dli_fname, RTLD_LAZY | RTLD_NOLOAD | RTLD_NODELETE);
  }
#endif
}

typedef struct {
  SSL_CTX *ssl;
  BIO_METHOD *io;

  js_env_t *env;
  js_ref_t *ctx;
} bare_tls_context_t;

typedef struct {
  SSL *ssl;
  BIO *io;
  X509 *certificate;
  EVP_PKEY *key;

  uint8_t *alpn;
  size_t alpn_len;

  js_env_t *env;
  js_ref_t *ctx;
  js_ref_t *on_read;
  js_ref_t *on_write;
} bare_tls_t;

static int
bare_tls__on_read(BIO *io, char *buffer, int len) {
  if (len <= 0) return 0;

  int err;

  BIO_clear_retry_flags(io);

  bare_tls_t *socket = BIO_get_ex_data(io, 0);

  js_env_t *env = socket->env;

  js_value_t *arraybuffer;
  err = js_create_external_arraybuffer(env, (void *) buffer, len, NULL, NULL, &arraybuffer);
  assert(err == 0);

  js_value_t *typedarray;
  err = js_create_typedarray(env, js_uint8array, len, arraybuffer, 0, &typedarray);
  assert(err == 0);

  js_value_t *ctx;
  err = js_get_reference_value(env, socket->ctx, &ctx);
  assert(err == 0);

  js_value_t *on_read;
  err = js_get_reference_value(env, socket->on_read, &on_read);
  assert(err == 0);

  js_value_t *result;
  err = js_call_function(env, ctx, on_read, 1, &typedarray, &result);
  if (err < 0) return -1;

  err = js_get_value_int32(env, result, &len);
  assert(err == 0);

  err = js_detach_arraybuffer(env, arraybuffer);
  assert(err == 0);

  if (len == 0) {
    BIO_set_retry_read(io);

    return -1;
  }

  return len;
}

static int
bare_tls__on_write(BIO *io, const char *buffer, int len) {
  if (len <= 0) return 0;

  int err;

  BIO_clear_retry_flags(io);

  bare_tls_t *socket = BIO_get_ex_data(io, 0);

  js_env_t *env = socket->env;

  js_value_t *arraybuffer;
  err = js_create_external_arraybuffer(env, (void *) buffer, len, NULL, NULL, &arraybuffer);
  assert(err == 0);

  js_value_t *typedarray;
  err = js_create_typedarray(env, js_uint8array, len, arraybuffer, 0, &typedarray);
  assert(err == 0);

  js_value_t *ctx;
  err = js_get_reference_value(env, socket->ctx, &ctx);
  assert(err == 0);

  js_value_t *on_write;
  err = js_get_reference_value(env, socket->on_write, &on_write);
  assert(err == 0);

  js_value_t *result;
  err = js_call_function(env, ctx, on_write, 1, &typedarray, &result);
  if (err < 0) return -1;

  err = js_get_value_int32(env, result, &len);
  assert(err == 0);

  err = js_detach_arraybuffer(env, arraybuffer);
  assert(err == 0);

  if (len == 0) {
    BIO_set_retry_write(io);

    return -1;
  }

  return len;
}

static long
bare_tls__on_ctrl(BIO *io, int cmd, long argc, void *argv) {
  switch (cmd) {
  case BIO_CTRL_FLUSH:
    return 1;

  default:
    return 0;
  }
}

static int
bare_tls__on_alpn_select(SSL *ssl, const uint8_t **out, uint8_t *outlen, const uint8_t *in, unsigned int inlen, void *arg) {
  bare_tls_t *socket = (bare_tls_t *) SSL_get_ex_data(ssl, 0);

  if (socket == NULL || socket->alpn == NULL || socket->alpn_len == 0) {
    return SSL_TLSEXT_ERR_NOACK;
  }

  if (SSL_select_next_proto((uint8_t **) out, outlen, socket->alpn, (unsigned int) socket->alpn_len, in, inlen) != OPENSSL_NPN_NEGOTIATED) {
    return SSL_TLSEXT_ERR_ALERT_FATAL;
  }

  return SSL_TLSEXT_ERR_OK;
}

static void
bare_tls__on_teardown(void *data) {
  int err;

  bare_tls_context_t *context = (bare_tls_context_t *) data;

  js_env_t *env = context->env;

  SSL_CTX_free(context->ssl);

  BIO_meth_free(context->io);

  err = js_delete_reference(env, context->ctx);
  assert(err == 0);
}

static js_value_t *
bare_tls_context(js_env_t *env, js_callback_info_t *info) {
  int err;

  js_value_t *handle;

  bare_tls_context_t *context;
  err = js_create_arraybuffer(env, sizeof(bare_tls_context_t), (void **) &context, &handle);
  assert(err == 0);

  BIO_METHOD *io = context->io = BIO_meth_new(BIO_get_new_index() | BIO_TYPE_SOURCE_SINK, "callback");

  if (io == NULL) goto err;

  SSL_CTX *ssl = context->ssl = SSL_CTX_new(TLS_method());

  if (ssl == NULL) {
    BIO_meth_free(io);

    goto err;
  }

  err = BIO_meth_set_read(io, bare_tls__on_read);
  assert(err == 1);

  err = BIO_meth_set_write(io, bare_tls__on_write);
  assert(err == 1);

  err = BIO_meth_set_ctrl(io, bare_tls__on_ctrl);
  assert(err == 1);

  err = SSL_CTX_set_ex_data(ssl, 0, (void *) context);
  assert(err == 1);

  err = SSL_CTX_set_min_proto_version(ssl, TLS1_2_VERSION);
  assert(err == 1);

  SSL_CTX_set_options(ssl, SSL_OP_NO_TICKET);

  BIO *bio = BIO_new_mem_buf(bare_tls__certs, (int) bare_tls__certs_len);
  assert(bio != NULL);

  X509_STORE *store = SSL_CTX_get_cert_store(ssl);
  assert(store != NULL);

  X509 *cert;
  while ((cert = PEM_read_bio_X509(bio, NULL, NULL, NULL)) != NULL) {
    err = X509_STORE_add_cert(store, cert);

    X509_free(cert);

    if (err != 1) {
      BIO_free(bio);

      SSL_CTX_free(ssl);

      BIO_meth_free(io);

      goto err;
    }
  }

  ERR_clear_error();

  err = BIO_free(bio);
  assert(err == 1);

  SSL_CTX_set_alpn_select_cb(ssl, bare_tls__on_alpn_select, NULL);

  context->env = env;

  err = js_add_teardown_callback(env, bare_tls__on_teardown, (void *) context);
  assert(err == 0);

  err = js_create_reference(env, handle, 1, &context->ctx);
  assert(err == 0);

  return handle;

err:
  js_throw_error(env, ERR_reason_symbol_name(ERR_peek_last_error()), "Context initialisation failed");
  return NULL;
}

static js_value_t *
bare_tls_init(js_env_t *env, js_callback_info_t *info) {
  int err;

  size_t argc = 11;
  js_value_t *argv[11];

  err = js_get_callback_info(env, info, &argc, argv, NULL, NULL);
  assert(err == 0);

  assert(argc == 11);

  bare_tls_context_t *context;
  err = js_get_arraybuffer_info(env, argv[0], (void **) &context, NULL);
  assert(err == 0);

  js_value_t *handle;

  bare_tls_t *socket;
  err = js_create_arraybuffer(env, sizeof(bare_tls_t), (void **) &socket, &handle);
  assert(err == 0);

  socket->certificate = NULL;
  socket->key = NULL;
  socket->alpn = NULL;
  socket->alpn_len = 0;

  BIO *io = socket->io = BIO_new(context->io);

  if (io == NULL) goto err;

  SSL *ssl = socket->ssl = SSL_new(context->ssl);

  if (ssl == NULL) {
    BIO_free(io);

    goto err;
  }

  err = BIO_set_ex_data(io, 0, (void *) socket);
  assert(err == 1);

  BIO_set_init(io, true);

  err = SSL_set_ex_data(ssl, 0, (void *) socket);
  assert(err == 1);

  SSL_set_bio(ssl, io, io);

  bool is_server;
  err = js_get_value_bool(env, argv[1], &is_server);
  assert(err == 0);

  bool reject_unauthorized;
  err = js_get_value_bool(env, argv[5], &reject_unauthorized);
  assert(err == 0);

  if (is_server) {
    if (reject_unauthorized) {
      SSL_set_verify(ssl, SSL_VERIFY_PEER | SSL_VERIFY_FAIL_IF_NO_PEER_CERT, NULL);
    }
    SSL_set_accept_state(ssl);
  } else {
    SSL_set_connect_state(ssl);

    if (reject_unauthorized) {
      SSL_set_verify(ssl, SSL_VERIFY_PEER, NULL);
    }
  }

  bool has_cert;
  err = js_is_typedarray(env, argv[2], &has_cert);
  assert(err == 0);

  if (has_cert) {
    char *pem;
    size_t len;
    err = js_get_typedarray_info(env, argv[2], NULL, (void **) &pem, &len, NULL, NULL);
    assert(err == 0);

    if (len > INT_MAX) len = INT_MAX;

    BIO *io = BIO_new(BIO_s_mem());
    assert(io != NULL);

    err = BIO_write(io, pem, (int) len);
    assert(err == (int) len);

    X509 *certificate = socket->certificate = PEM_read_bio_X509(io, NULL, NULL, NULL);

    if (certificate == NULL) {
      BIO_free(io);

      SSL_free(ssl);

      goto err;
    }

    err = SSL_use_certificate(ssl, certificate);

    if (err == 0) {
      BIO_free(io);

      SSL_free(ssl);

      X509_free(certificate);

      goto err;
    }

    X509 *chain_cert;
    while ((chain_cert = PEM_read_bio_X509(io, NULL, NULL, NULL)) != NULL) {
      err = SSL_add1_chain_cert(ssl, chain_cert);

      X509_free(chain_cert);

      if (err == 0) {
        BIO_free(io);

        SSL_free(ssl);

        goto err;
      }
    }

    ERR_clear_error();

    err = BIO_free(io);
    assert(err == 1);
  }

  bool has_key;
  err = js_is_typedarray(env, argv[3], &has_key);
  assert(err == 0);

  if (has_key) {
    char *pem;
    size_t len;
    err = js_get_typedarray_info(env, argv[3], NULL, (void **) &pem, &len, NULL, NULL);
    assert(err == 0);

    if (len > INT_MAX) len = INT_MAX;

    BIO *io = BIO_new(BIO_s_mem());
    assert(io != NULL);

    err = BIO_write(io, pem, (int) len);
    assert(err == (int) len);

    EVP_PKEY *key = socket->key = PEM_read_bio_PrivateKey(io, NULL, NULL, NULL);

    char *mem_data;
    long mem_len = BIO_get_mem_data(io, &mem_data);
    assert(mem_len >= 0);

    OPENSSL_cleanse(mem_data, mem_len);

    err = BIO_free(io);
    assert(err == 1);

    if (key == NULL) {
      SSL_free(ssl);

      goto err;
    }

    int res = SSL_use_PrivateKey(ssl, key);

    if (res == 0) {
      SSL_free(ssl);

      EVP_PKEY_free(key);

      goto err;
    }
  }

  bool has_host;
  err = js_is_string(env, argv[4], &has_host);
  assert(err == 0);

  if (has_host) {
    size_t len;
    err = js_get_value_string_utf8(env, argv[4], NULL, 0, &len);
    assert(err == 0);

    if (len == SIZE_MAX) {
      if (socket->certificate) X509_free(socket->certificate);
      if (socket->key) EVP_PKEY_free(socket->key);

      SSL_free(ssl);

      goto err;
    }

    len += 1 /* NULL */;

    utf8_t *host = malloc(len);
    err = js_get_value_string_utf8(env, argv[4], host, len, NULL);
    assert(err == 0);

    X509_VERIFY_PARAM *param = SSL_get0_param(ssl);
    assert(param != NULL);

    if (X509_VERIFY_PARAM_set1_ip_asc(param, (char *) host) != 1) {
      ERR_clear_error();

      err = X509_VERIFY_PARAM_set1_host(param, (char *) host, len - 1 /* NULL */);

      if (err != 1) {
        free(host);

        if (socket->certificate) X509_free(socket->certificate);
        if (socket->key) EVP_PKEY_free(socket->key);

        SSL_free(ssl);

        goto err;
      }

      err = SSL_set_tlsext_host_name(ssl, (char *) host);

      if (err != 1) {
        free(host);

        if (socket->certificate) X509_free(socket->certificate);
        if (socket->key) EVP_PKEY_free(socket->key);

        SSL_free(ssl);

        goto err;
      }
    }

    free(host);
  }

  bool has_ca;
  err = js_is_typedarray(env, argv[6], &has_ca);
  assert(err == 0);

  if (has_ca) {
    char *pem;
    size_t len;
    err = js_get_typedarray_info(env, argv[6], NULL, (void **) &pem, &len, NULL, NULL);
    assert(err == 0);

    if (len > INT_MAX) len = INT_MAX;

    BIO *io = BIO_new_mem_buf(pem, (int) len);
    assert(io != NULL);

    X509_STORE *store = X509_STORE_new();
    assert(store != NULL);

    X509 *cert;
    while ((cert = PEM_read_bio_X509(io, NULL, NULL, NULL)) != NULL) {
      err = X509_STORE_add_cert(store, cert);

      X509_free(cert);

      if (err != 1) {
        BIO_free(io);

        X509_STORE_free(store);

        if (socket->certificate) X509_free(socket->certificate);
        if (socket->key) EVP_PKEY_free(socket->key);

        SSL_free(ssl);

        goto err;
      }
    }

    ERR_clear_error();

    if (is_server) {
      STACK_OF(X509_NAME) *names = sk_X509_NAME_new_null();
      BIO *names_io = BIO_new_mem_buf(pem, (int) len);
      if (names != NULL && names_io != NULL &&
          SSL_add_bio_cert_subjects_to_stack(names, names_io) == 1) {
        SSL_set_client_CA_list(ssl, names);
      } else if (names != NULL) {
        sk_X509_NAME_pop_free(names, X509_NAME_free);
      }
      if (names_io != NULL) BIO_free(names_io);
    }

    err = BIO_free(io);
    assert(err == 1);

    err = SSL_set0_verify_cert_store(ssl, store);

    if (err != 1) {
      X509_STORE_free(store);

      if (socket->certificate) X509_free(socket->certificate);
      if (socket->key) EVP_PKEY_free(socket->key);

      SSL_free(ssl);

      goto err;
    }
  }

  bool has_alpn;
  err = js_is_typedarray(env, argv[7], &has_alpn);
  assert(err == 0);

  if (has_alpn) {
    uint8_t *alpn;
    size_t len;
    err = js_get_typedarray_info(env, argv[7], NULL, (void **) &alpn, &len, NULL, NULL);
    assert(err == 0);

    if (len > UINT_MAX) len = UINT_MAX;

    if (is_server) {
      socket->alpn = malloc(len);
      socket->alpn_len = len;

      memcpy(socket->alpn, alpn, len);
    } else {
      err = SSL_set_alpn_protos(ssl, alpn, (unsigned int) len);
      assert(err == 0);
    }
  }

  socket->env = env;

  err = js_create_reference(env, argv[8], 1, &socket->ctx);
  assert(err == 0);

  err = js_create_reference(env, argv[9], 1, &socket->on_read);
  assert(err == 0);

  err = js_create_reference(env, argv[10], 1, &socket->on_write);
  assert(err == 0);

  return handle;

err:
  js_throw_error(env, ERR_reason_symbol_name(ERR_peek_last_error()), "Socket initialisation failed");
  return NULL;
}

static js_value_t *
bare_tls_destroy(js_env_t *env, js_callback_info_t *info) {
  int err;

  size_t argc = 1;
  js_value_t *argv[1];

  err = js_get_callback_info(env, info, &argc, argv, NULL, NULL);
  assert(err == 0);

  assert(argc == 1);

  bare_tls_t *socket;
  err = js_get_arraybuffer_info(env, argv[0], (void **) &socket, NULL);
  assert(err == 0);

  SSL_free(socket->ssl);

  if (socket->certificate) X509_free(socket->certificate);

  if (socket->key) EVP_PKEY_free(socket->key);

  if (socket->alpn) free(socket->alpn);

  err = js_delete_reference(env, socket->on_read);
  assert(err == 0);

  err = js_delete_reference(env, socket->on_write);
  assert(err == 0);

  err = js_delete_reference(env, socket->ctx);
  assert(err == 0);

  return NULL;
}

static js_value_t *
bare_tls_handshake(js_env_t *env, js_callback_info_t *info) {
  int err;

  size_t argc = 1;
  js_value_t *argv[1];

  err = js_get_callback_info(env, info, &argc, argv, NULL, NULL);
  assert(err == 0);

  assert(argc == 1);

  bare_tls_t *socket;
  err = js_get_arraybuffer_info(env, argv[0], (void **) &socket, NULL);
  assert(err == 0);

  bool done = true;

  err = SSL_do_handshake(socket->ssl);

  if (err <= 0) {
    err = SSL_get_error(socket->ssl, err);

    if (err == SSL_ERROR_WANT_READ || err == SSL_ERROR_WANT_WRITE) {
      done = false;
    } else {
      js_throw_error(env, ERR_reason_symbol_name(ERR_peek_last_error()), "Handshake failed");
      return NULL;
    }
  }

  js_value_t *result;
  err = js_get_boolean(env, done, &result);
  assert(err == 0);

  return result;
}

static js_value_t *
bare_tls_read(js_env_t *env, js_callback_info_t *info) {
  int err;

  size_t argc = 2;
  js_value_t *argv[2];

  err = js_get_callback_info(env, info, &argc, argv, NULL, NULL);
  assert(err == 0);

  assert(argc == 2);

  bare_tls_t *socket;
  err = js_get_arraybuffer_info(env, argv[0], (void **) &socket, NULL);
  assert(err == 0);

  char *buffer;
  size_t len;
  err = js_get_typedarray_info(env, argv[1], NULL, (void **) &buffer, &len, NULL, NULL);
  assert(err == 0);

  bool retry = false;
  bool eof = false;

  if (len > INT_MAX) len = INT_MAX;

  err = SSL_read(socket->ssl, buffer, (int) len);

  if (err <= 0) {
    err = SSL_get_error(socket->ssl, err);

    if (err == SSL_ERROR_WANT_READ || err == SSL_ERROR_WANT_WRITE) {
      retry = true;
    } else if (SSL_get_shutdown(socket->ssl)) {
      eof = true;
    } else {
      js_throw_error(env, ERR_reason_symbol_name(ERR_peek_last_error()), "Read failed");
      return NULL;
    }
  }

  int res = eof ? 0 : retry ? -1
                            : err;

  js_value_t *result;
  err = js_create_int64(env, res, &result);
  assert(err == 0);

  return result;
}

static js_value_t *
bare_tls_write(js_env_t *env, js_callback_info_t *info) {
  int err;

  size_t argc = 2;
  js_value_t *argv[2];

  err = js_get_callback_info(env, info, &argc, argv, NULL, NULL);
  assert(err == 0);

  assert(argc == 2);

  bare_tls_t *socket;
  err = js_get_arraybuffer_info(env, argv[0], (void **) &socket, NULL);
  assert(err == 0);

  char *buffer;
  size_t len;
  err = js_get_typedarray_info(env, argv[1], NULL, (void **) &buffer, &len, NULL, NULL);
  assert(err == 0);

  bool retry = false;

  if (len > INT_MAX) len = INT_MAX;

  err = SSL_write(socket->ssl, buffer, (int) len);

  if (err <= 0) {
    err = SSL_get_error(socket->ssl, err);

    if (err == SSL_ERROR_WANT_READ || err == SSL_ERROR_WANT_WRITE) {
      retry = true;
    } else {
      js_throw_error(env, ERR_reason_symbol_name(ERR_peek_last_error()), "Write failed");
      return NULL;
    }
  }

  int res = retry ? 0 : err;

  js_value_t *result;
  err = js_create_int64(env, res, &result);
  assert(err == 0);

  return result;
}

static js_value_t *
bare_tls_shutdown(js_env_t *env, js_callback_info_t *info) {
  int err;

  size_t argc = 1;
  js_value_t *argv[1];

  err = js_get_callback_info(env, info, &argc, argv, NULL, NULL);
  assert(err == 0);

  assert(argc == 1);

  bare_tls_t *socket;
  err = js_get_arraybuffer_info(env, argv[0], (void **) &socket, NULL);
  assert(err == 0);

  err = SSL_shutdown(socket->ssl);

  if (err < 0) {
    js_throw_error(env, ERR_reason_symbol_name(ERR_peek_last_error()), "Shutdown failed");
    return NULL;
  }

  js_value_t *result;
  err = js_get_boolean(env, err == 1, &result);
  assert(err == 0);

  return result;
}

static js_value_t *
bare_tls_alpn_protocol(js_env_t *env, js_callback_info_t *info) {
  int err;

  size_t argc = 1;
  js_value_t *argv[1];

  err = js_get_callback_info(env, info, &argc, argv, NULL, NULL);
  assert(err == 0);

  assert(argc == 1);

  bare_tls_t *socket;
  err = js_get_arraybuffer_info(env, argv[0], (void **) &socket, NULL);
  assert(err == 0);

  const uint8_t *data = NULL;
  unsigned int len = 0;

  SSL_get0_alpn_selected(socket->ssl, &data, &len);

  js_value_t *result;

  if (data && len) {
    err = js_create_string_utf8(env, data, len, &result);
    assert(err == 0);
  } else {
    err = js_get_null(env, &result);
    assert(err == 0);
  }

  return result;
}

static js_value_t *
bare_tls_exports(js_env_t *env, js_value_t *exports) {
  int err;

  bare_tls__pin();

#define V(name, fn) \
  { \
    js_value_t *val; \
    err = js_create_function(env, name, -1, fn, NULL, &val); \
    assert(err == 0); \
    err = js_set_named_property(env, exports, name, val); \
    assert(err == 0); \
  }

  V("context", bare_tls_context);
  V("init", bare_tls_init);
  V("destroy", bare_tls_destroy);
  V("handshake", bare_tls_handshake);
  V("read", bare_tls_read);
  V("write", bare_tls_write);
  V("shutdown", bare_tls_shutdown);
  V("alpnProtocol", bare_tls_alpn_protocol);
#undef V

  return exports;
}

BARE_MODULE(bare_tls, bare_tls_exports)
