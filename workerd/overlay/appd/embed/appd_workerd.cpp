#include "appd_workerd.h"

#include <workerd/jsg/setup.h>
#include <workerd/server/server.h>
#include <workerd/server/v8-platform-impl.h>
#include <workerd/server/cpp-capnp-schema.embed.h>
#include <workerd/server/workerd-capnp-schema.embed.h>
#include <workerd/server/workerd.capnp.h>
#include <workerd/util/autogate.h>
#include <workerd/util/entropy.h>

#include <capnp/dynamic.h>
#include <capnp/schema-parser.h>
#include <kj/async-io.h>
#include <kj/filesystem.h>

#include <v8-extension.h>

#include <condition_variable>
#include <limits>
#include <mutex>

#ifdef _WIN32
#include <direct.h>
#include <winsock2.h>
#define appd_chdir _chdir
#else
#include <unistd.h>
#define appd_chdir chdir
#endif

using namespace workerd;
using workerd::server::Server;
using workerd::server::WorkerdPlatform;

namespace {

struct StartupState {
  std::mutex mutex;
  std::condition_variable readyCv;
  bool ready = false;
  int status = APPD_WORKERD_ERROR;
};

StartupState gStartup;

void beginStartup() {
  std::lock_guard<std::mutex> lock(gStartup.mutex);
  gStartup.ready = false;
  gStartup.status = APPD_WORKERD_ERROR;
}

void publishStartup(int status) {
  {
    std::lock_guard<std::mutex> lock(gStartup.mutex);
    gStartup.status = status;
    gStartup.ready = true;
  }
  gStartup.readyCv.notify_all();
}

#ifdef _WIN32
void closeListenerFd(uintptr_t listenerFd) {
  closesocket(static_cast<SOCKET>(listenerFd));
}
#else
void closeListenerFd(uintptr_t listenerFd) {
  if (listenerFd <= static_cast<uintptr_t>(std::numeric_limits<int>::max())) {
    close(static_cast<int>(listenerFd));
  }
}
#endif

class ListenerFdGuard final {
public:
  explicit ListenerFdGuard(uintptr_t listenerFd)
      : listenerFd_(listenerFd) {}

  ListenerFdGuard(const ListenerFdGuard&) = delete;
  ListenerFdGuard& operator=(const ListenerFdGuard&) = delete;

  ~ListenerFdGuard() {
    if (owns_) {
      closeListenerFd(listenerFd_);
    }
  }

  uintptr_t get() const {
    return listenerFd_;
  }

  void release() {
    owns_ = false;
  }

private:
  uintptr_t listenerFd_;
  bool owns_ = true;
};

const char kWasmStubSource[] =
    "if (typeof globalThis.WebAssembly === 'undefined') {"
    "  globalThis.WebAssembly = {"
    "    CompileError: class CompileError extends Error {"
    "      constructor(m) { super(m); this.name = 'CompileError'; }"
    "    },"
    "    LinkError: class LinkError extends Error {"
    "      constructor(m) { super(m); this.name = 'LinkError'; }"
    "    },"
    "    RuntimeError: class RuntimeError extends Error {"
    "      constructor(m) { super(m); this.name = 'RuntimeError'; }"
    "    },"
    "    compile() {"
    "      return Promise.reject(new WebAssembly.CompileError("
    "        'Wasm code generation disallowed by embedder'));"
    "    },"
    "    instantiate() {"
    "      return Promise.reject(new WebAssembly.CompileError("
    "        'Wasm code generation disallowed by embedder'));"
    "    },"
    "    validate() { return false; },"
    "    Module: class Module { constructor() {"
    "      throw new WebAssembly.CompileError("
    "        'Wasm code generation disallowed by embedder');"
    "    }},"
    "    Instance: class Instance { constructor() {"
    "      throw new WebAssembly.CompileError("
    "        'Wasm code generation disallowed by embedder');"
    "    }},"
    "    Memory: class Memory { constructor() {"
    "      throw new WebAssembly.CompileError("
    "        'Wasm code generation disallowed by embedder');"
    "    }},"
    "    Table: class Table { constructor() {"
    "      throw new WebAssembly.CompileError("
    "        'Wasm code generation disallowed by embedder');"
    "    }},"
    "  };"
    "}";

void registerWasmStubExtension() {
  static std::once_flag once;
  std::call_once(once, [] {
    auto ext = std::make_unique<v8::Extension>("appd/wasm-stub", kWasmStubSource);
    ext->set_auto_enable(true);
    v8::RegisterExtension(std::move(ext));
  });
}

class EmbedEntropySource final: public kj::EntropySource {
public:
  void generate(kj::ArrayPtr<kj::byte> buffer) override {
    workerd::getEntropy(buffer);
  }
};

class BuiltinSchema final: public capnp::SchemaFile {
public:
  BuiltinSchema(kj::StringPtr name, kj::StringPtr content)
      : name_(name),
        content_(content) {}

  kj::StringPtr getDisplayName() const override {
    return name_;
  }

  kj::Array<const char> readContent() const override {
    return kj::Array<const char>(
        content_.begin(), content_.size(), kj::NullArrayDisposer::instance);
  }

  kj::Maybe<kj::Own<SchemaFile>> import(kj::StringPtr target) const override;

  bool operator==(const SchemaFile& other) const override {
    if (auto d = dynamic_cast<const BuiltinSchema*>(&other)) {
      return d->name_ == name_;
    }
    return false;
  }

  size_t hashCode() const override {
    return kj::hashCode(name_);
  }

  void reportError(SourcePos start, SourcePos end, kj::StringPtr message) const override {
    KJ_LOG(ERROR, name_, start.line, start.column, end.line, end.column, message);
  }

private:
  kj::StringPtr name_;
  kj::StringPtr content_;
};

class DiskSchema final: public capnp::SchemaFile {
public:
  DiskSchema(
      const kj::Directory& root, kj::Path fullPath, kj::Own<const kj::ReadableFile> file)
      : root_(root),
        fullPath_(kj::mv(fullPath)),
        file_(kj::mv(file)),
        displayName_(fullPath_.toNativeString(true)) {}

  kj::StringPtr getDisplayName() const override {
    return displayName_;
  }

  kj::Array<const char> readContent() const override {
    auto size = file_->stat().size;
    if (!size) return nullptr;
    return file_->mmap(0, size).releaseAsChars();
  }

  kj::Maybe<kj::Own<SchemaFile>> import(kj::StringPtr target) const override;

  bool operator==(const SchemaFile& other) const override {
    if (auto d = dynamic_cast<const DiskSchema*>(&other)) {
      return d->fullPath_ == fullPath_;
    }
    return false;
  }

  size_t hashCode() const override {
    return kj::hashCode(fullPath_);
  }

  void reportError(SourcePos start, SourcePos end, kj::StringPtr message) const override {
    KJ_LOG(ERROR, displayName_, start.line, start.column, end.line, end.column, message);
  }

private:
  const kj::Directory& root_;
  kj::Path fullPath_;
  kj::Own<const kj::ReadableFile> file_;
  kj::String displayName_;
};

kj::Maybe<kj::Own<capnp::SchemaFile>> tryImportBuiltin(kj::StringPtr name) {
  if (name == "/capnp/c++.capnp") {
    return kj::heap<BuiltinSchema>("/capnp/c++.capnp", CPP_CAPNP_SCHEMA);
  }
  if (name == "/workerd/workerd.capnp") {
    return kj::heap<BuiltinSchema>("/workerd/workerd.capnp", WORKERD_CAPNP_SCHEMA);
  }
  return kj::none;
}

kj::Maybe<kj::Own<capnp::SchemaFile>> BuiltinSchema::import(kj::StringPtr target) const {
  return tryImportBuiltin(target);
}

kj::Maybe<kj::Own<capnp::SchemaFile>> DiskSchema::import(kj::StringPtr target) const {
  if (target.startsWith("/")) {
    return tryImportBuiltin(target);
  }

  auto relativeTo = fullPath_.parent();
  auto parsed = relativeTo.eval(target);
  KJ_IF_SOME(newFile, root_.tryOpenFile(parsed)) {
    return kj::implicitCast<kj::Own<capnp::SchemaFile>>(
        kj::heap<DiskSchema>(root_, kj::mv(parsed), kj::mv(newFile)));
  }
  return kj::none;
}

int failStartup(kj::StringPtr message) {
  KJ_LOG(ERROR, message);
  publishStartup(APPD_WORKERD_ERROR);
  return APPD_WORKERD_ERROR;
}

int doServe(const char* configPathStr, const char* workingDirStr, uintptr_t listenerFd) {
  ListenerFdGuard listenerGuard(listenerFd);

  try {
    if (configPathStr == nullptr) {
      return failStartup("config_path must not be null");
    }
    if (workingDirStr == nullptr) {
      return failStartup("working_dir must not be null");
    }
    if (appd_chdir(workingDirStr) != 0) {
      return failStartup("failed to chdir to workerd working directory");
    }

    auto fs = kj::newDiskFilesystem();
    auto fullConfigPath = fs->getCurrentPath().evalNative(configPathStr);
    auto configFile = KJ_UNWRAP_OR(fs->getRoot().tryOpenFile(fullConfigPath), {
      return failStartup("workerd config file not found");
    });

    capnp::SchemaParser schemaParser;
    schemaParser.loadCompiledTypeAndDependencies<server::config::Config>();
    auto parsed = schemaParser.parseFile(
        kj::heap<DiskSchema>(fs->getRoot(), kj::mv(fullConfigPath), kj::mv(configFile)));

    kj::Maybe<server::config::Config::Reader> maybeConfig;
    for (auto nested: parsed.getAllNested()) {
      if (!nested.getProto().isConst()) continue;

      auto constSchema = nested.asConst();
      auto type = constSchema.getType();
      if (type.isStruct() &&
          type.asStruct().getProto().getId() == capnp::typeId<server::config::Config>()) {
        maybeConfig = constSchema.as<server::config::Config>();
        break;
      }
    }

    auto config = KJ_UNWRAP_OR(maybeConfig, {
      return failStartup("no workerd Config constant found in capnp file");
    });

    util::Autogate::initAutogate(config.getAutogates());

    auto io = kj::setupAsyncIo();
    auto& network = io.provider->getNetwork();
    EmbedEntropySource entropySource;

    auto server = kj::heap<Server>(
        *fs,
        io.provider->getTimer(),
        kj::systemPreciseMonotonicClock(),
        network,
        entropySource,
        Worker::LoggingOptions(Worker::ConsoleMode::STDOUT),
        [](kj::String error) { KJ_LOG(ERROR, "workerd config error", error); });

    auto listener = io.lowLevelProvider->wrapListenSocketFd(
        static_cast<kj::LowLevelAsyncIoProvider::Fd>(listenerGuard.get()),
        kj::LowLevelAsyncIoProvider::TAKE_OWNERSHIP);
    listenerGuard.release();
    server->overrideSocket(kj::str("http"), kj::mv(listener));

    registerWasmStubExtension();
    auto platform = jsg::defaultPlatform(0);
    WorkerdPlatform v8Platform(*platform);
    jsg::V8System v8System(v8Platform,
        KJ_MAP(flag, config.getV8Flags()) -> kj::StringPtr { return flag; },
        platform.get());

    publishStartup(APPD_WORKERD_OK);

    server->run(v8System, config).wait(io.waitScope);
    server = nullptr;
    return APPD_WORKERD_OK;
  } catch (kj::Exception& e) {
    KJ_LOG(ERROR, "workerd exception", e.getDescription());
    publishStartup(APPD_WORKERD_ERROR);
    return APPD_WORKERD_ERROR;
  }
}

}  // namespace

extern "C" {

int appd_workerd_serve(const char* config_path, const char* working_dir, uintptr_t listener_fd) {
  beginStartup();
  return doServe(config_path, working_dir, listener_fd);
}

int appd_workerd_wait_ready(void) {
  std::unique_lock<std::mutex> lock(gStartup.mutex);
  gStartup.readyCv.wait(lock, [] { return gStartup.ready; });
  return gStartup.status;
}

}  // extern "C"
