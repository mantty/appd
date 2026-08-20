#![deny(missing_docs)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::elidable_lifetime_names)]
#![allow(clippy::needless_pass_by_value)]

//! Native `node:fs` bindings for the appd `QuickJS` runtime.

use std::sync::{Arc, Mutex, MutexGuard};

use appd_vfs::{
    CopyOptions, DirectoryEntry, Error as VfsError, OpenOptions, Stat, VirtualFileSystem,
};
use rquickjs::function::{IntoJsFunc, Opt, Rest, This};
use rquickjs::module::{Declarations, Exports, ModuleDef};
use rquickjs::{
    Array, ArrayBuffer, BigInt, Constructor, Ctx, Exception, Function, Object, Promise, Symbol,
    TypedArray, Value,
};
use rquickjs::{Coerced, FromJs, IntoJs};

/// The native `node:fs` module name.
pub const MODULE_NAME: &str = "node:fs";
/// The native `node:fs/promises` module name.
pub const PROMISES_MODULE_NAME: &str = "node:fs/promises";

/// A request-owned VFS handle installed into a `QuickJS` context.
pub type VfsHandle = Arc<Mutex<VirtualFileSystem>>;

struct VfsUserData(VfsHandle);

// This userdata contains no JavaScript values, so changing the marker lifetime
// cannot change its representation or validity.
#[allow(clippy::elidable_lifetime_names)]
unsafe impl<'js> rquickjs::JsLifetime<'js> for VfsUserData {
    type Changed<'to> = VfsUserData;
}

/// Install the request VFS and the native Node filesystem builtins.
pub fn install(ctx: &Ctx<'_>, vfs: &VfsHandle) -> rquickjs::Result<()> {
    ctx.store_userdata(VfsUserData(Arc::clone(vfs)))?;
    let (fs_module, fs_evaluation) =
        rquickjs::Module::evaluate_def::<NodeFsModule, _>(ctx.clone(), MODULE_NAME)?;
    fs_evaluation.finish::<()>()?;
    let (promises_module, promises_evaluation) = rquickjs::Module::evaluate_def::<
        NodeFsPromisesModule,
        _,
    >(ctx.clone(), PROMISES_MODULE_NAME)?;
    promises_evaluation.finish::<()>()?;

    let process = ctx
        .globals()
        .get::<_, Option<Object>>("process")?
        .map_or_else(|| Object::new(ctx.clone()), Ok)?;
    process.set("__appd_node_fs", fs_module.namespace()?)?;
    process.set("__appd_node_fs_promises", promises_module.namespace()?)?;
    let get_builtin_module: Function = ctx.eval(
        "name => name === 'node:fs' ? globalThis.process.__appd_node_fs : name === 'node:fs/promises' ? globalThis.process.__appd_node_fs_promises : undefined",
    )?;
    process.set("getBuiltinModule", get_builtin_module)?;
    ctx.globals().set("process", process)
}

#[allow(clippy::wildcard_imports)]
#[rquickjs::module]
pub(crate) mod node_fs_module {
    use super::*;

    #[qjs(declare)]
    pub fn declare(declare: &Declarations) -> rquickjs::Result<()> {
        declare_exports(declare, false)
    }

    #[qjs(evaluate)]
    pub fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> rquickjs::Result<()> {
        export_module(ctx, exports, false)
    }
}

#[allow(clippy::wildcard_imports)]
#[rquickjs::module]
pub(crate) mod node_fs_promises_module {
    use super::*;

    #[qjs(declare)]
    pub fn declare(declare: &Declarations) -> rquickjs::Result<()> {
        declare_exports(declare, true)
    }

    #[qjs(evaluate)]
    pub fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> rquickjs::Result<()> {
        export_module(ctx, exports, true)
    }
}

/// The native `node:fs` module definition.
pub struct NodeFsModule;

impl ModuleDef for NodeFsModule {
    fn declare(declare: &Declarations) -> rquickjs::Result<()> {
        <js_node_fs_module as ModuleDef>::declare(declare)
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> rquickjs::Result<()> {
        <js_node_fs_module as ModuleDef>::evaluate(ctx, exports)
    }
}

/// The native `node:fs/promises` module definition.
pub struct NodeFsPromisesModule;

impl ModuleDef for NodeFsPromisesModule {
    fn declare(declare: &Declarations) -> rquickjs::Result<()> {
        <js_node_fs_promises_module as ModuleDef>::declare(declare)
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> rquickjs::Result<()> {
        <js_node_fs_promises_module as ModuleDef>::evaluate(ctx, exports)
    }
}

#[allow(clippy::too_many_lines)]
fn declare_exports(declare: &Declarations, promises_only: bool) -> rquickjs::Result<()> {
    let names = if promises_only {
        [
            "constants",
            "F_OK",
            "R_OK",
            "W_OK",
            "X_OK",
            "readFile",
            "writeFile",
            "appendFile",
            "access",
            "chmod",
            "chown",
            "cp",
            "fchmod",
            "fchown",
            "fdatasync",
            "fsync",
            "fstat",
            "ftruncate",
            "futimes",
            "glob",
            "lchmod",
            "lchown",
            "link",
            "lutimes",
            "mkdir",
            "mkdtemp",
            "open",
            "opendir",
            "readdir",
            "lstat",
            "readlink",
            "realpath",
            "rename",
            "unlink",
            "rm",
            "rmdir",
            "copyFile",
            "symlink",
            "stat",
            "statfs",
            "truncate",
            "utimes",
            "FileHandle",
            "default",
        ]
        .as_slice()
    } else {
        [
            "constants",
            "F_OK",
            "R_OK",
            "W_OK",
            "X_OK",
            "readFileSync",
            "writeFileSync",
            "appendFileSync",
            "accessSync",
            "chmodSync",
            "chownSync",
            "mkdirSync",
            "readdirSync",
            "statSync",
            "lstatSync",
            "existsSync",
            "unlinkSync",
            "rmSync",
            "rmdirSync",
            "renameSync",
            "copyFileSync",
            "cpSync",
            "fchmodSync",
            "fchownSync",
            "fdatasyncSync",
            "fsyncSync",
            "futimesSync",
            "globSync",
            "lchmodSync",
            "lchownSync",
            "lutimesSync",
            "linkSync",
            "mkdtempSync",
            "opendirSync",
            "symlinkSync",
            "readlinkSync",
            "realpathSync",
            "openSync",
            "closeSync",
            "fstatSync",
            "readSync",
            "readvSync",
            "writeSync",
            "writevSync",
            "truncateSync",
            "ftruncateSync",
            "statfsSync",
            "utimesSync",
            "openAsBlob",
            "Dirent",
            "Dir",
            "Stats",
            "ReadStream",
            "WriteStream",
            "FileReadStream",
            "FileWriteStream",
            "createReadStream",
            "createWriteStream",
            "access",
            "exists",
            "appendFile",
            "chmod",
            "chown",
            "close",
            "copyFile",
            "cp",
            "fchmod",
            "fchown",
            "fdatasync",
            "fstat",
            "fsync",
            "ftruncate",
            "futimes",
            "glob",
            "lchmod",
            "lchown",
            "link",
            "lstat",
            "lutimes",
            "mkdir",
            "mkdtemp",
            "open",
            "opendir",
            "read",
            "readFile",
            "readlink",
            "readv",
            "readdir",
            "realpath",
            "rename",
            "rm",
            "rmdir",
            "stat",
            "statfs",
            "symlink",
            "truncate",
            "unlink",
            "utimes",
            "write",
            "writeFile",
            "writev",
            "promises",
            "default",
        ]
        .as_slice()
    };
    for name in names {
        declare.declare(*name)?;
    }
    Ok(())
}

fn export_module<'js>(
    ctx: &Ctx<'js>,
    exports: &Exports<'js>,
    promises_only: bool,
) -> rquickjs::Result<()> {
    let object = Object::new(ctx.clone())?;
    export_constants(ctx, exports, &object)?;
    if promises_only {
        let file_handle = file_handle_constructor(ctx)?;
        exports.export("FileHandle", file_handle.clone())?;
        object.set("FileHandle", file_handle)?;
        export_promises(ctx, exports, &object)?;
    } else {
        let dirent = dirent_constructor(ctx)?;
        let dir = dir_constructor(ctx)?;
        let stats = stats_constructor(ctx)?;
        for (name, value) in [
            ("Dirent", dirent.clone()),
            ("Dir", dir.clone()),
            ("Stats", stats.clone()),
        ] {
            exports.export(name, value.clone())?;
            object.set(name, value)?;
        }

        export_sync(ctx, exports, &object)?;
        let promises = promise_object(ctx.clone())?;
        exports.export("promises", promises.clone())?;
        object.set("promises", promises)?;
        export_streams(ctx, exports, &object)?;
        export_callback(ctx, exports, &object)?;
    }
    exports.export("default", object).map(|_| ())
}

fn export_constants<'js>(
    ctx: &Ctx<'js>,
    exports: &Exports<'js>,
    object: &Object<'js>,
) -> rquickjs::Result<()> {
    let constants = constants(ctx.clone())?;
    for (name, value) in [
        ("F_OK", 0_i32),
        ("R_OK", 4_i32),
        ("W_OK", 2_i32),
        ("X_OK", 1_i32),
    ] {
        exports.export(name, value)?;
        object.set(name, value)?;
    }
    exports.export("constants", constants.clone())?;
    object.set("constants", constants)
}

fn export_sync<'js>(
    ctx: &Ctx<'js>,
    exports: &Exports<'js>,
    object: &Object<'js>,
) -> rquickjs::Result<()> {
    export_function(ctx, exports, object, "readFileSync", read_file_sync)?;
    export_function(ctx, exports, object, "writeFileSync", write_file_sync)?;
    export_function(ctx, exports, object, "appendFileSync", append_file_sync)?;
    export_function(ctx, exports, object, "accessSync", access_sync)?;
    export_function(ctx, exports, object, "chmodSync", chmod_sync)?;
    export_function(ctx, exports, object, "chownSync", chown_sync)?;
    export_function(ctx, exports, object, "mkdirSync", mkdir_sync)?;
    export_function(ctx, exports, object, "readdirSync", readdir_sync)?;
    export_function(ctx, exports, object, "statSync", stat_sync)?;
    export_function(ctx, exports, object, "lstatSync", lstat_sync)?;
    export_function(ctx, exports, object, "existsSync", exists_sync)?;
    export_function(ctx, exports, object, "unlinkSync", unlink_sync)?;
    export_function(ctx, exports, object, "rmSync", rm_sync)?;
    export_function(ctx, exports, object, "rmdirSync", rmdir_sync)?;
    export_function(ctx, exports, object, "renameSync", rename_sync)?;
    export_function(ctx, exports, object, "copyFileSync", copy_file_sync)?;
    export_function(ctx, exports, object, "cpSync", cp_sync)?;
    export_function(ctx, exports, object, "fchmodSync", fchmod_sync)?;
    export_function(ctx, exports, object, "fchownSync", fchown_sync)?;
    export_function(ctx, exports, object, "fdatasyncSync", fdatasync_sync)?;
    export_function(ctx, exports, object, "fsyncSync", fsync_sync)?;
    export_function(ctx, exports, object, "futimesSync", futimes_sync)?;
    export_function(ctx, exports, object, "globSync", glob_sync)?;
    export_function(ctx, exports, object, "lchmodSync", lchmod_sync)?;
    export_function(ctx, exports, object, "lchownSync", lchown_sync)?;
    export_function(ctx, exports, object, "lutimesSync", lutimes_sync)?;
    export_function(ctx, exports, object, "linkSync", link_sync)?;
    export_function(ctx, exports, object, "mkdtempSync", mkdtemp_sync)?;
    export_function(ctx, exports, object, "opendirSync", opendir_sync)?;
    export_function(ctx, exports, object, "symlinkSync", symlink_sync)?;
    export_function(ctx, exports, object, "readlinkSync", read_link_sync)?;
    export_function(ctx, exports, object, "realpathSync", realpath_sync)?;
    export_function(ctx, exports, object, "openSync", open_sync)?;
    export_function(ctx, exports, object, "closeSync", close_sync)?;
    export_function(ctx, exports, object, "fstatSync", fstat_sync)?;
    export_function(ctx, exports, object, "readSync", read_sync_export)?;
    export_function(ctx, exports, object, "readvSync", readv_sync)?;
    export_function(ctx, exports, object, "writeSync", write_sync_export)?;
    export_function(ctx, exports, object, "writevSync", writev_sync)?;
    export_function(ctx, exports, object, "truncateSync", truncate_sync)?;
    export_function(ctx, exports, object, "ftruncateSync", ftruncate_sync)?;
    export_function(ctx, exports, object, "statfsSync", statfs_sync)?;
    export_function(ctx, exports, object, "utimesSync", utimes_sync)?;
    export_function(ctx, exports, object, "openAsBlob", open_as_blob)
}

fn export_callback<'js>(
    ctx: &Ctx<'js>,
    exports: &Exports<'js>,
    object: &Object<'js>,
) -> rquickjs::Result<()> {
    for (name, operation) in callback_operations() {
        let name = *name;
        let operation = *operation;
        let function = Function::new(ctx.clone(), move |ctx: Ctx<'js>, args: Rest<Value<'js>>| {
            callback_call(ctx, operation, args)
        })?;
        if name == "realpath" {
            function.set("native", function.clone())?;
        }
        exports.export(name, function.clone())?;
        object.set(name, function)?;
    }
    Ok(())
}

fn export_streams<'js>(
    ctx: &Ctx<'js>,
    exports: &Exports<'js>,
    object: &Object<'js>,
) -> rquickjs::Result<()> {
    let read_stream = stream_type_constructor(ctx, "Readable")?;
    let write_stream = stream_type_constructor(ctx, "Writable")?;
    for (name, value) in [
        ("ReadStream", read_stream.clone()),
        ("FileReadStream", read_stream),
        ("WriteStream", write_stream.clone()),
        ("FileWriteStream", write_stream),
    ] {
        exports.export(name, value.clone())?;
        object.set(name, value)?;
    }
    export_function(
        ctx,
        exports,
        object,
        "createReadStream",
        create_read_stream_export,
    )?;
    export_function(
        ctx,
        exports,
        object,
        "createWriteStream",
        create_write_stream_export,
    )
}

fn stream_type_constructor<'js>(ctx: &Ctx<'js>, name: &str) -> rquickjs::Result<Constructor<'js>> {
    let readable = name == "Readable";
    let prototype_name = if readable {
        "__appd_node_fs_read_stream_proto"
    } else {
        "__appd_node_fs_write_stream_proto"
    };
    let prototype = Object::new(ctx.clone())?;
    ctx.globals().set(prototype_name, prototype.clone())?;
    let prototype_name = prototype_name.to_owned();
    Constructor::new_prototype(
        ctx,
        prototype,
        move |ctx: Ctx<'js>, input: Opt<Value<'js>>, options: Opt<Value<'js>>| {
            let stream = if readable {
                create_read_stream(&ctx, input.0, None, options.0)
            } else {
                create_write_stream(&ctx, input.0, None, options.0)
            }?;
            if let Some(base_prototype) = stream.get_prototype() {
                let prototype: Object = ctx.globals().get(&prototype_name)?;
                prototype.set_prototype(Some(&base_prototype))?;
            }
            Ok::<Object<'js>, rquickjs::Error>(stream)
        },
    )
}

fn export_promises<'js>(
    ctx: &Ctx<'js>,
    exports: &Exports<'js>,
    object: &Object<'js>,
) -> rquickjs::Result<()> {
    export_promise_functions(ctx, |name, function| {
        exports.export(name, function.clone())?;
        object.set(name, function)
    })
}

fn promise_object(ctx: Ctx<'_>) -> rquickjs::Result<Object<'_>> {
    let object = Object::new(ctx.clone())?;
    export_promise_functions(&ctx, |name, function| object.set(name, function))?;
    Ok(object)
}

fn export_promise_functions<'js>(
    ctx: &Ctx<'js>,
    mut export: impl FnMut(&str, Function<'js>) -> rquickjs::Result<()>,
) -> rquickjs::Result<()> {
    for (name, function) in [
        ("readFile", Function::new(ctx.clone(), read_file_promise)?),
        ("writeFile", Function::new(ctx.clone(), write_file_promise)?),
        (
            "appendFile",
            Function::new(ctx.clone(), append_file_promise)?,
        ),
        ("access", Function::new(ctx.clone(), access_promise)?),
        ("chmod", Function::new(ctx.clone(), chmod_promise)?),
        ("chown", Function::new(ctx.clone(), chown_promise)?),
        ("cp", Function::new(ctx.clone(), cp_promise)?),
        ("fchmod", Function::new(ctx.clone(), fchmod_promise)?),
        ("fchown", Function::new(ctx.clone(), fchown_promise)?),
        ("fdatasync", Function::new(ctx.clone(), fdatasync_promise)?),
        ("fsync", Function::new(ctx.clone(), fsync_promise)?),
        ("fstat", Function::new(ctx.clone(), fstat_promise)?),
        ("ftruncate", Function::new(ctx.clone(), ftruncate_promise)?),
        ("futimes", Function::new(ctx.clone(), futimes_promise)?),
        ("glob", Function::new(ctx.clone(), glob_promise)?),
        ("lchmod", Function::new(ctx.clone(), lchmod_promise)?),
        ("lchown", Function::new(ctx.clone(), lchown_promise)?),
        ("link", Function::new(ctx.clone(), link_promise)?),
        ("lutimes", Function::new(ctx.clone(), lutimes_promise)?),
        ("mkdir", Function::new(ctx.clone(), mkdir_promise)?),
        ("mkdtemp", Function::new(ctx.clone(), mkdtemp_promise)?),
        ("open", Function::new(ctx.clone(), open_promise)?),
        ("opendir", Function::new(ctx.clone(), opendir_promise)?),
        ("readdir", Function::new(ctx.clone(), readdir_promise)?),
        ("stat", Function::new(ctx.clone(), stat_promise)?),
        ("lstat", Function::new(ctx.clone(), lstat_promise)?),
        ("unlink", Function::new(ctx.clone(), unlink_promise)?),
        ("rm", Function::new(ctx.clone(), rm_promise)?),
        ("rmdir", Function::new(ctx.clone(), rmdir_promise)?),
        ("rename", Function::new(ctx.clone(), rename_promise)?),
        ("copyFile", Function::new(ctx.clone(), copy_file_promise)?),
        ("symlink", Function::new(ctx.clone(), symlink_promise)?),
        ("readlink", Function::new(ctx.clone(), read_link_promise)?),
        ("realpath", Function::new(ctx.clone(), realpath_promise)?),
        ("truncate", Function::new(ctx.clone(), truncate_promise)?),
        ("statfs", Function::new(ctx.clone(), statfs_promise)?),
        ("utimes", Function::new(ctx.clone(), utimes_promise)?),
    ] {
        export(name, function)?;
    }
    Ok(())
}

fn export_function<'js, F, P>(
    ctx: &Ctx<'js>,
    exports: &Exports<'js>,
    object: &Object<'js>,
    name: &str,
    function: F,
) -> rquickjs::Result<()>
where
    F: IntoJsFunc<'js, P> + Copy + 'js,
{
    let function = Function::new(ctx.clone(), function)?;
    exports.export(name, function.clone())?;
    object.set(name, function)
}

fn constants(ctx: Ctx<'_>) -> rquickjs::Result<Object<'_>> {
    let object = Object::new(ctx)?;
    for (name, value) in [
        ("UV_FS_SYMLINK_DIR", 1_i32),
        ("UV_FS_SYMLINK_JUNCTION", 2),
        ("O_RDONLY", 0),
        ("O_WRONLY", 1),
        ("O_RDWR", 2),
        ("UV_DIRENT_UNKNOWN", 0),
        ("UV_DIRENT_FILE", 1),
        ("UV_DIRENT_DIR", 2),
        ("UV_DIRENT_LINK", 3),
        ("UV_DIRENT_FIFO", 4),
        ("UV_DIRENT_SOCKET", 5),
        ("UV_DIRENT_CHAR", 6),
        ("UV_DIRENT_BLOCK", 7),
        ("EXTENSIONLESS_FORMAT_JAVASCRIPT", 0),
        ("EXTENSIONLESS_FORMAT_WASM", 1),
        ("S_IFMT", 61_440),
        ("S_IFREG", 32_768),
        ("S_IFDIR", 16_384),
        ("S_IFCHR", 8_192),
        ("S_IFBLK", 24_576),
        ("S_IFIFO", 4_096),
        ("S_IFLNK", 40_960),
        ("S_IFSOCK", 49_152),
        ("O_CREAT", 64),
        ("O_EXCL", 128),
        ("UV_FS_O_FILEMAP", 0),
        ("O_NOCTTY", 256),
        ("O_TRUNC", 512),
        ("O_APPEND", 1_024),
        ("O_DIRECTORY", 65_536),
        ("O_NOATIME", 262_144),
        ("O_NOFOLLOW", 131_072),
        ("O_SYNC", 1_052_672),
        ("O_DSYNC", 4_096),
        ("O_DIRECT", 16_384),
        ("O_NONBLOCK", 2_048),
        ("S_IRWXU", 448),
        ("S_IRUSR", 256),
        ("S_IWUSR", 128),
        ("S_IXUSR", 64),
        ("S_IRWXG", 56),
        ("S_IRGRP", 32),
        ("S_IWGRP", 16),
        ("S_IXGRP", 8),
        ("S_IRWXO", 7),
        ("S_IROTH", 4),
        ("S_IWOTH", 2),
        ("S_IXOTH", 1),
        ("F_OK", 0),
        ("R_OK", 4),
        ("W_OK", 2),
        ("X_OK", 1),
        ("UV_FS_COPYFILE_EXCL", 1),
        ("COPYFILE_EXCL", 1),
        ("UV_FS_COPYFILE_FICLONE", 2),
        ("COPYFILE_FICLONE", 2),
        ("UV_FS_COPYFILE_FICLONE_FORCE", 4),
        ("COPYFILE_FICLONE_FORCE", 4),
    ] {
        object.set(name, value)?;
    }
    Ok(object)
}

#[allow(clippy::struct_excessive_bools)]
struct FsOptions {
    encoding: Option<String>,
    recursive: bool,
    force: bool,
    error_on_exist: bool,
    dereference: bool,
    with_file_types: bool,
    bigint: bool,
    throw_if_no_entry: bool,
    cwd: Option<String>,
    blob_type: Option<String>,
}

impl Default for FsOptions {
    fn default() -> Self {
        Self {
            encoding: None,
            recursive: false,
            force: false,
            error_on_exist: false,
            dereference: false,
            with_file_types: false,
            bigint: false,
            throw_if_no_entry: true,
            cwd: None,
            blob_type: None,
        }
    }
}

fn parse_options<'js>(ctx: &Ctx<'js>, value: Option<Value<'js>>) -> rquickjs::Result<FsOptions> {
    let Some(value) = value else {
        return Ok(FsOptions::default());
    };
    if value.is_undefined() || value.is_null() {
        return Ok(FsOptions::default());
    }
    if value.is_string() {
        return Ok(FsOptions {
            encoding: Some(
                value
                    .into_string()
                    .ok_or_else(|| Exception::throw_type(ctx, "expected a string"))?
                    .to_string()?,
            ),
            ..FsOptions::default()
        });
    }
    if value.is_bool() {
        return Ok(FsOptions::default());
    }
    let object = value
        .try_into_object()
        .map_err(|_| Exception::throw_type(ctx, "filesystem options must be a string or object"))?;
    Ok(FsOptions {
        encoding: object.get("encoding")?,
        recursive: object.get::<_, Option<bool>>("recursive")?.unwrap_or(false),
        force: object.get::<_, Option<bool>>("force")?.unwrap_or(false),
        error_on_exist: object
            .get::<_, Option<bool>>("errorOnExist")?
            .unwrap_or(false),
        dereference: object
            .get::<_, Option<bool>>("dereference")?
            .unwrap_or(false),
        with_file_types: object
            .get::<_, Option<bool>>("withFileTypes")?
            .unwrap_or(false),
        bigint: object.get::<_, Option<bool>>("bigint")?.unwrap_or(false),
        throw_if_no_entry: object
            .get::<_, Option<bool>>("throwIfNoEntry")?
            .unwrap_or(true),
        cwd: object.get("cwd")?,
        blob_type: object.get("type")?,
    })
}

fn option_flag<'js>(
    ctx: &Ctx<'js>,
    value: Option<Value<'js>>,
) -> rquickjs::Result<Option<Value<'js>>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !value.is_object() {
        return Ok(None);
    }
    let object = value
        .try_into_object()
        .map_err(|_| Exception::throw_type(ctx, "filesystem options must be a string or object"))?;
    object.get("flag")
}

fn option_property<'js>(
    ctx: &Ctx<'js>,
    value: Option<&Value<'js>>,
    name: &str,
) -> rquickjs::Result<Option<Value<'js>>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !value.is_object() {
        return Ok(None);
    }
    let object = value
        .clone()
        .try_into_object()
        .map_err(|_| Exception::throw_type(ctx, "filesystem options must be an object"))?;
    object.get(name)
}

fn path<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<String> {
    let value: Coerced<std::string::String> = Coerced::from_js(ctx, value)?;
    let mut path = value.as_ref().clone();
    if !path.starts_with('/') {
        path = format!("/bundle/{path}");
    }
    if path.chars().count() > appd_vfs::MAX_PATH_LENGTH {
        return Err(Exception::throw_range(ctx, "path is too long"));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(Exception::throw_range(
                        ctx,
                        "path escapes the virtual filesystem",
                    ));
                }
            }
            part => {
                parts.push(part);
                if parts.len() > appd_vfs::MAX_PATH_SEGMENTS {
                    return Err(Exception::throw_range(ctx, "path has too many segments"));
                }
            }
        }
    }
    let path = format!("/{}", parts.join("/"));
    if !["/", "/bundle", "/tmp", "/dev"]
        .iter()
        .any(|root| path == *root || path.starts_with(&format!("{root}/")))
    {
        return Err(Exception::throw_message(
            ctx,
            &format!("ENOENT: no such file or directory, '{path}'"),
        ));
    }
    Ok(path)
}

enum PathOrFd {
    Path(String),
    Fd(u32),
}

fn path_or_fd<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<PathOrFd> {
    if value.is_number() {
        return descriptor_value(ctx, value).map(PathOrFd::Fd);
    }
    path(ctx, value).map(PathOrFd::Path)
}

fn descriptor_value<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<u32> {
    let descriptor: Coerced<i64> = Coerced::from_js(ctx, value)?;
    u32::try_from(*descriptor)
        .map_err(|_| Exception::throw_range(ctx, "file descriptor must be non-negative"))
}

fn bytes<'js>(
    ctx: &Ctx<'js>,
    value: Value<'js>,
    encoding: Option<&str>,
) -> rquickjs::Result<Vec<u8>> {
    if value.is_string() {
        let text = value
            .into_string()
            .ok_or_else(|| Exception::throw_type(ctx, "expected a string"))?
            .to_string()?;
        return match encoding.unwrap_or("utf8").to_ascii_lowercase().as_str() {
            "base64" => Ok(decode_base64(&text)),
            "hex" => decode_hex(ctx, &text),
            "buffer" | "utf8" | "utf-8" => Ok(text.into_bytes()),
            _ => Err(Exception::throw_type(ctx, "unsupported encoding")),
        };
    }
    if let Some(buffer) = ArrayBuffer::from_value(value.clone()) {
        return buffer
            .as_bytes()
            .map(ToOwned::to_owned)
            .ok_or_else(|| Exception::throw_type(ctx, "array buffer is detached"));
    }
    let object = value.clone().try_into_object().map_err(|_| {
        Exception::throw_type(ctx, "data must be a string, ArrayBuffer, or typed array")
    })?;
    typed_array_bytes(&object).ok_or_else(|| {
        Exception::throw_type(ctx, "data must be a string, ArrayBuffer, or typed array")
    })
}

fn write_buffer_value<'js>(
    ctx: &Ctx<'js>,
    value: &Value<'js>,
    bytes: &[u8],
) -> rquickjs::Result<Value<'js>> {
    if value.is_string() {
        Ok(TypedArray::<u8>::new_copy(ctx.clone(), bytes)?.into_value())
    } else {
        Ok(value.clone())
    }
}

fn typed_array_bytes(object: &Object<'_>) -> Option<Vec<u8>> {
    macro_rules! typed_array {
        ($type:ty) => {
            if object.is_typed_array::<$type>() {
                return TypedArray::<$type>::from_object(object.clone())
                    .ok()?
                    .as_bytes()
                    .map(ToOwned::to_owned);
            }
        };
    }
    typed_array!(u8);
    typed_array!(i8);
    typed_array!(u16);
    typed_array!(i16);
    typed_array!(u32);
    typed_array!(i32);
    typed_array!(u64);
    typed_array!(i64);
    typed_array!(f32);
    typed_array!(f64);
    None
}

fn is_buffer_value(value: &Value<'_>) -> bool {
    if ArrayBuffer::from_value(value.clone()).is_some() {
        return true;
    }
    value
        .clone()
        .try_into_object()
        .ok()
        .is_some_and(|object| typed_array_bytes(&object).is_some())
}

fn output<'js>(
    ctx: Ctx<'js>,
    bytes: &[u8],
    encoding: Option<&str>,
) -> rquickjs::Result<Value<'js>> {
    match encoding.map(str::to_ascii_lowercase).as_deref() {
        None | Some("buffer") => Ok(TypedArray::<u8>::new_copy(ctx, bytes)?.into_value()),
        Some("base64") => Ok(base64_encode(bytes).into_js(&ctx)?),
        Some("hex") => {
            use std::fmt::Write as _;
            let mut value = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                let _ = write!(&mut value, "{byte:02x}");
            }
            Ok(value.into_js(&ctx)?)
        }
        Some("utf8" | "utf-8") => Ok(String::from_utf8_lossy(bytes).into_owned().into_js(&ctx)?),
        Some(_) => Err(Exception::throw_type(&ctx, "unsupported encoding")),
    }
}

fn read_descriptor(ctx: &Ctx<'_>, descriptor: u32) -> rquickjs::Result<Vec<u8>> {
    let size = vfs_call(ctx, |vfs| vfs.fstat(descriptor))?.size;
    let size = usize::try_from(size)
        .map_err(|_| Exception::throw_range(ctx, "file is too large to read"))?;
    vfs_call(ctx, |vfs| vfs.read(descriptor, size, Some(0)))
}

fn write_descriptor(
    ctx: &Ctx<'_>,
    descriptor: u32,
    data: &[u8],
    append: bool,
) -> rquickjs::Result<usize> {
    if append {
        let position = vfs_call(ctx, |vfs| vfs.fstat(descriptor).map(|stat| stat.size))?;
        vfs_call(ctx, |vfs| vfs.write(descriptor, data, Some(position)))
    } else {
        vfs_call(ctx, |vfs| vfs.ftruncate(descriptor, 0))?;
        vfs_call(ctx, |vfs| vfs.write(descriptor, data, Some(0)))
    }
}

fn read_file_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Value<'js>> {
    let options = parse_options(&ctx, options.0)?;
    let bytes = match path_or_fd(&ctx, input)? {
        PathOrFd::Path(path) => vfs_call(&ctx, |vfs| vfs.read_file(&path))?,
        PathOrFd::Fd(descriptor) => read_descriptor(&ctx, descriptor)?,
    };
    output(ctx, &bytes, options.encoding.as_deref())
}

fn write_file_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    data: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<()> {
    let options_value = options.0;
    let options = parse_options(&ctx, options_value.clone())?;
    let flag = option_flag(&ctx, options_value)?;
    let append = flag_string(flag.as_ref())?.is_some_and(|flag| flag.contains('a'));
    let bytes = bytes(&ctx, data, options.encoding.as_deref())?;
    match path_or_fd(&ctx, input)? {
        PathOrFd::Path(path) => {
            let open_options = if let Some(flag) = flag {
                open_options(&ctx, Some(flag))?
            } else {
                OpenOptions {
                    read: false,
                    write: true,
                    create: true,
                    truncate: true,
                    ..OpenOptions::default()
                }
            };
            vfs_call(&ctx, |vfs| {
                let descriptor = vfs.open(&path, open_options)?;
                let result = vfs.write(descriptor, &bytes, None).map(|_| ());
                result.and(vfs.close(descriptor))
            })
        }
        PathOrFd::Fd(descriptor) => {
            write_descriptor(&ctx, descriptor, &bytes, append)?;
            Ok(())
        }
    }
}

fn append_file_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    data: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<()> {
    let options = parse_options(&ctx, options.0)?;
    let bytes = bytes(&ctx, data, options.encoding.as_deref())?;
    match path_or_fd(&ctx, input)? {
        PathOrFd::Path(path) => vfs_call(&ctx, |vfs| vfs.write_file(&path, &bytes, true)),
        PathOrFd::Fd(descriptor) => {
            write_descriptor(&ctx, descriptor, &bytes, true)?;
            Ok(())
        }
    }
}

fn access_sync<'js>(ctx: Ctx<'js>, input: Value<'js>, mode: Opt<u32>) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    vfs_call(&ctx, |vfs| vfs.access(&path, mode.0.unwrap_or(0)))
}

fn chmod_sync<'js>(ctx: Ctx<'js>, input: Value<'js>, mode: Value<'js>) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    validate_mode(&ctx, mode)?;
    vfs_call(&ctx, |vfs| vfs.stat(&path).map(|_| ()))
}

fn chown_sync<'js>(ctx: Ctx<'js>, input: Value<'js>, _uid: u32, _gid: u32) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    vfs_call(&ctx, |vfs| vfs.stat(&path).map(|_| ()))
}

fn fchmod_sync<'js>(ctx: Ctx<'js>, descriptor: u32, mode: Value<'js>) -> rquickjs::Result<()> {
    validate_mode(&ctx, mode)?;
    vfs_call(&ctx, |vfs| vfs.fstat(descriptor).map(|_| ()))
}

fn fchown_sync(ctx: Ctx<'_>, descriptor: u32, _uid: u32, _gid: u32) -> rquickjs::Result<()> {
    vfs_call(&ctx, |vfs| vfs.fstat(descriptor).map(|_| ()))
}

fn fdatasync_sync(ctx: Ctx<'_>, descriptor: u32) -> rquickjs::Result<()> {
    vfs_call(&ctx, |vfs| vfs.fstat(descriptor).map(|_| ()))
}

fn fsync_sync(ctx: Ctx<'_>, descriptor: u32) -> rquickjs::Result<()> {
    vfs_call(&ctx, |vfs| vfs.fstat(descriptor).map(|_| ()))
}

fn futimes_sync(
    ctx: Ctx<'_>,
    descriptor: u32,
    _atime: Value<'_>,
    _mtime: Value<'_>,
) -> rquickjs::Result<()> {
    vfs_call(&ctx, |vfs| vfs.fstat(descriptor).map(|_| ()))
}

fn lchmod_sync<'js>(ctx: Ctx<'js>, input: Value<'js>, mode: Value<'js>) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    validate_mode(&ctx, mode)?;
    vfs_call(&ctx, |vfs| vfs.lstat(&path).map(|_| ()))
}

fn lchown_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    _uid: u32,
    _gid: u32,
) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    vfs_call(&ctx, |vfs| vfs.lstat(&path).map(|_| ()))
}

fn lutimes_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    _atime: Value<'js>,
    _mtime: Value<'js>,
) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    vfs_call(&ctx, |vfs| vfs.lstat(&path).map(|_| ()))
}

fn utimes_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    _atime: Value<'js>,
    _mtime: Value<'js>,
) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    vfs_call(&ctx, |vfs| vfs.stat(&path).map(|_| ()))
}

fn mkdir_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    let options_value = options.0;
    let options = if let Some(value) = options_value.as_ref()
        && value.is_number()
    {
        validate_mode(&ctx, value.clone())?;
        FsOptions::default()
    } else {
        if let Some(mode) = option_property(&ctx, options_value.as_ref(), "mode")?
            && !mode.is_undefined()
        {
            validate_mode(&ctx, mode)?;
        }
        parse_options(&ctx, options_value)?
    };
    vfs_call(&ctx, |vfs| vfs.mkdir(&path, options.recursive))
}

fn readdir_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Array<'js>> {
    let path = path(&ctx, input)?;
    let options = parse_options(&ctx, options.0)?;
    let entries: Vec<(String, DirectoryEntry)> = if options.recursive {
        vfs_call(&ctx, |vfs| vfs.walk(&path))?
    } else {
        vfs_call(&ctx, |vfs| vfs.read_dir(&path))?
            .into_iter()
            .map(|entry| {
                let entry_path = if path == "/" {
                    format!("/{name}", name = entry.name)
                } else {
                    format!("{path}/{name}", name = entry.name)
                };
                (entry_path, entry)
            })
            .collect()
    };
    let result = Array::new(ctx.clone())?;
    for (index, (entry_path, entry)) in entries.into_iter().enumerate() {
        let name = if options.recursive {
            entry_path
                .strip_prefix(&path)
                .unwrap_or(&entry_path)
                .trim_start_matches('/')
                .to_owned()
        } else {
            entry.name.clone()
        };
        if options.with_file_types {
            let parent_path = entry_path
                .rsplit_once('/')
                .map_or(path.as_str(), |(parent, _)| normalized_parent(parent));
            result.set(
                index,
                dirent(
                    &ctx,
                    &entry,
                    name_value(&ctx, &name, options.encoding.as_deref())?,
                    parent_path,
                )?,
            )?;
        } else {
            result.set(index, name_value(&ctx, &name, options.encoding.as_deref())?)?;
        }
    }
    Ok(result)
}

fn stat_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Value<'js>> {
    let options = parse_options(&ctx, options.0)?;
    match path_or_fd(&ctx, input)? {
        PathOrFd::Path(path) => stat_value(&ctx, &path, true, &options),
        PathOrFd::Fd(descriptor) => {
            let stat = vfs_call(&ctx, |vfs| vfs.fstat(descriptor))?;
            stat_object(&ctx, &stat, options.bigint).map(Object::into_value)
        }
    }
}

fn lstat_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Value<'js>> {
    let path = path(&ctx, input)?;
    let options = parse_options(&ctx, options.0)?;
    stat_value(&ctx, &path, false, &options)
}

fn stat_value<'js>(
    ctx: &Ctx<'js>,
    path: &str,
    follow_symlinks: bool,
    options: &FsOptions,
) -> rquickjs::Result<Value<'js>> {
    let vfs = vfs_handle(ctx)?;
    let result = if follow_symlinks {
        lock(&vfs).stat(path)
    } else {
        lock(&vfs).lstat(path)
    };
    match result {
        Ok(stat) => stat_object(ctx, &stat, options.bigint).map(Object::into_value),
        Err(error)
            if error.kind() == appd_vfs::ErrorKind::NotFound && !options.throw_if_no_entry =>
        {
            Ok(Value::new_undefined(ctx.clone()))
        }
        Err(error) => Err(vfs_exception(ctx, &error)?.throw()),
    }
}

fn exists_sync<'js>(ctx: Ctx<'js>, input: Value<'js>) -> rquickjs::Result<bool> {
    let path = path(&ctx, input)?;
    let vfs = vfs_handle(&ctx)?;
    Ok(lock(&vfs).stat(&path).is_ok())
}

fn unlink_sync<'js>(ctx: Ctx<'js>, input: Value<'js>) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    vfs_call(&ctx, |vfs| vfs.remove_file(&path))
}

fn rm_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    let options = parse_options(&ctx, options.0)?;
    vfs_call(&ctx, |vfs| {
        vfs.remove(&path, options.recursive, options.force)
    })
}

fn rmdir_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    let options = parse_options(&ctx, options.0)?;
    vfs_call(&ctx, |vfs| vfs.remove_directory(&path, options.recursive))
}

fn rename_sync<'js>(ctx: Ctx<'js>, from: Value<'js>, to: Value<'js>) -> rquickjs::Result<()> {
    let from = path(&ctx, from)?;
    let to = path(&ctx, to)?;
    vfs_call(&ctx, |vfs| vfs.rename(&from, &to))
}

fn copy_file_sync<'js>(
    ctx: Ctx<'js>,
    from: Value<'js>,
    to: Value<'js>,
    mode: Opt<u32>,
) -> rquickjs::Result<()> {
    let from = path(&ctx, from)?;
    let to = path(&ctx, to)?;
    let mode = mode.0.unwrap_or_default();
    if !matches!(mode, 0..=2) {
        return Err(Exception::throw_range(&ctx, "unsupported copy mode"));
    }
    vfs_call(&ctx, |vfs| {
        vfs.copy_file_with_options(&from, &to, mode & 1 != 0)
    })
}

fn cp_sync<'js>(
    ctx: Ctx<'js>,
    from: Value<'js>,
    to: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<()> {
    let from = path(&ctx, from)?;
    let to = path(&ctx, to)?;
    let options_value = options.0;
    if let Some(mode) = option_property(&ctx, options_value.as_ref(), "mode")? {
        let mode = Coerced::<i64>::from_js(&ctx, mode)?.0;
        let mode = u32::try_from(mode)
            .map_err(|_| Exception::throw_range(&ctx, "options.mode is out of range"))?;
        if mode & 4 != 0 {
            return Err(Exception::throw_message(
                &ctx,
                "COPYFILE_FICLONE_FORCE is not supported",
            ));
        }
    }
    if let Some(filter) = option_property(&ctx, options_value.as_ref(), "filter")?
        && !filter.is_undefined()
    {
        if !filter.is_function() {
            return Err(Exception::throw_type(
                &ctx,
                "options.filter must be a function",
            ));
        }
        return Err(Exception::throw_message(
            &ctx,
            "options.filter is not supported",
        ));
    }
    for name in ["preserveTimestamps", "verbatimSymlinks"] {
        if let Some(value) = option_property(&ctx, options_value.as_ref(), name)? {
            let _ = bool::from_js(&ctx, value)?;
        }
    }
    let mut options = parse_options(&ctx, options_value.clone())?;
    options.force = option_property(&ctx, options_value.as_ref(), "force")?
        .map(|value| bool::from_js(&ctx, value))
        .transpose()?
        .unwrap_or(true);
    vfs_call(&ctx, |vfs| {
        vfs.copy(
            &from,
            &to,
            CopyOptions {
                recursive: options.recursive,
                force: options.force,
                error_on_exist: options.error_on_exist,
                dereference: options.dereference,
            },
        )
    })
}

fn link_sync<'js>(ctx: Ctx<'js>, existing: Value<'js>, new: Value<'js>) -> rquickjs::Result<()> {
    let existing = path(&ctx, existing)?;
    let new = path(&ctx, new)?;
    vfs_call(&ctx, |vfs| vfs.link(&existing, &new))
}

fn mkdtemp_sync<'js>(
    ctx: Ctx<'js>,
    prefix: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Value<'js>> {
    let prefix = path(&ctx, prefix)?;
    let directory = vfs_call(&ctx, |vfs| vfs.make_temp_dir(&prefix))?;
    let options = parse_options(&ctx, options.0)?;
    text_value(&ctx, &directory, options.encoding.as_deref())
}

fn opendir_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    let path = path(&ctx, input)?;
    let options = parse_options(&ctx, options.0)?;
    let entries = if options.recursive {
        vfs_call(&ctx, |vfs| vfs.walk(&path))?
            .into_iter()
            .map(|(entry_path, entry)| DirItem {
                name: entry_path
                    .strip_prefix(&path)
                    .unwrap_or(&entry_path)
                    .trim_start_matches('/')
                    .to_owned(),
                parent: entry_path.rsplit_once('/').map_or_else(
                    || "/".to_owned(),
                    |(parent, _)| normalized_parent(parent).to_owned(),
                ),
                entry,
            })
            .collect()
    } else {
        vfs_call(&ctx, |vfs| vfs.read_dir(&path))?
            .into_iter()
            .map(|entry| DirItem {
                name: entry.name.clone(),
                parent: path.clone(),
                entry,
            })
            .collect()
    };
    dir_object(&ctx, path, entries, options.encoding.as_deref())
}

fn readv_sync<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    buffers: Array<'js>,
    position: Opt<Value<'js>>,
) -> rquickjs::Result<u32> {
    let position = position_value(&ctx, position.0)?;
    let mut lengths = Vec::with_capacity(buffers.len());
    let mut writable = Vec::with_capacity(buffers.len());
    for index in 0..buffers.len() {
        let value: Value = buffers.get(index)?;
        let bytes = writable_bytes(&ctx, value)?;
        lengths.push(bytes.len);
        writable.push(bytes);
    }
    let chunks = vfs_call(&ctx, |vfs| vfs.readv(descriptor, &lengths, position))?;
    let mut total = 0_usize;
    for (buffer, bytes) in writable.iter().zip(chunks) {
        buffer.write(0, &bytes);
        total += bytes.len();
    }
    u32::try_from(total).map_err(|_| Exception::throw_range(&ctx, "read is too large"))
}

fn writev_sync<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    buffers: Array<'js>,
    position: Opt<Value<'js>>,
) -> rquickjs::Result<u32> {
    let position = position_value(&ctx, position.0)?;
    let mut values = Vec::with_capacity(buffers.len());
    for index in 0..buffers.len() {
        let value: Value = buffers.get(index)?;
        values.push(bytes(&ctx, value, None)?);
    }
    let written = vfs_call(&ctx, |vfs| vfs.writev(descriptor, &values, position))?;
    u32::try_from(written).map_err(|_| Exception::throw_range(&ctx, "write is too large"))
}

fn statfs_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    let _path = path(&ctx, input)?;
    let options = parse_options(&ctx, options.0)?;
    statfs_object(&ctx, options.bigint)
}

fn open_as_blob<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    let path = path(&ctx, input)?;
    let options = parse_options(&ctx, options.0)?;
    let bytes = vfs_call(&ctx, |vfs| vfs.read_file(&path))?;
    blob_object(&ctx, bytes, options.blob_type.as_deref().unwrap_or(""))
}

fn glob_sync<'js>(
    ctx: Ctx<'js>,
    pattern: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Array<'js>> {
    let options_value = options.0;
    let exclude = option_property(&ctx, options_value.as_ref(), "exclude")?;
    let options = parse_options(&ctx, options_value)?;
    let patterns = if pattern.is_array() {
        let patterns = Array::from_value(pattern)?;
        let mut values = Vec::with_capacity(patterns.len());
        for index in 0..patterns.len() {
            values.push(patterns.get::<String>(index)?);
        }
        values
    } else {
        vec![Coerced::<String>::from_js(&ctx, pattern)?.0]
    };
    let cwd = options.cwd.clone().unwrap_or_else(|| "/bundle".to_owned());
    let cwd = path(&ctx, cwd.into_js(&ctx)?)?;
    let mut matches: Vec<Value<'js>> = Vec::new();
    for pattern in patterns {
        for (path, entry) in glob_matches(&ctx, &cwd, &pattern, &options)? {
            let relative = if pattern.starts_with('/') {
                path.clone()
            } else {
                path.strip_prefix(&cwd)
                    .unwrap_or(&path)
                    .trim_start_matches('/')
                    .to_owned()
            };
            if glob_excluded(
                &ctx,
                exclude.as_ref(),
                &relative,
                &entry,
                options.with_file_types,
                &path,
            )? {
                continue;
            }
            if options.with_file_types {
                let name = path.rsplit('/').next().unwrap_or_default();
                let parent = path.rsplit_once('/').map_or("/", |(parent, _)| parent);
                matches.push(
                    dirent(
                        &ctx,
                        &entry,
                        name_value(&ctx, name, options.encoding.as_deref())?,
                        normalized_parent(parent),
                    )?
                    .into_value(),
                );
            } else {
                matches.push(name_value(
                    &ctx,
                    if pattern.starts_with('/') {
                        &path
                    } else {
                        path.strip_prefix(&cwd)
                            .unwrap_or(&path)
                            .trim_start_matches('/')
                    },
                    options.encoding.as_deref(),
                )?);
            }
        }
    }
    let result = Array::new(ctx.clone())?;
    for (index, value) in matches.into_iter().enumerate() {
        result.set(index, value)?;
    }
    Ok(result)
}

fn glob_excluded<'js>(
    ctx: &Ctx<'js>,
    exclude: Option<&Value<'js>>,
    relative: &str,
    entry: &DirectoryEntry,
    with_file_types: bool,
    parent_path: &str,
) -> rquickjs::Result<bool> {
    let Some(exclude) = exclude else {
        return Ok(false);
    };
    if let Ok(function) = Function::from_value(exclude.clone()) {
        let value = if with_file_types {
            dirent(
                ctx,
                entry,
                name_value(ctx, relative.rsplit('/').next().unwrap_or_default(), None)?,
                parent_path
                    .rsplit_once('/')
                    .map_or("/", |(parent, _)| normalized_parent(parent)),
            )?
            .into_value()
        } else {
            relative.to_owned().into_js(ctx)?
        };
        return function.call::<_, bool>((value,));
    }
    let patterns = Array::from_value(exclude.clone())
        .map_err(|_| Exception::throw_type(ctx, "options.exclude must be a function or array"))?;
    for index in 0..patterns.len() {
        let pattern: String = patterns.get(index)?;
        if glob_match(&pattern, relative) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn symlink_sync<'js>(ctx: Ctx<'js>, target: Value<'js>, input: Value<'js>) -> rquickjs::Result<()> {
    let target: Coerced<std::string::String> = Coerced::from_js(&ctx, target)?;
    let path = path(&ctx, input)?;
    vfs_call(&ctx, |vfs| vfs.symlink(target.as_ref(), &path))
}

fn read_link_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Value<'js>> {
    let path = path(&ctx, input)?;
    let target = vfs_call(&ctx, |vfs| vfs.read_link(&path))?;
    let options = parse_options(&ctx, options.0)?;
    text_value(&ctx, &target, options.encoding.as_deref())
}

fn realpath_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Value<'js>> {
    let path = path(&ctx, input)?;
    let target = vfs_call(&ctx, |vfs| vfs.realpath(&path))?;
    let options = parse_options(&ctx, options.0)?;
    text_value(&ctx, &target, options.encoding.as_deref())
}

fn open_sync<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    flags: Opt<Value<'js>>,
    mode: Opt<Value<'js>>,
) -> rquickjs::Result<u32> {
    let path = path(&ctx, input)?;
    if let Some(mode) = mode.0 {
        validate_mode(&ctx, mode)?;
    }
    let options = open_options(&ctx, flags.0)?;
    vfs_call(&ctx, |vfs| vfs.open(&path, options))
}

fn close_sync(ctx: Ctx<'_>, descriptor: u32) -> rquickjs::Result<()> {
    vfs_call(&ctx, |vfs| vfs.close(descriptor))
}

fn fstat_sync<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    let options = parse_options(&ctx, options.0)?;
    let stat = vfs_call(&ctx, |vfs| vfs.fstat(descriptor))?;
    stat_object(&ctx, &stat, options.bigint)
}

fn read_sync<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    buffer: Value<'js>,
    offset: Opt<u32>,
    length: Opt<u32>,
    position: Opt<Value<'js>>,
) -> rquickjs::Result<u32> {
    let buffer = writable_bytes(&ctx, buffer)?;
    let offset = offset.0.unwrap_or(0) as usize;
    if offset > buffer.len {
        return Err(Exception::throw_range(&ctx, "offset is outside the buffer"));
    }
    let length = length
        .0
        .map_or(buffer.len - offset, |length| length as usize);
    if length > buffer.len - offset {
        return Err(Exception::throw_range(&ctx, "length is outside the buffer"));
    }
    let position = position_value(&ctx, position.0)?;
    let bytes = vfs_call(&ctx, |vfs| vfs.read(descriptor, length, position))?;
    buffer.write(offset, &bytes);
    u32::try_from(bytes.len()).map_err(|_| Exception::throw_range(&ctx, "read is too large"))
}

fn read_sync_export<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    buffer: Value<'js>,
    offset_or_options: Opt<Value<'js>>,
    length: Opt<Value<'js>>,
    position: Opt<Value<'js>>,
) -> rquickjs::Result<u32> {
    let (offset, length, position) = if let Some(value) = offset_or_options.0 {
        if value.is_object() {
            let options = value
                .clone()
                .try_into_object()
                .map_err(|_| Exception::throw_type(&ctx, "read options must be an object"))?;
            (
                options.get("offset")?,
                options.get("length")?,
                options.get("position")?,
            )
        } else {
            (Some(value), length.0, position.0)
        }
    } else {
        (None, length.0, position.0)
    };
    read_sync(
        ctx.clone(),
        descriptor,
        buffer,
        optional_u32(&ctx, offset)?,
        optional_u32(&ctx, length)?,
        Opt(position),
    )
}

fn write_sync<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    value: Value<'js>,
    args: Rest<Value<'js>>,
) -> rquickjs::Result<u32> {
    let (bytes, position) = write_arguments(&ctx, value, args.0)?;
    let written = vfs_call(&ctx, |vfs| vfs.write(descriptor, &bytes, position))?;
    u32::try_from(written).map_err(|_| Exception::throw_range(&ctx, "write is too large"))
}

fn write_sync_export<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    value: Value<'js>,
    offset_or_options: Opt<Value<'js>>,
    length: Opt<Value<'js>>,
    position: Opt<Value<'js>>,
) -> rquickjs::Result<u32> {
    let data_is_string = value.is_string();
    let args = if let Some(value) = offset_or_options.0 {
        if value.is_object() {
            let options = value
                .clone()
                .try_into_object()
                .map_err(|_| Exception::throw_type(&ctx, "write options must be an object"))?;
            let offset = options.get::<_, Option<Value>>("offset")?;
            let length = options.get::<_, Option<Value>>("length")?;
            let position = options.get::<_, Option<Value>>("position")?;
            if data_is_string {
                vec![
                    position.unwrap_or_else(|| Value::new_undefined(ctx.clone())),
                    options
                        .get::<_, Option<Value>>("encoding")?
                        .unwrap_or_else(|| Value::new_undefined(ctx.clone())),
                ]
            } else {
                vec![
                    offset.unwrap_or_else(|| Value::new_undefined(ctx.clone())),
                    length.unwrap_or_else(|| Value::new_undefined(ctx.clone())),
                    position.unwrap_or_else(|| Value::new_undefined(ctx.clone())),
                ]
            }
        } else {
            [Some(value), length.0, position.0]
                .into_iter()
                .flatten()
                .collect()
        }
    } else {
        [None, length.0, position.0].into_iter().flatten().collect()
    };
    write_sync(ctx, descriptor, value, Rest(args))
}

fn truncate_sync<'js>(ctx: Ctx<'js>, input: Value<'js>, length: Opt<u64>) -> rquickjs::Result<()> {
    let path = path(&ctx, input)?;
    vfs_call(&ctx, |vfs| vfs.truncate(&path, length.0.unwrap_or(0)))
}

fn ftruncate_sync(ctx: Ctx<'_>, descriptor: u32, length: Opt<u64>) -> rquickjs::Result<()> {
    vfs_call(&ctx, |vfs| vfs.ftruncate(descriptor, length.0.unwrap_or(0)))
}

fn read_file_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(ctx.clone(), read_file_sync(ctx, input, options))
}

fn write_file_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    data: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        write_file_sync(ctx.clone(), input, data, options)
            .map(|()| Value::new_undefined(ctx.clone())),
    )
}

fn append_file_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    data: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        append_file_sync(ctx.clone(), input, data, options)
            .map(|()| Value::new_undefined(ctx.clone())),
    )
}

fn mkdir_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        mkdir_sync(ctx.clone(), input, options).map(|()| Value::new_undefined(ctx.clone())),
    )
}

fn readdir_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        readdir_sync(ctx.clone(), input, options).map(Array::into_value),
    )
}

fn stat_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(ctx.clone(), stat_sync(ctx.clone(), input, options))
}

fn lstat_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(ctx.clone(), lstat_sync(ctx.clone(), input, options))
}

fn unlink_promise<'js>(ctx: Ctx<'js>, input: Value<'js>) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        unlink_sync(ctx.clone(), input).map(|()| Value::new_undefined(ctx.clone())),
    )
}

fn rm_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        rm_sync(ctx.clone(), input, options).map(|()| Value::new_undefined(ctx.clone())),
    )
}

fn rmdir_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        rmdir_sync(ctx.clone(), input, options).map(|()| Value::new_undefined(ctx)),
    )
}

fn rename_promise<'js>(
    ctx: Ctx<'js>,
    from: Value<'js>,
    to: Value<'js>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        rename_sync(ctx.clone(), from, to).map(|()| Value::new_undefined(ctx.clone())),
    )
}

fn copy_file_promise<'js>(
    ctx: Ctx<'js>,
    from: Value<'js>,
    to: Value<'js>,
    mode: Opt<u32>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        copy_file_sync(ctx.clone(), from, to, mode).map(|()| Value::new_undefined(ctx.clone())),
    )
}

fn access_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    mode: Opt<u32>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        access_sync(ctx.clone(), input, mode).map(|()| Value::new_undefined(ctx)),
    )
}

fn chmod_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    mode: Value<'js>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        chmod_sync(ctx.clone(), input, mode).map(|()| Value::new_undefined(ctx)),
    )
}

fn chown_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    uid: u32,
    gid: u32,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        chown_sync(ctx.clone(), input, uid, gid).map(|()| Value::new_undefined(ctx)),
    )
}

fn cp_promise<'js>(
    ctx: Ctx<'js>,
    from: Value<'js>,
    to: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        cp_sync(ctx.clone(), from, to, options).map(|()| Value::new_undefined(ctx)),
    )
}

fn fchmod_promise<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    mode: Value<'js>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        fchmod_sync(ctx.clone(), descriptor, mode).map(|()| Value::new_undefined(ctx)),
    )
}

fn fchown_promise<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    uid: u32,
    gid: u32,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        fchown_sync(ctx.clone(), descriptor, uid, gid).map(|()| Value::new_undefined(ctx)),
    )
}

fn fdatasync_promise<'js>(ctx: Ctx<'js>, descriptor: u32) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        fdatasync_sync(ctx.clone(), descriptor).map(|()| Value::new_undefined(ctx)),
    )
}

fn fsync_promise<'js>(ctx: Ctx<'js>, descriptor: u32) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        fsync_sync(ctx.clone(), descriptor).map(|()| Value::new_undefined(ctx)),
    )
}

fn fstat_promise<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        fstat_sync(ctx.clone(), descriptor, options).map(Object::into_value),
    )
}

fn ftruncate_promise<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    length: Opt<u64>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        ftruncate_sync(ctx.clone(), descriptor, length).map(|()| Value::new_undefined(ctx)),
    )
}

fn futimes_promise<'js>(
    ctx: Ctx<'js>,
    descriptor: u32,
    atime: Value<'js>,
    mtime: Value<'js>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        futimes_sync(ctx.clone(), descriptor, atime, mtime).map(|()| Value::new_undefined(ctx)),
    )
}

fn glob_promise<'js>(
    ctx: Ctx<'js>,
    pattern: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    let values = glob_sync(ctx.clone(), pattern, options)?;
    let values = (0..values.len())
        .map(|index| values.get(index))
        .collect::<rquickjs::Result<Vec<Value>>>()?;
    async_value_iterator(&ctx, values)
}

#[allow(clippy::arc_with_non_send_sync)]
fn async_value_iterator<'js>(
    ctx: &Ctx<'js>,
    values: Vec<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    let state = Arc::new(Mutex::new(values));
    let iterator = Object::new(ctx.clone())?;
    let next_state = Arc::clone(&state);
    iterator.set(
        "next",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            let mut values = lock(&next_state);
            let value = values
                .is_empty()
                .then(|| Value::new_undefined(ctx.clone()))
                .or_else(|| Some(values.remove(0)));
            let done = value.as_ref().is_some_and(Value::is_undefined);
            let result = Object::new(ctx.clone())?;
            result.set("done", done)?;
            result.set(
                "value",
                value.unwrap_or_else(|| Value::new_undefined(ctx.clone())),
            )?;
            promise(ctx.clone(), Ok(result.into_value())).map(Promise::into_value)
        })?,
    )?;
    iterator.set(
        Symbol::async_iterator(ctx.clone()),
        Function::new(ctx.clone(), |this: This<Object<'js>>| {
            Ok::<Object<'js>, rquickjs::Error>(this.0)
        })?,
    )?;
    Ok(iterator)
}

fn lchmod_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    mode: Value<'js>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        lchmod_sync(ctx.clone(), input, mode).map(|()| Value::new_undefined(ctx)),
    )
}

fn lchown_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    uid: u32,
    gid: u32,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        lchown_sync(ctx.clone(), input, uid, gid).map(|()| Value::new_undefined(ctx)),
    )
}

fn link_promise<'js>(
    ctx: Ctx<'js>,
    existing: Value<'js>,
    new: Value<'js>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        link_sync(ctx.clone(), existing, new).map(|()| Value::new_undefined(ctx)),
    )
}

fn lutimes_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    atime: Value<'js>,
    mtime: Value<'js>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        lutimes_sync(ctx.clone(), input, atime, mtime).map(|()| Value::new_undefined(ctx)),
    )
}

fn mkdtemp_promise<'js>(
    ctx: Ctx<'js>,
    prefix: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(ctx.clone(), mkdtemp_sync(ctx.clone(), prefix, options))
}

fn open_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    flags: Opt<Value<'js>>,
    mode: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    let descriptor = open_sync(ctx.clone(), input, flags, mode)?;
    let state = FileHandleState {
        descriptor: Arc::new(Mutex::new(Some(descriptor))),
        owns_descriptor: true,
    };
    promise(
        ctx.clone(),
        file_handle_object(&ctx, state).map(Object::into_value),
    )
}

fn opendir_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        opendir_sync(ctx.clone(), input, options).map(Object::into_value),
    )
}

fn statfs_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        statfs_sync(ctx.clone(), input, options).map(Object::into_value),
    )
}

fn utimes_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    atime: Value<'js>,
    mtime: Value<'js>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        utimes_sync(ctx.clone(), input, atime, mtime).map(|()| Value::new_undefined(ctx)),
    )
}

fn symlink_promise<'js>(
    ctx: Ctx<'js>,
    target: Value<'js>,
    input: Value<'js>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        symlink_sync(ctx.clone(), target, input).map(|()| Value::new_undefined(ctx.clone())),
    )
}

fn read_link_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(ctx.clone(), read_link_sync(ctx, input, options))
}

fn realpath_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    promise(ctx.clone(), realpath_sync(ctx, input, options))
}

fn truncate_promise<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    length: Opt<u64>,
) -> rquickjs::Result<Promise<'js>> {
    promise(
        ctx.clone(),
        truncate_sync(ctx.clone(), input, length).map(|()| Value::new_undefined(ctx.clone())),
    )
}

fn promise<'js>(
    ctx: Ctx<'js>,
    result: rquickjs::Result<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    let (promise, resolve, reject) = Promise::new(&ctx)?;
    match result {
        Ok(value) => resolve.call::<_, ()>((value,))?,
        Err(_error) if ctx.has_exception() => reject.call::<_, ()>((ctx.catch(),))?,
        Err(error) => return Err(error),
    }
    Ok(promise)
}

#[derive(Clone, Copy)]
enum CallbackOperation {
    Access,
    Exists,
    AppendFile,
    Chmod,
    Chown,
    Close,
    CopyFile,
    Cp,
    Fchmod,
    Fchown,
    Fdatasync,
    Fstat,
    Fsync,
    Ftruncate,
    Futimes,
    Glob,
    Lchmod,
    Lchown,
    Link,
    Lstat,
    Lutimes,
    Mkdir,
    Mkdtemp,
    Open,
    Opendir,
    Read,
    ReadFile,
    Readlink,
    Readv,
    Readdir,
    Realpath,
    Rename,
    Rm,
    Rmdir,
    Stat,
    Statfs,
    Symlink,
    Truncate,
    Unlink,
    Utimes,
    Write,
    WriteFile,
    Writev,
}

fn callback_operations() -> &'static [(&'static str, CallbackOperation)] {
    &[
        ("access", CallbackOperation::Access),
        ("exists", CallbackOperation::Exists),
        ("appendFile", CallbackOperation::AppendFile),
        ("chmod", CallbackOperation::Chmod),
        ("chown", CallbackOperation::Chown),
        ("close", CallbackOperation::Close),
        ("copyFile", CallbackOperation::CopyFile),
        ("cp", CallbackOperation::Cp),
        ("fchmod", CallbackOperation::Fchmod),
        ("fchown", CallbackOperation::Fchown),
        ("fdatasync", CallbackOperation::Fdatasync),
        ("fstat", CallbackOperation::Fstat),
        ("fsync", CallbackOperation::Fsync),
        ("ftruncate", CallbackOperation::Ftruncate),
        ("futimes", CallbackOperation::Futimes),
        ("glob", CallbackOperation::Glob),
        ("lchmod", CallbackOperation::Lchmod),
        ("lchown", CallbackOperation::Lchown),
        ("link", CallbackOperation::Link),
        ("lstat", CallbackOperation::Lstat),
        ("lutimes", CallbackOperation::Lutimes),
        ("mkdir", CallbackOperation::Mkdir),
        ("mkdtemp", CallbackOperation::Mkdtemp),
        ("open", CallbackOperation::Open),
        ("opendir", CallbackOperation::Opendir),
        ("read", CallbackOperation::Read),
        ("readFile", CallbackOperation::ReadFile),
        ("readlink", CallbackOperation::Readlink),
        ("readv", CallbackOperation::Readv),
        ("readdir", CallbackOperation::Readdir),
        ("realpath", CallbackOperation::Realpath),
        ("rename", CallbackOperation::Rename),
        ("rm", CallbackOperation::Rm),
        ("rmdir", CallbackOperation::Rmdir),
        ("stat", CallbackOperation::Stat),
        ("statfs", CallbackOperation::Statfs),
        ("symlink", CallbackOperation::Symlink),
        ("truncate", CallbackOperation::Truncate),
        ("unlink", CallbackOperation::Unlink),
        ("utimes", CallbackOperation::Utimes),
        ("write", CallbackOperation::Write),
        ("writeFile", CallbackOperation::WriteFile),
        ("writev", CallbackOperation::Writev),
    ]
}

#[allow(clippy::too_many_lines)]
fn callback_call<'js>(
    ctx: Ctx<'js>,
    operation: CallbackOperation,
    args: Rest<Value<'js>>,
) -> rquickjs::Result<()> {
    let mut args = args.0;
    let callback = if matches!(
        operation,
        CallbackOperation::Close | CallbackOperation::Fsync
    ) && args.len() == 1
    {
        None
    } else {
        let callback = args
            .pop()
            .ok_or_else(|| Exception::throw_type(&ctx, "callback is required"))?;
        Some(
            Function::from_value(callback)
                .map_err(|_| Exception::throw_type(&ctx, "callback must be a function"))?,
        )
    };
    if matches!(operation, CallbackOperation::Exists) {
        let callback =
            callback.ok_or_else(|| Exception::throw_type(&ctx, "callback is required"))?;
        let input = take_arg(&ctx, &mut args)?;
        let exists = exists_sync(ctx.clone(), input)?;
        return callback_value(&ctx, callback, exists);
    }
    match operation {
        CallbackOperation::Read => {
            let callback =
                callback.ok_or_else(|| Exception::throw_type(&ctx, "callback is required"))?;
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let ReadArguments {
                buffer,
                offset,
                length,
                position,
            } = normalize_read_arguments(&ctx, args)?;
            let result = read_sync(
                ctx.clone(),
                descriptor,
                buffer.clone(),
                optional_u32(&ctx, offset)?,
                optional_u32(&ctx, length)?,
                Opt(position),
            )
            .and_then(|count| {
                Ok(vec![
                    Value::new_null(ctx.clone()),
                    count.into_js(&ctx)?,
                    buffer,
                ])
            });
            return callback_values(&ctx, callback, result);
        }
        CallbackOperation::Write => {
            let callback =
                callback.ok_or_else(|| Exception::throw_type(&ctx, "callback is required"))?;
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let value = take_arg(&ctx, &mut args)?;
            let result =
                write_sync(ctx.clone(), descriptor, value.clone(), Rest(args)).and_then(|count| {
                    Ok(vec![
                        Value::new_null(ctx.clone()),
                        count.into_js(&ctx)?,
                        value,
                    ])
                });
            return callback_values(&ctx, callback, result);
        }
        CallbackOperation::Readv => {
            let callback =
                callback.ok_or_else(|| Exception::throw_type(&ctx, "callback is required"))?;
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let buffers = take_arg(&ctx, &mut args)?;
            let array = Array::from_value(buffers.clone())?;
            let position = take_opt_value(&mut args);
            let result = readv_sync(ctx.clone(), descriptor, array, position).and_then(|count| {
                Ok(vec![
                    Value::new_null(ctx.clone()),
                    count.into_js(&ctx)?,
                    buffers,
                ])
            });
            return callback_values(&ctx, callback, result);
        }
        CallbackOperation::Writev => {
            let callback =
                callback.ok_or_else(|| Exception::throw_type(&ctx, "callback is required"))?;
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let buffers = take_arg(&ctx, &mut args)?;
            let array = Array::from_value(buffers.clone())?;
            let position = take_opt_value(&mut args);
            let result = writev_sync(ctx.clone(), descriptor, array, position).and_then(|count| {
                Ok(vec![
                    Value::new_null(ctx.clone()),
                    count.into_js(&ctx)?,
                    buffers,
                ])
            });
            return callback_values(&ctx, callback, result);
        }
        _ => {}
    }
    let result = match operation {
        CallbackOperation::Access => {
            let input = take_arg(&ctx, &mut args)?;
            access_sync(ctx.clone(), input, take_opt_u32(&ctx, &mut args)?)
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::AppendFile => {
            let input = take_arg(&ctx, &mut args)?;
            let data = take_arg(&ctx, &mut args)?;
            append_file_sync(ctx.clone(), input, data, take_opt_value(&mut args))
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Chmod => {
            let input = take_arg(&ctx, &mut args)?;
            let mode = take_arg(&ctx, &mut args)?;
            chmod_sync(ctx.clone(), input, mode).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Chown => {
            let input = take_arg(&ctx, &mut args)?;
            let uid = take_arg(&ctx, &mut args)?;
            let gid = take_arg(&ctx, &mut args)?;
            chown_sync(
                ctx.clone(),
                input,
                numeric_u32(&ctx, uid)?,
                numeric_u32(&ctx, gid)?,
            )
            .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Close => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            close_sync(ctx.clone(), descriptor).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::CopyFile => {
            let from = take_arg(&ctx, &mut args)?;
            let to = take_arg(&ctx, &mut args)?;
            copy_file_sync(ctx.clone(), from, to, take_opt_u32(&ctx, &mut args)?)
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Cp => {
            let from = take_arg(&ctx, &mut args)?;
            let to = take_arg(&ctx, &mut args)?;
            cp_sync(ctx.clone(), from, to, take_opt_value(&mut args))
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Fchmod => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let mode = take_arg(&ctx, &mut args)?;
            fchmod_sync(ctx.clone(), descriptor, mode).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Fchown => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let uid = numeric_u32(&ctx, take_arg(&ctx, &mut args)?)?;
            let gid = numeric_u32(&ctx, take_arg(&ctx, &mut args)?)?;
            fchown_sync(ctx.clone(), descriptor, uid, gid)
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Fdatasync => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            fdatasync_sync(ctx.clone(), descriptor).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Fstat => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            fstat_sync(ctx.clone(), descriptor, take_opt_value(&mut args)).map(Object::into_value)
        }
        CallbackOperation::Fsync => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            fsync_sync(ctx.clone(), descriptor).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Ftruncate => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            ftruncate_sync(ctx.clone(), descriptor, take_opt_u64(&ctx, &mut args)?)
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Futimes => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let atime = take_arg(&ctx, &mut args)?;
            let mtime = take_arg(&ctx, &mut args)?;
            futimes_sync(ctx.clone(), descriptor, atime, mtime)
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Glob => {
            let pattern = take_arg(&ctx, &mut args)?;
            glob_sync(ctx.clone(), pattern, take_opt_value(&mut args)).map(Array::into_value)
        }
        CallbackOperation::Lchmod => {
            let input = take_arg(&ctx, &mut args)?;
            let mode = take_arg(&ctx, &mut args)?;
            lchmod_sync(ctx.clone(), input, mode).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Lchown => {
            let input = take_arg(&ctx, &mut args)?;
            let uid = numeric_u32(&ctx, take_arg(&ctx, &mut args)?)?;
            let gid = numeric_u32(&ctx, take_arg(&ctx, &mut args)?)?;
            lchown_sync(ctx.clone(), input, uid, gid).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Link => {
            let existing = take_arg(&ctx, &mut args)?;
            let new = take_arg(&ctx, &mut args)?;
            link_sync(ctx.clone(), existing, new).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Lstat => {
            let input = take_arg(&ctx, &mut args)?;
            lstat_sync(ctx.clone(), input, take_opt_value(&mut args))
        }
        CallbackOperation::Lutimes => {
            let input = take_arg(&ctx, &mut args)?;
            let atime = take_arg(&ctx, &mut args)?;
            let mtime = take_arg(&ctx, &mut args)?;
            lutimes_sync(ctx.clone(), input, atime, mtime)
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Mkdir => {
            let input = take_arg(&ctx, &mut args)?;
            mkdir_sync(ctx.clone(), input, take_opt_value(&mut args))
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Mkdtemp => {
            let input = take_arg(&ctx, &mut args)?;
            mkdtemp_sync(ctx.clone(), input, take_opt_value(&mut args))
        }
        CallbackOperation::Open => {
            let input = take_arg(&ctx, &mut args)?;
            let flags = take_front_value(&mut args);
            let mode = take_front_value(&mut args);
            open_sync(ctx.clone(), input, Opt(flags), Opt(mode))
                .and_then(|value| value.into_js(&ctx))
        }
        CallbackOperation::Opendir => {
            let input = take_arg(&ctx, &mut args)?;
            opendir_sync(ctx.clone(), input, take_opt_value(&mut args)).map(Object::into_value)
        }
        CallbackOperation::Read => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let buffer = take_arg(&ctx, &mut args)?;
            let offset = take_front_u32(&ctx, &mut args)?;
            let length = take_front_u32(&ctx, &mut args)?;
            let position = take_front_value(&mut args);
            read_sync(
                ctx.clone(),
                descriptor,
                buffer,
                offset,
                length,
                Opt(position),
            )
            .and_then(|count| count.into_js(&ctx))
        }
        CallbackOperation::ReadFile => {
            let input = take_arg(&ctx, &mut args)?;
            read_file_sync(ctx.clone(), input, take_opt_value(&mut args))
        }
        CallbackOperation::Readlink => {
            let input = take_arg(&ctx, &mut args)?;
            read_link_sync(ctx.clone(), input, take_opt_value(&mut args))
        }
        CallbackOperation::Readv => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let buffers = Array::from_value(take_arg(&ctx, &mut args)?)?;
            let position = take_opt_value(&mut args);
            readv_sync(ctx.clone(), descriptor, buffers, position)
                .and_then(|count| count.into_js(&ctx))
        }
        CallbackOperation::Readdir => {
            let input = take_arg(&ctx, &mut args)?;
            readdir_sync(ctx.clone(), input, take_opt_value(&mut args)).map(Array::into_value)
        }
        CallbackOperation::Realpath => {
            let input = take_arg(&ctx, &mut args)?;
            realpath_sync(ctx.clone(), input, take_opt_value(&mut args))
        }
        CallbackOperation::Rename => {
            let from = take_arg(&ctx, &mut args)?;
            let to = take_arg(&ctx, &mut args)?;
            rename_sync(ctx.clone(), from, to).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Rm => {
            let input = take_arg(&ctx, &mut args)?;
            rm_sync(ctx.clone(), input, take_opt_value(&mut args))
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Rmdir => {
            let input = take_arg(&ctx, &mut args)?;
            rmdir_sync(ctx.clone(), input, take_opt_value(&mut args))
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Stat => {
            let input = take_arg(&ctx, &mut args)?;
            stat_sync(ctx.clone(), input, take_opt_value(&mut args))
        }
        CallbackOperation::Statfs => {
            let input = take_arg(&ctx, &mut args)?;
            statfs_sync(ctx.clone(), input, take_opt_value(&mut args)).map(Object::into_value)
        }
        CallbackOperation::Symlink => {
            let target = take_arg(&ctx, &mut args)?;
            let input = take_arg(&ctx, &mut args)?;
            symlink_sync(ctx.clone(), target, input).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Truncate => {
            let input = take_arg(&ctx, &mut args)?;
            truncate_sync(ctx.clone(), input, take_opt_u64(&ctx, &mut args)?)
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Unlink => {
            let input = take_arg(&ctx, &mut args)?;
            unlink_sync(ctx.clone(), input).map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Utimes => {
            let input = take_arg(&ctx, &mut args)?;
            let atime = take_arg(&ctx, &mut args)?;
            let mtime = take_arg(&ctx, &mut args)?;
            utimes_sync(ctx.clone(), input, atime, mtime)
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Write => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let value = take_arg(&ctx, &mut args)?;
            write_sync(ctx.clone(), descriptor, value, Rest(args))
                .and_then(|count| count.into_js(&ctx))
        }
        CallbackOperation::WriteFile => {
            let input = take_arg(&ctx, &mut args)?;
            let data = take_arg(&ctx, &mut args)?;
            write_file_sync(ctx.clone(), input, data, take_opt_value(&mut args))
                .map(|()| Value::new_undefined(ctx.clone()))
        }
        CallbackOperation::Writev => {
            let descriptor = descriptor_value(&ctx, take_arg(&ctx, &mut args)?)?;
            let buffers = Array::from_value(take_arg(&ctx, &mut args)?)?;
            let position = take_opt_value(&mut args);
            writev_sync(ctx.clone(), descriptor, buffers, position)
                .and_then(|count| count.into_js(&ctx))
        }
        CallbackOperation::Exists => unreachable!(),
    };
    if let Some(callback) = callback {
        callback_result(&ctx, callback, result)
    } else {
        result.map(|_| ())
    }
}

fn take_arg<'js>(ctx: &Ctx<'js>, args: &mut Vec<Value<'js>>) -> rquickjs::Result<Value<'js>> {
    if args.is_empty() {
        return Err(Exception::throw_type(ctx, "missing filesystem argument"));
    }
    Ok(args.remove(0))
}

fn take_opt_value<'js>(args: &mut Vec<Value<'js>>) -> Opt<Value<'js>> {
    Opt(args.pop())
}

fn take_front_value<'js>(args: &mut Vec<Value<'js>>) -> Option<Value<'js>> {
    (!args.is_empty()).then(|| args.remove(0))
}

fn take_opt_u32<'js>(ctx: &Ctx<'js>, args: &mut Vec<Value<'js>>) -> rquickjs::Result<Opt<u32>> {
    optional_u32(ctx, args.pop())
}

fn take_front_u32<'js>(ctx: &Ctx<'js>, args: &mut Vec<Value<'js>>) -> rquickjs::Result<Opt<u32>> {
    optional_u32(ctx, take_front_value(args))
}

struct ReadArguments<'js> {
    buffer: Value<'js>,
    offset: Option<Value<'js>>,
    length: Option<Value<'js>>,
    position: Option<Value<'js>>,
}

fn normalize_read_arguments<'js>(
    ctx: &Ctx<'js>,
    mut args: Vec<Value<'js>>,
) -> rquickjs::Result<ReadArguments<'js>> {
    let first = args.first().cloned();
    match first {
        Some(value) if is_buffer_value(&value) => {
            args.remove(0);
            if args
                .first()
                .is_some_and(|value| value.is_object() && !is_buffer_value(value))
            {
                let options = args
                    .remove(0)
                    .try_into_object()
                    .map_err(|_| Exception::throw_type(ctx, "read options must be an object"))?;
                return Ok(ReadArguments {
                    buffer: value,
                    offset: options.get("offset")?,
                    length: options.get("length")?,
                    position: options.get("position")?,
                });
            }
            Ok(ReadArguments {
                buffer: value,
                offset: take_front_value(&mut args),
                length: take_front_value(&mut args),
                position: take_front_value(&mut args),
            })
        }
        Some(value) if value.is_object() => {
            let options = value
                .try_into_object()
                .map_err(|_| Exception::throw_type(ctx, "read options must be an object"))?;
            let length = options.get::<_, Option<Value>>("length")?;
            let buffer = options
                .get::<_, Option<Value>>("buffer")?
                .map_or_else(|| default_read_buffer(ctx, length.as_ref()), Ok)?;
            Ok(ReadArguments {
                buffer,
                offset: options.get("offset")?,
                length,
                position: options.get("position")?,
            })
        }
        None => Ok(ReadArguments {
            buffer: default_read_buffer(ctx, None)?,
            offset: None,
            length: None,
            position: None,
        }),
        Some(_) => Err(Exception::throw_type(
            ctx,
            "buffer or read options are required",
        )),
    }
}

fn default_read_buffer<'js>(
    ctx: &Ctx<'js>,
    length: Option<&Value<'js>>,
) -> rquickjs::Result<Value<'js>> {
    let length = length
        .map(|value| Coerced::<u64>::from_js(ctx, value.clone()))
        .transpose()?
        .map_or(16_384, |value| {
            usize::try_from(*value).unwrap_or(usize::MAX)
        });
    let length = u32::try_from(length)
        .map_err(|_| Exception::throw_range(ctx, "read buffer is too large"))?;
    TypedArray::<u8>::new(ctx.clone(), vec![0; length as usize]).map(TypedArray::into_value)
}

fn optional_u32<'js>(ctx: &Ctx<'js>, value: Option<Value<'js>>) -> rquickjs::Result<Opt<u32>> {
    if value
        .as_ref()
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(Opt(None));
    }
    value
        .map(|value| {
            let value = Coerced::<i64>::from_js(ctx, value)?.0;
            u32::try_from(value).map_err(|_| Exception::throw_range(ctx, "value is out of range"))
        })
        .transpose()
        .map(Opt)
}

fn numeric_u32<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<u32> {
    let value = Coerced::<i64>::from_js(ctx, value)?.0;
    u32::try_from(value).map_err(|_| Exception::throw_range(ctx, "value is out of range"))
}

fn take_opt_u64<'js>(ctx: &Ctx<'js>, args: &mut Vec<Value<'js>>) -> rquickjs::Result<Opt<u64>> {
    optional_u64(ctx, args.pop())
}

fn optional_u64<'js>(ctx: &Ctx<'js>, value: Option<Value<'js>>) -> rquickjs::Result<Opt<u64>> {
    if value
        .as_ref()
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(Opt(None));
    }
    value
        .map(|value| Coerced::<u64>::from_js(ctx, value).map(|value| value.0))
        .transpose()
        .map(Opt)
}

fn callback_result<'js>(
    ctx: &Ctx<'js>,
    callback: Function<'js>,
    result: rquickjs::Result<Value<'js>>,
) -> rquickjs::Result<()> {
    callback_values(
        ctx,
        callback,
        result.map(|value| vec![Value::new_null(ctx.clone()), value]),
    )
}

fn callback_value<'js, T>(ctx: &Ctx<'js>, callback: Function<'js>, value: T) -> rquickjs::Result<()>
where
    T: IntoJs<'js> + 'js,
{
    callback_values(ctx, callback, Ok(vec![value.into_js(ctx)?]))
}

fn callback_values<'js>(
    ctx: &Ctx<'js>,
    callback: Function<'js>,
    result: rquickjs::Result<Vec<Value<'js>>>,
) -> rquickjs::Result<()> {
    let values = match result {
        Ok(values) => values,
        Err(_error) if ctx.has_exception() => vec![ctx.catch()],
        Err(error) => return Err(error),
    };
    let task = Function::new(ctx.clone(), move |_: Ctx<'js>| {
        callback.call::<_, ()>((Rest(values.clone()),))
    })?;
    if let Some(queue_microtask) = ctx.globals().get::<_, Option<Function>>("queueMicrotask")? {
        queue_microtask.call::<_, ()>((task,))
    } else {
        task.call::<_, ()>(())
    }
}

fn flag_string<'js>(value: Option<&Value<'js>>) -> rquickjs::Result<Option<String>> {
    value
        .map(|value| {
            value
                .as_string()
                .ok_or_else(|| rquickjs::Error::new_from_js("value", "string"))?
                .to_string()
        })
        .transpose()
}

fn open_options<'js>(ctx: &Ctx<'js>, value: Option<Value<'js>>) -> rquickjs::Result<OpenOptions> {
    let Some(value) = value else {
        return Ok(OpenOptions::default());
    };
    if value.is_number() {
        let flags = *Coerced::<i32>::from_js(ctx, value)?;
        let access = flags & 3;
        return Ok(OpenOptions {
            read: access != 1,
            write: access != 0,
            append: flags & 1024 != 0,
            create: flags & 64 != 0,
            exclusive: flags & 128 != 0,
            truncate: flags & 512 != 0,
            ..OpenOptions::default()
        });
    }
    let flags: Coerced<std::string::String> = Coerced::from_js(ctx, value)?;
    let value = flags.as_ref();
    let first = value.chars().next().unwrap_or('r');
    if !matches!(first, 'r' | 'w' | 'a')
        || value
            .chars()
            .skip(1)
            .any(|character| !matches!(character, '+' | 'x' | 's'))
    {
        return Err(Exception::throw_type(ctx, "invalid file open flags"));
    }
    let plus = value.contains('+');
    Ok(OpenOptions {
        read: first == 'r' || plus,
        write: first != 'r' || plus,
        append: first == 'a',
        create: matches!(first, 'w' | 'a') || value.contains('x'),
        exclusive: value.contains('x'),
        truncate: first == 'w',
        ..OpenOptions::default()
    })
}

fn position_value<'js>(ctx: &Ctx<'js>, value: Option<Value<'js>>) -> rquickjs::Result<Option<u64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    let position: Coerced<i64> = Coerced::from_js(ctx, value)?;
    if *position < -1 {
        return Err(Exception::throw_range(
            ctx,
            "file position must be -1 or non-negative",
        ));
    }
    Ok((*position >= 0).then_some(position.cast_unsigned()))
}

fn write_arguments<'js>(
    ctx: &Ctx<'js>,
    value: Value<'js>,
    args: Vec<Value<'js>>,
) -> rquickjs::Result<(Vec<u8>, Option<u64>)> {
    let string = value.is_string();
    let (offset, length, position, encoding) = if string {
        let position = args.first().cloned();
        let encoding = args
            .get(1)
            .and_then(|value| value.as_string())
            .map(rquickjs::String::to_string)
            .transpose()?;
        (0_u64, None, position, encoding)
    } else {
        let offset = args
            .first()
            .map(|value| Coerced::<u64>::from_js(ctx, value.clone()))
            .transpose()?
            .map_or(0, |value| *value);
        let length = args
            .get(1)
            .map(|value| Coerced::<u64>::from_js(ctx, value.clone()))
            .transpose()?;
        (
            offset,
            length.map(|value| *value),
            args.get(2).cloned(),
            None,
        )
    };
    let bytes = bytes(ctx, value, encoding.as_deref())?;
    let offset =
        usize::try_from(offset).map_err(|_| Exception::throw_range(ctx, "offset is too large"))?;
    if offset > bytes.len() {
        return Err(Exception::throw_range(ctx, "offset is outside the buffer"));
    }
    let length = length
        .map(|length| {
            usize::try_from(length).map_err(|_| Exception::throw_range(ctx, "length is too large"))
        })
        .transpose()?
        .unwrap_or(bytes.len() - offset);
    if length > bytes.len() - offset {
        return Err(Exception::throw_range(ctx, "length is outside the buffer"));
    }
    Ok((
        bytes[offset..offset + length].to_vec(),
        position_value(ctx, position)?,
    ))
}

fn normalize_write_options<'js>(
    ctx: &Ctx<'js>,
    data_is_string: bool,
    args: Vec<Value<'js>>,
) -> rquickjs::Result<Vec<Value<'js>>> {
    let Some(first) = args.first() else {
        return Ok(args);
    };
    if !first.is_object() {
        return Ok(args);
    }
    let options = first
        .clone()
        .try_into_object()
        .map_err(|_| Exception::throw_type(ctx, "write options must be an object"))?;
    let value = |name: &str| -> rquickjs::Result<Value<'js>> {
        Ok(options
            .get::<_, Option<Value>>(name)?
            .unwrap_or_else(|| Value::new_undefined(ctx.clone())))
    };
    if data_is_string {
        Ok(vec![value("position")?, value("encoding")?])
    } else {
        Ok(vec![value("offset")?, value("length")?, value("position")?])
    }
}

struct WritableBytes<'js> {
    _value: Value<'js>,
    ptr: *mut u8,
    len: usize,
}

fn writable_bytes<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<WritableBytes<'js>> {
    if let Some(buffer) = ArrayBuffer::from_value(value.clone()) {
        let raw = buffer
            .as_raw()
            .ok_or_else(|| Exception::throw_type(ctx, "buffer is detached"))?;
        return Ok(WritableBytes {
            _value: value,
            ptr: raw.ptr.as_ptr(),
            len: raw.len,
        });
    }
    let object = value
        .clone()
        .try_into_object()
        .map_err(|_| Exception::throw_type(ctx, "buffer must be an ArrayBuffer or typed array"))?;
    macro_rules! typed_array {
        ($type:ty) => {
            if object.is_typed_array::<$type>() {
                let array = TypedArray::<$type>::from_object(object.clone())?;
                let raw = array
                    .as_raw()
                    .ok_or_else(|| Exception::throw_type(ctx, "buffer is detached"))?;
                return Ok(WritableBytes {
                    _value: value,
                    ptr: raw.ptr.as_ptr(),
                    len: raw.len,
                });
            }
        };
    }
    typed_array!(u8);
    typed_array!(i8);
    typed_array!(u16);
    typed_array!(i16);
    typed_array!(u32);
    typed_array!(i32);
    typed_array!(u64);
    typed_array!(i64);
    typed_array!(f32);
    typed_array!(f64);
    Err(Exception::throw_type(
        ctx,
        "buffer must be an ArrayBuffer or typed array",
    ))
}

impl WritableBytes<'_> {
    fn write(&self, offset: usize, bytes: &[u8]) {
        // QuickJS keeps the typed array alive for the duration of this native
        // call, so the raw view remains valid while we copy into it.
        unsafe {
            std::slice::from_raw_parts_mut(self.ptr.add(offset), bytes.len())
                .copy_from_slice(bytes);
        }
    }
}

fn illegal_constructor(ctx: Ctx<'_>) -> rquickjs::Result<()> {
    Err(Exception::throw_type(&ctx, "Illegal constructor"))
}

fn constructor<'js>(
    ctx: &Ctx<'js>,
    name: &str,
    prototype_name: &str,
) -> rquickjs::Result<(Function<'js>, Object<'js>)> {
    let function = Function::new(ctx.clone(), illegal_constructor)?;
    function.set_name(name)?;
    let function_object = Object::from_value(function.clone().into_value())?;
    let prototype = Object::new(ctx.clone())?;
    function_object.set("prototype", prototype.clone())?;
    prototype.set("constructor", function.clone())?;
    ctx.globals().set(prototype_name, prototype.clone())?;
    Ok((function, prototype))
}

fn dirent_constructor<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    let (function, prototype) = constructor(ctx, "Dirent", "__appd_node_fs_dirent_proto")?;
    for (name, method) in [
        ("isFile", dirent_is_file as fn(This<Object<'_>>) -> bool),
        ("isDirectory", dirent_is_directory),
        ("isBlockDevice", dirent_is_block_device),
        ("isCharacterDevice", dirent_is_character_device),
        ("isSymbolicLink", dirent_is_symbolic_link),
        ("isFIFO", dirent_is_fifo),
        ("isSocket", dirent_is_socket),
    ] {
        prototype.set(name, Function::new(ctx.clone(), method)?)?;
    }
    Ok(function)
}

fn dir_constructor<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    constructor(ctx, "Dir", "__appd_node_fs_dir_proto").map(|(function, _)| function)
}

#[derive(Clone)]
struct DirState {
    entries: Arc<Mutex<Option<Vec<DirItem>>>>,
    path: String,
    encoding: Option<String>,
}

#[derive(Clone)]
struct DirItem {
    name: String,
    parent: String,
    entry: DirectoryEntry,
}

#[allow(clippy::too_many_lines)]
fn dir_object<'js>(
    ctx: &Ctx<'js>,
    path: String,
    entries: Vec<DirItem>,
    encoding: Option<&str>,
) -> rquickjs::Result<Object<'js>> {
    let prototype: Option<Object> = ctx.globals().get("__appd_node_fs_dir_proto")?;
    let object = Object::new_proto(ctx.clone(), prototype.as_ref())?;
    let state = DirState {
        entries: Arc::new(Mutex::new(Some(entries))),
        path,
        encoding: encoding.map(ToOwned::to_owned),
    };
    object.set("path", state.path.clone())?;

    let read_state = state.clone();
    object.set(
        "readSync",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            dir_read_entry(&ctx, &read_state)
        })?,
    )?;
    let read_state = state.clone();
    object.set(
        "read",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, args: Rest<Value<'js>>| {
            let mut args = args.0;
            let callback = args
                .pop()
                .and_then(|value| Function::from_value(value).ok());
            let result = dir_read_entry(&ctx, &read_state);
            if let Some(callback) = callback {
                callback_values(
                    &ctx,
                    callback,
                    result.map(|value| vec![Value::new_null(ctx.clone()), value]),
                )?;
                Ok(Value::new_undefined(ctx))
            } else {
                promise(ctx.clone(), result).map(Promise::into_value)
            }
        })?,
    )?;
    let close_state = state.clone();
    object.set(
        "closeSync",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            dir_close(&ctx, &close_state)
        })?,
    )?;
    let close_state = state.clone();
    object.set(
        "close",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, args: Rest<Value<'js>>| {
            let callback = args
                .0
                .last()
                .cloned()
                .and_then(|value| Function::from_value(value).ok());
            let result = dir_close(&ctx, &close_state).map(|()| Value::new_undefined(ctx.clone()));
            if let Some(callback) = callback {
                callback_values(
                    &ctx,
                    callback,
                    result.map(|_| vec![Value::new_null(ctx.clone())]),
                )?;
                Ok(Value::new_undefined(ctx))
            } else {
                promise(ctx.clone(), result).map(Promise::into_value)
            }
        })?,
    )?;
    let entries_state = state.clone();
    object.set(
        "entries",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            dir_iterator(&ctx, entries_state.clone())
        })?,
    )?;
    let iterator_state = state;
    object.set(
        Symbol::async_iterator(ctx.clone()),
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            dir_iterator(&ctx, iterator_state.clone())
        })?,
    )?;
    Ok(object)
}

fn dir_read_entry<'js>(ctx: &Ctx<'js>, state: &DirState) -> rquickjs::Result<Value<'js>> {
    let item = {
        let mut entries = lock(&state.entries);
        let entries = entries
            .as_mut()
            .ok_or_else(|| Exception::throw_message(ctx, "ERR_DIR_CLOSED: directory is closed"))?;
        if entries.is_empty() {
            None
        } else {
            Some(entries.remove(0))
        }
    };
    if let Some(item) = item {
        dirent(
            ctx,
            &item.entry,
            name_value(ctx, &item.name, state.encoding.as_deref())?,
            normalized_parent(&item.parent),
        )
        .map(Object::into_value)
    } else {
        Ok(Value::new_null(ctx.clone()))
    }
}

fn dir_close(ctx: &Ctx<'_>, state: &DirState) -> rquickjs::Result<()> {
    let mut entries = lock(&state.entries);
    if entries.take().is_none() {
        return Err(Exception::throw_message(
            ctx,
            "ERR_DIR_CLOSED: directory is closed",
        ));
    }
    Ok(())
}

fn dir_iterator<'js>(ctx: &Ctx<'js>, state: DirState) -> rquickjs::Result<Object<'js>> {
    let iterator = Object::new(ctx.clone())?;
    iterator.set(
        "next",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            let value = dir_read_entry(&ctx, &state)?;
            let next = Object::new(ctx.clone())?;
            next.set("done", value.is_null())?;
            next.set("value", value)?;
            promise(ctx.clone(), Ok(next.into_value())).map(Promise::into_value)
        })?,
    )?;
    iterator.set(
        Symbol::async_iterator(ctx.clone()),
        Function::new(ctx.clone(), |this: This<Object<'js>>| {
            Ok::<Object<'js>, rquickjs::Error>(this.0)
        })?,
    )?;
    Ok(iterator)
}

#[derive(Clone)]
struct FileHandleState {
    descriptor: Arc<Mutex<Option<u32>>>,
    owns_descriptor: bool,
}

fn file_handle_constructor<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    constructor(ctx, "FileHandle", "__appd_node_fs_file_handle_proto").map(|(function, _)| function)
}

#[allow(clippy::too_many_lines)]
fn file_handle_object<'js>(
    ctx: &Ctx<'js>,
    state: FileHandleState,
) -> rquickjs::Result<Object<'js>> {
    let prototype: Option<Object> = ctx.globals().get("__appd_node_fs_file_handle_proto")?;
    let object = Object::new_proto(ctx.clone(), prototype.as_ref())?;
    install_event_emitter(ctx, &object)?;
    let descriptor = handle_descriptor(&state)
        .ok_or_else(|| Exception::throw_internal(ctx, "file descriptor was closed"))?;
    object.set("fd", descriptor)?;

    let read_state = state.clone();
    object.set(
        "read",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, args: Rest<Value<'js>>| {
            handle_read(&ctx, &read_state, args)
        })?,
    )?;
    let vector_read_state = state.clone();
    object.set(
        "readv",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, buffers: Array<'js>, position: Opt<Value<'js>>| {
                let descriptor = handle_descriptor(&vector_read_state).ok_or_else(|| {
                    Exception::throw_message(&ctx, "EBADF: file descriptor closed")
                })?;
                let bytes_read = readv_sync(ctx.clone(), descriptor, buffers.clone(), position)?;
                let result = Object::new(ctx.clone())?;
                result.set("bytesRead", bytes_read)?;
                result.set("buffers", buffers)?;
                promise(ctx.clone(), Ok(result.into_value()))
            },
        )?,
    )?;
    let read_file_state = state.clone();
    object.set(
        "readFile",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, options: Opt<Value<'js>>| {
                let descriptor = handle_descriptor(&read_file_state).ok_or_else(|| {
                    Exception::throw_message(&ctx, "EBADF: file descriptor closed")
                })?;
                let options = parse_options(&ctx, options.0)?;
                let bytes = read_descriptor(&ctx, descriptor)?;
                promise(
                    ctx.clone(),
                    output(ctx.clone(), &bytes, options.encoding.as_deref()),
                )
            },
        )?,
    )?;
    object.set(
        "readLines",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, _options: Opt<Value<'js>>| {
                Err::<(), rquickjs::Error>(Exception::throw_message(
                    &ctx,
                    "readLines is not implemented",
                ))
            },
        )?,
    )?;
    let write_state = state.clone();
    object.set(
        "write",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, args: Rest<Value<'js>>| {
            handle_write(&ctx, &write_state, args)
        })?,
    )?;
    let vector_write_state = state.clone();
    object.set(
        "writev",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, buffers: Array<'js>, position: Opt<Value<'js>>| {
                let descriptor = handle_descriptor(&vector_write_state).ok_or_else(|| {
                    Exception::throw_message(&ctx, "EBADF: file descriptor closed")
                })?;
                let position = position_value(&ctx, position.0)?;
                let mut values = Vec::with_capacity(buffers.len());
                for index in 0..buffers.len() {
                    values.push(bytes(&ctx, buffers.get(index)?, None)?);
                }
                let bytes_written =
                    vfs_call(&ctx, |vfs| vfs.writev(descriptor, &values, position))?;
                let result = Object::new(ctx.clone())?;
                result.set("bytesWritten", bytes_written)?;
                result.set("buffers", buffers)?;
                promise(ctx.clone(), Ok(result.into_value()))
            },
        )?,
    )?;
    let write_file_state = state.clone();
    object.set(
        "writeFile",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, data: Value<'js>, options: Opt<Value<'js>>| {
                let descriptor = handle_descriptor(&write_file_state).ok_or_else(|| {
                    Exception::throw_message(&ctx, "EBADF: file descriptor closed")
                })?;
                let options = parse_options(&ctx, options.0)?;
                let bytes = bytes(&ctx, data.clone(), options.encoding.as_deref())?;
                let bytes_written = write_descriptor(&ctx, descriptor, &bytes, false)?;
                let result = Object::new(ctx.clone())?;
                result.set("bytesWritten", bytes_written)?;
                result.set("buffer", write_buffer_value(&ctx, &data, &bytes)?)?;
                promise(ctx.clone(), Ok(result.into_value()))
            },
        )?,
    )?;
    let append_state = state.clone();
    object.set(
        "appendFile",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, data: Value<'js>, options: Opt<Value<'js>>| {
                let descriptor = handle_descriptor(&append_state).ok_or_else(|| {
                    Exception::throw_message(&ctx, "EBADF: file descriptor closed")
                })?;
                let options = parse_options(&ctx, options.0)?;
                let data = bytes(&ctx, data, options.encoding.as_deref())?;
                write_descriptor(&ctx, descriptor, &data, true)?;
                promise(ctx.clone(), Ok(Value::new_undefined(ctx)))
            },
        )?,
    )?;
    let stat_state = state.clone();
    object.set(
        "stat",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, options: Opt<Value<'js>>| {
                let descriptor = handle_descriptor(&stat_state).ok_or_else(|| {
                    Exception::throw_message(&ctx, "EBADF: file descriptor closed")
                })?;
                let options = parse_options(&ctx, options.0)?;
                let stat = vfs_call(&ctx, |vfs| vfs.fstat(descriptor))?;
                promise(
                    ctx.clone(),
                    stat_object(&ctx, &stat, options.bigint).map(Object::into_value),
                )
            },
        )?,
    )?;
    let truncate_state = state.clone();
    object.set(
        "truncate",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, length: Opt<u64>| {
            let descriptor = handle_descriptor(&truncate_state)
                .ok_or_else(|| Exception::throw_message(&ctx, "EBADF: file descriptor closed"))?;
            vfs_call(&ctx, |vfs| vfs.ftruncate(descriptor, length.0.unwrap_or(0)))?;
            promise(ctx.clone(), Ok(Value::new_undefined(ctx)))
        })?,
    )?;
    for (name, function) in [
        ("sync", file_handle_sync(ctx.clone(), state.clone())?),
        ("datasync", file_handle_sync(ctx.clone(), state.clone())?),
    ] {
        object.set(name, function)?;
    }
    let chmod_state = state.clone();
    object.set(
        "chmod",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, mode: Value<'js>| {
            let descriptor = handle_descriptor(&chmod_state)
                .ok_or_else(|| Exception::throw_message(&ctx, "EBADF: file descriptor closed"))?;
            fchmod_sync(ctx.clone(), descriptor, mode)?;
            promise(ctx.clone(), Ok(Value::new_undefined(ctx)))
        })?,
    )?;
    let chown_state = state.clone();
    object.set(
        "chown",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, uid: u32, gid: u32| {
            let descriptor = handle_descriptor(&chown_state)
                .ok_or_else(|| Exception::throw_message(&ctx, "EBADF: file descriptor closed"))?;
            fchown_sync(ctx.clone(), descriptor, uid, gid)?;
            promise(ctx.clone(), Ok(Value::new_undefined(ctx)))
        })?,
    )?;
    let utimes_state = state.clone();
    object.set(
        "utimes",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, atime: Value<'js>, mtime: Value<'js>| {
                let descriptor = handle_descriptor(&utimes_state).ok_or_else(|| {
                    Exception::throw_message(&ctx, "EBADF: file descriptor closed")
                })?;
                futimes_sync(ctx.clone(), descriptor, atime, mtime)?;
                promise(ctx.clone(), Ok(Value::new_undefined(ctx)))
            },
        )?,
    )?;
    let close_state = state.clone();
    object.set(
        "close",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, this: This<Object<'js>>| {
                let result = close_handle(&ctx, &close_state, &this.0)
                    .map(|()| Value::new_undefined(ctx.clone()));
                promise(ctx.clone(), result)
            },
        )?,
    )?;
    let stream_state = state.clone();
    object.set(
        "createReadStream",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, options: Opt<Value<'js>>| {
                let descriptor = handle_descriptor(&stream_state).ok_or_else(|| {
                    Exception::throw_message(&ctx, "EBADF: file descriptor closed")
                })?;
                create_read_stream(&ctx, None, Some(descriptor), options.0)
            },
        )?,
    )?;
    let stream_state = state.clone();
    object.set(
        "createWriteStream",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, options: Opt<Value<'js>>| {
                let descriptor = handle_descriptor(&stream_state).ok_or_else(|| {
                    Exception::throw_message(&ctx, "EBADF: file descriptor closed")
                })?;
                create_write_stream(&ctx, None, Some(descriptor), options.0)
            },
        )?,
    )?;
    let web_stream_state = state.clone();
    object.set(
        "readableWebStream",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, _options: Opt<Value<'js>>| {
                let descriptor = handle_descriptor(&web_stream_state).ok_or_else(|| {
                    Exception::throw_message(&ctx, "EBADF: file descriptor closed")
                })?;
                readable_web_stream(&ctx, descriptor)
            },
        )?,
    )?;
    Ok(object)
}

fn handle_descriptor(state: &FileHandleState) -> Option<u32> {
    lock(&state.descriptor).as_ref().copied()
}

fn take_descriptor(state: &FileHandleState) -> Option<u32> {
    lock(&state.descriptor).take()
}

fn close_handle<'js>(
    ctx: &Ctx<'js>,
    state: &FileHandleState,
    object: &Object<'js>,
) -> rquickjs::Result<()> {
    let descriptor = take_descriptor(state);
    if let Some(descriptor) = descriptor {
        vfs_call(ctx, |vfs| vfs.close(descriptor))?;
        object.set("fd", Value::new_undefined(ctx.clone()))?;
        if let Ok(Some(emit)) = object.get::<_, Option<Function>>("emit") {
            emit.call::<_, bool>(("close",))?;
        }
    }
    Ok(())
}

fn install_event_emitter<'js>(ctx: &Ctx<'js>, object: &Object<'js>) -> rquickjs::Result<()> {
    let process: Option<Object> = ctx.globals().get("process")?;
    let Some(process) = process else {
        return Ok(());
    };
    let Ok(get_builtin) = process.get::<_, Function>("getBuiltinModule") else {
        return Ok(());
    };
    let Ok(Some(module)) = get_builtin.call::<_, Option<Object>>(("node:events",)) else {
        return Ok(());
    };
    let Ok(Some(constructor)) = module.get::<_, Option<Constructor>>("EventEmitter") else {
        return Ok(());
    };
    let Ok(Some(prototype)) = constructor.get::<_, Option<Object>>("prototype") else {
        return Ok(());
    };
    object.set("__events", ctx.eval::<Object, _>("new Map()")?)?;
    for name in [
        "on",
        "addEventListener",
        "addListener",
        "once",
        "off",
        "removeListener",
        "removeEventListener",
        "removeAllListeners",
        "emit",
        "listeners",
        "listenerCount",
        "eventNames",
        "setMaxListeners",
        "getMaxListeners",
        "prependListener",
        "prependOnceListener",
    ] {
        if let Some(method) = prototype.get::<_, Option<Function>>(name)? {
            object.set(name, method)?;
        }
    }
    Ok(())
}

fn file_handle_sync<'js>(ctx: Ctx<'js>, state: FileHandleState) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx, move |ctx: Ctx<'js>| {
        let descriptor = handle_descriptor(&state)
            .ok_or_else(|| Exception::throw_message(&ctx, "EBADF: file descriptor closed"))?;
        vfs_call(&ctx, |vfs| vfs.fstat(descriptor))?;
        promise(ctx.clone(), Ok(Value::new_undefined(ctx)))
    })
}

fn handle_read<'js>(
    ctx: &Ctx<'js>,
    state: &FileHandleState,
    args: Rest<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    let descriptor = handle_descriptor(state)
        .ok_or_else(|| Exception::throw_message(ctx, "EBADF: file descriptor closed"))?;
    let ReadArguments {
        buffer,
        offset: offset_value,
        length: length_value,
        position: position_arg,
    } = normalize_read_arguments(ctx, args.0)?;
    let writable = writable_bytes(ctx, buffer.clone())?;
    let offset = offset_value
        .map(|value| Coerced::<u64>::from_js(ctx, value))
        .transpose()?
        .map_or(0, |value| usize::try_from(*value).unwrap_or(usize::MAX));
    let length = length_value
        .map(|value| Coerced::<u64>::from_js(ctx, value))
        .transpose()?
        .map_or(writable.len.saturating_sub(offset), |value| {
            usize::try_from(*value).unwrap_or(usize::MAX)
        });
    let position = position_value(ctx, position_arg)?;
    if offset > writable.len || length > writable.len - offset {
        return Err(Exception::throw_range(ctx, "read buffer range is invalid"));
    }
    let bytes = vfs_call(ctx, |vfs| vfs.read(descriptor, length, position))?;
    writable.write(offset, &bytes);
    let result = Object::new(ctx.clone())?;
    result.set("bytesRead", bytes.len())?;
    result.set("buffer", buffer)?;
    promise(ctx.clone(), Ok(result.into_value()))
}

fn handle_write<'js>(
    ctx: &Ctx<'js>,
    state: &FileHandleState,
    args: Rest<Value<'js>>,
) -> rquickjs::Result<Promise<'js>> {
    let descriptor = handle_descriptor(state)
        .ok_or_else(|| Exception::throw_message(ctx, "EBADF: file descriptor closed"))?;
    let mut args = args.0;
    let value = args
        .first()
        .cloned()
        .ok_or_else(|| Exception::throw_type(ctx, "data is required"))?;
    args.remove(0);
    let args = normalize_write_options(ctx, value.is_string(), args)?;
    let (data, position) = write_arguments(ctx, value.clone(), args)?;
    let bytes_written = vfs_call(ctx, |vfs| vfs.write(descriptor, &data, position))?;
    let result = Object::new(ctx.clone())?;
    result.set("bytesWritten", bytes_written)?;
    result.set("buffer", write_buffer_value(ctx, &value, &data)?)?;
    promise(ctx.clone(), Ok(result.into_value()))
}

fn create_read_stream_export<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    create_read_stream(&ctx, Some(input), None, options.0)
}

fn create_write_stream_export<'js>(
    ctx: Ctx<'js>,
    input: Value<'js>,
    options: Opt<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    create_write_stream(&ctx, Some(input), None, options.0)
}

fn create_read_stream<'js>(
    ctx: &Ctx<'js>,
    input: Option<Value<'js>>,
    descriptor: Option<u32>,
    options: Option<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    let path = input
        .as_ref()
        .map(|value| path(ctx, value.clone()))
        .transpose()?;
    let option_descriptor = option_descriptor(ctx, options.as_ref())?;
    let owns_descriptor = descriptor.is_none() && option_descriptor.is_none();
    let descriptor = if let Some(descriptor) = descriptor.or(option_descriptor) {
        descriptor
    } else {
        let path = path
            .as_deref()
            .ok_or_else(|| Exception::throw_type(ctx, "stream path is required"))?;
        let flags = option_property(ctx, options.as_ref(), "flags")?;
        let options = open_options(ctx, flags)?;
        vfs_call(ctx, |vfs| vfs.open(path, options))?
    };
    let bytes = read_descriptor(ctx, descriptor)?;
    let stream = stream_object(ctx, "Readable")?;
    stream.set("path", path.clone().unwrap_or_default())?;
    stream.set("fd", descriptor)?;
    stream.set("bytesRead", bytes.len())?;
    let push: Function = ctx.eval("(stream, chunk) => stream.push(chunk)")?;
    push.call::<_, ()>((
        stream.clone(),
        TypedArray::<u8>::new_copy(ctx.clone(), &bytes)?,
    ))?;
    push.call::<_, ()>((stream.clone(), Value::new_null(ctx.clone())))?;
    if owns_descriptor {
        let _ = vfs_call(ctx, |vfs| vfs.close(descriptor));
        stream.set("fd", Value::new_null(ctx.clone()))?;
    }
    Ok(stream)
}

fn create_write_stream<'js>(
    ctx: &Ctx<'js>,
    input: Option<Value<'js>>,
    descriptor: Option<u32>,
    options: Option<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    let path = input
        .as_ref()
        .map(|value| path(ctx, value.clone()))
        .transpose()?;
    let option_descriptor = option_descriptor(ctx, options.as_ref())?;
    let owns_descriptor = descriptor.is_none() && option_descriptor.is_none();
    let descriptor = if let Some(descriptor) = descriptor.or(option_descriptor) {
        descriptor
    } else {
        let path = path
            .as_deref()
            .ok_or_else(|| Exception::throw_type(ctx, "stream path is required"))?;
        let flags = option_property(ctx, options.as_ref(), "flags")?;
        let flags = flags.or_else(|| "w".into_js(ctx).ok());
        let options = open_options(ctx, flags)?;
        vfs_call(ctx, |vfs| vfs.open(path, options))?
    };
    let stream = stream_object(ctx, "Writable")?;
    stream.set("path", path.clone().unwrap_or_default())?;
    stream.set("fd", descriptor)?;
    let state = FileHandleState {
        descriptor: Arc::new(Mutex::new(Some(descriptor))),
        owns_descriptor,
    };
    let write_state = state.clone();
    stream.set(
        "write",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, args: Rest<Value<'js>>| {
            stream_write(&ctx, &write_state, args)
        })?,
    )?;
    let end_state = state.clone();
    stream.set(
        "end",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, this: This<Object<'js>>, args: Rest<Value<'js>>| {
                stream_end(&ctx, &end_state, this.0, args)
            },
        )?,
    )?;
    let destroy_state = state;
    stream.set(
        "destroy",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, this: This<Object<'js>>, _error: Opt<Value<'js>>| {
                if destroy_state.owns_descriptor
                    && let Some(descriptor) = take_descriptor(&destroy_state)
                {
                    let _ = vfs_call(&ctx, |vfs| vfs.close(descriptor));
                }
                Ok::<Object<'js>, rquickjs::Error>(this.0)
            },
        )?,
    )?;
    Ok(stream)
}

fn option_descriptor<'js>(
    ctx: &Ctx<'js>,
    options: Option<&Value<'js>>,
) -> rquickjs::Result<Option<u32>> {
    let Some(value) = option_property(ctx, options, "fd")? else {
        return Ok(None);
    };
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    if value.is_object() {
        let object = value
            .try_into_object()
            .map_err(|_| Exception::throw_type(ctx, "options.fd must be a file descriptor"))?;
        return object
            .get::<_, Option<Value>>("fd")?
            .map(|value| descriptor_value(ctx, value))
            .transpose();
    }
    descriptor_value(ctx, value).map(Some)
}

fn stream_object<'js>(ctx: &Ctx<'js>, kind: &str) -> rquickjs::Result<Object<'js>> {
    let process: Option<Object> = ctx.globals().get("process")?;
    if let Some(process) = process
        && let Ok(get_builtin) = process.get::<_, Function>("getBuiltinModule")
        && let Ok(module) = get_builtin.call::<_, Option<Object>>(("node:stream",))
        && let Some(module) = module
        && let Ok(constructor) = module.get::<_, Constructor>(kind)
    {
        return constructor.construct(());
    }
    Object::new(ctx.clone())
}

fn stream_write<'js>(
    ctx: &Ctx<'js>,
    state: &FileHandleState,
    args: Rest<Value<'js>>,
) -> rquickjs::Result<bool> {
    let mut args = args.0;
    let value = args
        .first()
        .cloned()
        .ok_or_else(|| Exception::throw_type(ctx, "chunk is required"))?;
    args.remove(0);
    let callback = args
        .last()
        .cloned()
        .and_then(|value| Function::from_value(value).ok());
    if callback.is_some() {
        args.pop();
    }
    let (data, position) = if value.is_string() {
        let encoding = args
            .first()
            .and_then(|value| value.as_string())
            .map(rquickjs::String::to_string)
            .transpose()?;
        (bytes(ctx, value, encoding.as_deref())?, None)
    } else {
        let args = normalize_write_options(ctx, false, args)?;
        write_arguments(ctx, value, args)?
    };
    let descriptor = handle_descriptor(state)
        .ok_or_else(|| Exception::throw_message(ctx, "EBADF: file descriptor closed"))?;
    let result = vfs_call(ctx, |vfs| vfs.write(descriptor, &data, position));
    if let Some(callback) = callback {
        match result {
            Ok(_) => callback.call::<_, ()>((Value::new_null(ctx.clone()),))?,
            Err(_error) if ctx.has_exception() => callback.call::<_, ()>((ctx.catch(),))?,
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn stream_end<'js>(
    ctx: &Ctx<'js>,
    state: &FileHandleState,
    stream: Object<'js>,
    args: Rest<Value<'js>>,
) -> rquickjs::Result<Object<'js>> {
    let mut args = args.0;
    let callback = args
        .last()
        .cloned()
        .and_then(|value| Function::from_value(value).ok());
    if callback.is_some() {
        args.pop();
    }
    if let Some(value) = args.first().cloned() {
        stream_write(ctx, state, Rest(vec![value]))?;
    }
    if state.owns_descriptor
        && let Some(descriptor) = take_descriptor(state)
    {
        vfs_call(ctx, |vfs| vfs.close(descriptor))?;
    }
    if let Some(callback) = callback {
        callback.call::<_, ()>((Value::new_null(ctx.clone()),))?;
    }
    Ok(stream)
}

fn readable_web_stream<'js>(ctx: &Ctx<'js>, descriptor: u32) -> rquickjs::Result<Object<'js>> {
    let bytes = read_descriptor(ctx, descriptor)?;
    let constructor: Constructor = ctx
        .globals()
        .get("ReadableStream")
        .map_err(|_| Exception::throw_internal(ctx, "ReadableStream is not installed"))?;
    let source = Object::new(ctx.clone())?;
    source.set(
        "start",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, controller: Object<'js>| {
                let enqueue: Function = controller.get("enqueue")?;
                enqueue.call::<_, ()>((TypedArray::<u8>::new_copy(ctx.clone(), &bytes)?,))?;
                let close: Function = controller.get("close")?;
                close.call::<_, ()>(())?;
                Ok::<(), rquickjs::Error>(())
            },
        )?,
    )?;
    constructor.construct((source,))
}

fn stats_constructor<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    let (function, prototype) = constructor(ctx, "Stats", "__appd_node_fs_stats_proto")?;
    for (name, method) in [
        ("isFile", stats_is_file as fn(This<Object<'_>>) -> bool),
        ("isDirectory", stats_is_directory),
        ("isBlockDevice", stats_is_block_device),
        ("isCharacterDevice", stats_is_character_device),
        ("isSymbolicLink", stats_is_symbolic_link),
        ("isFIFO", stats_is_fifo),
        ("isSocket", stats_is_socket),
    ] {
        prototype.set(name, Function::new(ctx.clone(), method)?)?;
    }
    Ok(function)
}

fn stats_mode(this: &This<Object<'_>>) -> Option<u32> {
    let mode: Value = this.0.get("mode").ok()?;
    if mode.is_big_int() {
        let bigint = BigInt::from_value(mode).ok()?;
        return u32::try_from(bigint.to_i64().ok()?).ok();
    }
    Coerced::<i64>::from_js(mode.clone().ctx(), mode)
        .ok()
        .and_then(|value| u32::try_from(*value).ok())
}

fn stats_type(this: &This<Object<'_>>) -> Option<u32> {
    Some(stats_mode(this)? & 0o170_000)
}

fn stats_is_file(this: This<Object<'_>>) -> bool {
    stats_type(&this).is_some_and(|mode| mode == 0o100_000)
}

fn stats_is_directory(this: This<Object<'_>>) -> bool {
    stats_type(&this).is_some_and(|mode| mode == 0o040_000)
}

fn stats_is_block_device(this: This<Object<'_>>) -> bool {
    stats_type(&this).is_some_and(|mode| mode == 0o060_000)
}

fn stats_is_character_device(this: This<Object<'_>>) -> bool {
    stats_type(&this).is_some_and(|mode| mode == 0o020_000)
}

fn stats_is_symbolic_link(this: This<Object<'_>>) -> bool {
    stats_type(&this).is_some_and(|mode| mode == 0o120_000)
}

fn stats_is_fifo(this: This<Object<'_>>) -> bool {
    stats_type(&this).is_some_and(|mode| mode == 0o010_000)
}

fn stats_is_socket(this: This<Object<'_>>) -> bool {
    stats_type(&this).is_some_and(|mode| mode == 0o140_000)
}

fn dirent_is_file(this: This<Object<'_>>) -> bool {
    this.0
        .get::<_, Option<Function>>("isFile")
        .is_ok_and(|function| function.is_some())
        && this
            .0
            .get::<_, Option<bool>>("device")
            .unwrap_or(Some(false))
            == Some(false)
        && this
            .0
            .get::<_, Option<String>>("type")
            .unwrap_or(None)
            .is_some_and(|kind| kind == "file")
}

fn dirent_is_directory(this: This<Object<'_>>) -> bool {
    this.0
        .get::<_, Option<String>>("type")
        .unwrap_or(None)
        .is_some_and(|kind| kind == "directory")
}

fn dirent_is_block_device(_: This<Object<'_>>) -> bool {
    false
}

fn dirent_is_character_device(this: This<Object<'_>>) -> bool {
    this.0
        .get::<_, Option<bool>>("device")
        .unwrap_or(Some(false))
        == Some(true)
}

fn dirent_is_symbolic_link(this: This<Object<'_>>) -> bool {
    this.0
        .get::<_, Option<String>>("type")
        .unwrap_or(None)
        .is_some_and(|kind| kind == "symlink")
}

fn dirent_is_fifo(_: This<Object<'_>>) -> bool {
    false
}

fn dirent_is_socket(_: This<Object<'_>>) -> bool {
    false
}

fn stat_object<'js>(ctx: &Ctx<'js>, stat: &Stat, bigint: bool) -> rquickjs::Result<Object<'js>> {
    let prototype: Option<Object> = ctx.globals().get("__appd_node_fs_stats_proto")?;
    let object = Object::new_proto(ctx.clone(), prototype.as_ref())?;
    let is_file = stat.kind == appd_vfs::NodeType::File && !stat.device;
    let is_directory = stat.kind == appd_vfs::NodeType::Directory;
    let mode: u32 = if stat.device {
        0o020_666
    } else if is_file {
        if stat.writable { 0o100_666 } else { 0o100_444 }
    } else if is_directory {
        if stat.writable { 0o40_777 } else { 0o40_555 }
    } else {
        0o120_777
    };
    object.set("type", node_type_name(stat.kind))?;
    set_number_or_bigint(ctx, &object, "dev", u64::from(stat.device), bigint)?;
    set_number_or_bigint(ctx, &object, "ino", 0, bigint)?;
    set_number_or_bigint(ctx, &object, "mode", u64::from(mode), bigint)?;
    set_number_or_bigint(ctx, &object, "nlink", 1, bigint)?;
    set_number_or_bigint(ctx, &object, "uid", 0, bigint)?;
    set_number_or_bigint(ctx, &object, "gid", 0, bigint)?;
    set_number_or_bigint(ctx, &object, "rdev", 0, bigint)?;
    set_number_or_bigint(ctx, &object, "size", stat.size, bigint)?;
    set_number_or_bigint(ctx, &object, "blksize", 0, bigint)?;
    set_number_or_bigint(ctx, &object, "blocks", 0, bigint)?;
    object.set("writable", stat.writable)?;
    object.set("device", stat.device)?;
    set_number_or_bigint(ctx, &object, "atimeMs", 0, bigint)?;
    set_number_or_bigint(ctx, &object, "mtimeMs", 0, bigint)?;
    set_number_or_bigint(ctx, &object, "ctimeMs", 0, bigint)?;
    set_number_or_bigint(ctx, &object, "birthtimeMs", 0, bigint)?;
    if bigint {
        object.set("atimeNs", BigInt::from_i64(ctx.clone(), 0)?)?;
        object.set("mtimeNs", BigInt::from_i64(ctx.clone(), 0)?)?;
        object.set("ctimeNs", BigInt::from_i64(ctx.clone(), 0)?)?;
        object.set("birthtimeNs", BigInt::from_i64(ctx.clone(), 0)?)?;
    }
    let date: Object = ctx.eval("new Date(0)")?;
    object.set("atime", date.clone())?;
    object.set("mtime", date.clone())?;
    object.set("ctime", date.clone())?;
    object.set("birthtime", date)?;
    Ok(object)
}

fn set_number_or_bigint<'js>(
    ctx: &Ctx<'js>,
    object: &Object<'js>,
    name: &str,
    value: u64,
    bigint: bool,
) -> rquickjs::Result<()> {
    if bigint {
        object.set(name, BigInt::from_u64(ctx.clone(), value)?)
    } else {
        object.set(name, value)
    }
}

fn name_value<'js>(
    ctx: &Ctx<'js>,
    name: &str,
    encoding: Option<&str>,
) -> rquickjs::Result<Value<'js>> {
    match encoding.map(str::to_ascii_lowercase).as_deref() {
        None | Some("utf8" | "utf-8") => name.to_owned().into_js(ctx),
        Some("buffer") => {
            Ok(TypedArray::<u8>::new_copy(ctx.clone(), name.as_bytes())?.into_value())
        }
        Some(_) => output(ctx.clone(), name.as_bytes(), encoding),
    }
}

fn text_value<'js>(
    ctx: &Ctx<'js>,
    value: &str,
    encoding: Option<&str>,
) -> rquickjs::Result<Value<'js>> {
    match encoding.map(str::to_ascii_lowercase).as_deref() {
        None | Some("utf8" | "utf-8") => value.to_owned().into_js(ctx),
        Some(_) => output(ctx.clone(), value.as_bytes(), encoding),
    }
}

fn normalized_parent(path: &str) -> &str {
    if path.is_empty() { "/" } else { path }
}

fn validate_mode<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> rquickjs::Result<()> {
    if value.is_number() {
        let mode: Coerced<i64> = Coerced::from_js(ctx, value)?;
        if (0..=i64::from(u32::MAX)).contains(&*mode) {
            return Ok(());
        }
    } else if let Some(string) = value.as_string()
        && u32::from_str_radix(&string.to_string()?, 8).is_ok()
    {
        return Ok(());
    }
    Err(Exception::throw_type(ctx, "invalid file mode"))
}

fn statfs_object<'js>(ctx: &Ctx<'js>, bigint: bool) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    for name in [
        "type", "bsize", "blocks", "bfree", "bavail", "files", "ffree",
    ] {
        set_number_or_bigint(ctx, &object, name, 0, bigint)?;
    }
    Ok(object)
}

fn glob_matches(
    ctx: &Ctx<'_>,
    cwd: &str,
    pattern: &str,
    _options: &FsOptions,
) -> rquickjs::Result<Vec<(String, DirectoryEntry)>> {
    let entries = vfs_call(ctx, |vfs| vfs.walk(cwd))?;
    let mut result = Vec::new();
    for (path, entry) in entries {
        let candidate = if pattern.starts_with('/') {
            path.clone()
        } else {
            path.strip_prefix(cwd)
                .unwrap_or(&path)
                .trim_start_matches('/')
                .to_owned()
        };
        if expand_braces(pattern)
            .iter()
            .any(|expanded| glob_match(expanded, &candidate))
        {
            result.push((path, entry));
        }
    }
    result.sort_by(|left, right| left.0.cmp(&right.0));
    result.dedup_by(|left, right| left.0 == right.0);
    Ok(result)
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(start) = pattern.find('{') else {
        return vec![pattern.to_owned()];
    };
    let Some(end) = pattern[start + 1..].find('}') else {
        return vec![pattern.to_owned()];
    };
    let end = start + 1 + end;
    let prefix = &pattern[..start];
    let suffix = &pattern[end + 1..];
    pattern[start + 1..end]
        .split(',')
        .flat_map(|part| expand_braces(&format!("{prefix}{part}{suffix}")))
        .collect()
}

fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').filter(|part| !part.is_empty()).collect();
    let path: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    glob_segments(&pattern, &path)
}

fn glob_segments(pattern: &[&str], path: &[&str]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    if pattern[0] == "**" {
        return glob_segments(&pattern[1..], path)
            || (!path.is_empty() && glob_segments(pattern, &path[1..]));
    }
    !path.is_empty()
        && segment_match(pattern[0], path[0])
        && glob_segments(&pattern[1..], &path[1..])
}

fn segment_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut states = vec![false; value.len() + 1];
    states[0] = true;
    for character in pattern {
        let mut next = vec![false; value.len() + 1];
        if *character == b'*' {
            next[0] = states[0];
            for index in 1..=value.len() {
                next[index] = states[index] || next[index - 1];
            }
        } else {
            for index in 1..=value.len() {
                next[index] =
                    states[index - 1] && (*character == b'?' || *character == value[index - 1]);
            }
        }
        states = next;
    }
    states[value.len()]
}

fn blob_object<'js>(
    ctx: &Ctx<'js>,
    bytes: Vec<u8>,
    mime_type: &str,
) -> rquickjs::Result<Object<'js>> {
    if let Some(constructor) = ctx.globals().get::<_, Option<Constructor>>("Blob")? {
        let parts = Array::new(ctx.clone())?;
        parts.set(0, TypedArray::<u8>::new_copy(ctx.clone(), &bytes)?)?;
        let options = Object::new(ctx.clone())?;
        options.set("type", mime_type)?;
        return constructor.construct((parts, options));
    }
    let object = Object::new(ctx.clone())?;
    object.set("size", bytes.len())?;
    object.set("type", mime_type)?;
    let text_bytes = bytes.clone();
    object.set(
        "text",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            promise(
                ctx.clone(),
                String::from_utf8_lossy(&text_bytes)
                    .into_owned()
                    .into_js(&ctx),
            )
        })?,
    )?;
    object.set(
        "arrayBuffer",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>| {
            promise(
                ctx.clone(),
                ArrayBuffer::new_copy(ctx.clone(), &bytes).map(ArrayBuffer::into_value),
            )
        })?,
    )?;
    Ok(object)
}

const fn node_type_name(kind: appd_vfs::NodeType) -> &'static str {
    match kind {
        appd_vfs::NodeType::File => "file",
        appd_vfs::NodeType::Directory => "directory",
        appd_vfs::NodeType::Symlink => "symlink",
    }
}

fn dirent<'js>(
    ctx: &Ctx<'js>,
    entry: &DirectoryEntry,
    name: Value<'js>,
    parent_path: &str,
) -> rquickjs::Result<Object<'js>> {
    let prototype: Option<Object> = ctx.globals().get("__appd_node_fs_dirent_proto")?;
    let object = Object::new_proto(ctx.clone(), prototype.as_ref())?;
    object.set("name", name)?;
    object.set("parentPath", parent_path)?;
    object.set("type", node_type_name(entry.kind))?;
    object.set("device", entry.device)?;
    set_type_methods(ctx, &object, entry.kind, entry.device)?;
    Ok(object)
}

fn set_type_methods<'js>(
    ctx: &Ctx<'js>,
    object: &Object<'js>,
    kind: appd_vfs::NodeType,
    device: bool,
) -> rquickjs::Result<()> {
    let is_file = kind == appd_vfs::NodeType::File && !device;
    let is_directory = kind == appd_vfs::NodeType::Directory;
    let is_symlink = kind == appd_vfs::NodeType::Symlink;
    object.set("isFile", Function::new(ctx.clone(), move || is_file)?)?;
    object.set(
        "isDirectory",
        Function::new(ctx.clone(), move || is_directory)?,
    )?;
    object.set(
        "isSymbolicLink",
        Function::new(ctx.clone(), move || is_symlink)?,
    )?;
    object.set(
        "isCharacterDevice",
        Function::new(ctx.clone(), move || device)?,
    )?;
    object.set("isBlockDevice", Function::new(ctx.clone(), || false)?)?;
    object.set("isFIFO", Function::new(ctx.clone(), || false)?)?;
    object.set("isSocket", Function::new(ctx.clone(), || false)?)?;
    Ok(())
}

fn vfs_handle(ctx: &Ctx<'_>) -> rquickjs::Result<VfsHandle> {
    ctx.userdata::<VfsUserData>()
        .map(|handle| Arc::clone(&handle.0))
        .ok_or_else(|| Exception::throw_internal(ctx, "appd VFS is not installed"))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn vfs_call<T>(
    ctx: &Ctx<'_>,
    operation: impl FnOnce(&mut VirtualFileSystem) -> appd_vfs::Result<T>,
) -> rquickjs::Result<T> {
    let vfs = vfs_handle(ctx)?;
    match operation(&mut lock(&vfs)) {
        Ok(value) => Ok(value),
        Err(error) => Err(vfs_exception(ctx, &error)?.throw()),
    }
}

fn vfs_exception<'js>(ctx: &Ctx<'js>, error: &VfsError) -> rquickjs::Result<Exception<'js>> {
    let message = error.to_string();
    let exception = Exception::from_message(ctx.clone(), &message)?;
    exception.as_object().set("code", error.code())?;
    if !error.path().is_empty() {
        exception.as_object().set("path", error.path())?;
    }
    Ok(exception)
}

fn decode_hex(ctx: &Ctx<'_>, value: &str) -> rquickjs::Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(Exception::throw_type(ctx, "invalid hex string"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Some((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| Exception::throw_type(ctx, "invalid hex string"))
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn decode_base64(value: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    for character in value.bytes() {
        let Some(digit) = base64_digit(character) else {
            continue;
        };
        buffer = (buffer << 6) | u32::from(digit);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push(u8::try_from((buffer >> bits) & 0xff).unwrap_or_default());
        }
    }
    bytes
}

fn base64_digit(value: u8) -> Option<u8> {
    match value {
        b'A'..=b'Z' => Some(value - b'A'),
        b'a'..=b'z' => Some(value - b'a' + 26),
        b'0'..=b'9' => Some(value - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        result.push(ALPHABET[(first >> 2) as usize] as char);
        result.push(ALPHABET[((first & 3) << 4 | second >> 4) as usize] as char);
        result.push(if chunk.len() > 1 {
            ALPHABET[((second & 15) << 2 | third >> 6) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            ALPHABET[(third & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{MODULE_NAME, NodeFsModule, NodeFsPromisesModule, PROMISES_MODULE_NAME, install};
    use appd_vfs::{Bundle, VirtualFileSystem};
    use rquickjs::loader::{BuiltinResolver, ModuleLoader};
    use rquickjs::{Context, Module, Object, Promise, Runtime};
    use std::sync::{Arc, Mutex};

    #[test]
    #[allow(clippy::too_many_lines)]
    fn exposes_the_native_node_fs_surface() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        std::fs::create_dir_all(directory.path().join("config"))?;
        std::fs::write(
            directory.path().join("config/app.json"),
            br#"{"enabled":true}"#,
        )?;
        let runtime = Runtime::new()?;
        runtime.set_loader(
            BuiltinResolver::default()
                .with_module(MODULE_NAME)
                .with_module(PROMISES_MODULE_NAME),
            ModuleLoader::default()
                .with_module(MODULE_NAME, NodeFsModule)
                .with_module(PROMISES_MODULE_NAME, NodeFsPromisesModule),
        );
        let context = Context::full(&runtime)?;
        context.with(|ctx| -> Result<(), Box<dyn std::error::Error>> {
            let vfs = Arc::new(Mutex::new(VirtualFileSystem::new(Bundle::new(
                directory.path(),
            ))));
            install(&ctx, &vfs)?;
            let namespace: Object = Module::import(&ctx, MODULE_NAME)?.finish()?;
            let default: Object = namespace.get("default")?;
            assert!(default.get::<_, rquickjs::Function>("readFileSync").is_ok());
            assert!(namespace.get::<_, Object>("promises").is_ok());
            let promises_namespace: Object =
                Module::import(&ctx, PROMISES_MODULE_NAME)?.finish()?;
            assert!(promises_namespace
                .get::<_, rquickjs::Function>("readFile")
                .is_ok());
            assert!(promises_namespace.get::<_, Object>("default").is_ok());
            let result: String = ctx.eval(
                r#"(() => {
                  const fs = process.getBuiltinModule("node:fs");
                  fs.mkdirSync("/tmp/data", { recursive: true });
                  fs.writeFileSync("/tmp/data/value.txt", "hello");
                  fs.appendFileSync("/tmp/data/value.txt", " world");
                  const fd = fs.openSync("/tmp/data/value.bin", "w+");
                  fs.writeSync(fd, Uint8Array.from([1, 2, 3]));
                  const bytes = new Uint8Array(3);
                  const count = fs.readSync(fd, bytes, 0, bytes.length, 0);
                  fs.closeSync(fd);
                  const entries = fs.readdirSync("/tmp/data", { withFileTypes: true });
                  fs.symlinkSync("/tmp/data/value.txt", "/tmp/data/link.txt");
                  const linkTarget = fs.readlinkSync("/tmp/data/link.txt");
                  const linkPath = fs.realpathSync("/tmp/data/link.txt");
                  fs.copyFileSync("/tmp/data/value.txt", "/tmp/data/copy.txt");
                  fs.renameSync("/tmp/data/copy.txt", "/tmp/data/renamed.txt");
                  const renamed = fs.readFileSync("/tmp/data/renamed.txt", "utf8");
                  fs.unlinkSync("/tmp/data/renamed.txt");
                  const append = fs.openSync("/tmp/data/value.txt", "a");
                  let appendReadCode;
                  try { fs.readSync(append, new Uint8Array(1), 0, 1, 0); }
                  catch (error) { appendReadCode = error.code; }
                  fs.closeSync(append);
                  const zero = fs.openSync("/dev/zero", "r");
                  const zeros = new Uint8Array(4);
                  fs.readSync(zero, zeros, 0, zeros.length, 0);
                  fs.closeSync(zero);
                  let readonlyCode;
                  const bundle = fs.openSync("/bundle/config/app.json", "r");
                  try { fs.ftruncateSync(bundle, 1); } catch (error) { readonlyCode = error.code; }
                  fs.closeSync(bundle);
                  return JSON.stringify({
                    text: fs.readFileSync("/tmp/data/value.txt", "utf8"),
                    count,
                    bytes: [...bytes],
                    file: fs.statSync("/tmp/data/value.txt").isFile(),
                    entry: entries[0].isFile(),
                    linkTarget,
                    linkPath,
                    renamed,
                    appendReadCode,
                    zeros: [...zeros],
                    readonlyCode,
                    bundle: fs.readFileSync("/bundle/config/app.json", "utf8"),
                  });
                })()"#,
            )?;
            assert_eq!(
                result,
                r#"{"text":"hello world","count":3,"bytes":[1,2,3],"file":true,"entry":true,"linkTarget":"/tmp/data/value.txt","linkPath":"/tmp/data/value.txt","renamed":"hello world","appendReadCode":"EPERM","zeros":[0,0,0,0],"readonlyCode":"EPERM","bundle":"{\"enabled\":true}"}"#
            );

            let promise: Promise = ctx.eval(
                r#"(async () => {
                  const fs = process.getBuiltinModule("node:fs");
                  let callbackText;
                  await new Promise((resolve, reject) => fs.writeFile(
                    "/tmp/callback.txt",
                    "callback",
                    error => error ? reject(error) : resolve(),
                  ));
                  await new Promise((resolve, reject) => fs.readFile(
                    "/tmp/callback.txt",
                    "utf8",
                    (error, value) => error ? reject(error) : (callbackText = value, resolve()),
                  ));
                  await fs.promises.writeFile("/tmp/promise.txt", "done");
                  const promiseText = await fs.promises.readFile("/tmp/promise.txt", "utf8");
                  const imported = await import("node:fs/promises");
                  await imported.writeFile("/tmp/imported.txt", "imported");
                  const importedText = await imported.readFile("/tmp/imported.txt", "utf8");
                  return JSON.stringify({ callbackText, promiseText, importedText });
                })()"#,
            )?;
            assert_eq!(
                promise.finish::<String>()?,
                r#"{"callbackText":"callback","promiseText":"done","importedText":"imported"}"#
            );
            let realpath_shape: String = ctx.eval(
                r#"(() => { const fs = process.getBuiltinModule("node:fs"); return `${typeof fs.realpath}:${typeof fs.realpath?.native}`; })()"#,
            )?;
            assert_eq!(realpath_shape, "function:function");

            let advanced: Promise = ctx.eval(
                r#"(async () => {
                  const fs = process.getBuiltinModule("node:fs");
                  fs.accessSync("/bundle/config/app.json", fs.R_OK);
                  let accessCode;
                  try { fs.accessSync("/bundle/config/app.json", fs.W_OK); }
                  catch (error) { accessCode = error.code; }
                  const temp = fs.mkdtempSync("/tmp/prefix-");
                  fs.writeFileSync(`${temp}/nested.txt`, "nested");
                  fs.linkSync(`${temp}/nested.txt`, `${temp}/hard.txt`);
                  fs.writeFileSync("/tmp/data/fd-append.txt", "a");
                  const appendFd = fs.openSync("/tmp/data/fd-append.txt", "r+");
                  fs.appendFileSync(appendFd, "b");
                  fs.fsync(appendFd);
                  fs.close(appendFd);
                  fs.cpSync(temp, "/tmp/copied", { recursive: true });
                  const matches = fs.globSync("**/*.txt", { cwd: "/tmp/copied" });
                  const fd = fs.openSync("/tmp/data/vectors.bin", "w+");
                  const written = fs.writevSync(fd, [Uint8Array.from([1, 2]), Uint8Array.from([3, 4])], 0);
                  fs.closeSync(fd);
                  const readFd = fs.openSync("/tmp/data/vectors.bin", "r");
                  const first = new Uint8Array(2);
                  const second = new Uint8Array(2);
                  const read = fs.readvSync(readFd, [first, second], 0);
                  fs.closeSync(readFd);
                  const dir = fs.opendirSync(temp);
                  const dirEntry = dir.readSync();
                  dir.closeSync();
                  const recursiveDir = fs.opendirSync(temp, { recursive: true });
                  const recursiveNames = [];
                  for (let entry; (entry = recursiveDir.readSync()) !== null;) recursiveNames.push(entry.name);
                  recursiveDir.closeSync();
                  const handle = await fs.promises.open("/tmp/data/value.txt", "r");
                  const handleRead = await handle.read({ length: 5 });
                  await handle.close();
                  const writeHandle = await fs.promises.open("/tmp/data/handle-write.txt", "w+");
                  const writeResult = await writeHandle.writeFile("write");
                  await writeHandle.appendFile("!");
                  const handleWriteText = await writeHandle.readFile("utf8");
                  await writeHandle.close();
                  const blob = fs.openAsBlob("/bundle/config/app.json");
                  const blobText = await blob.text();
                  const nativeRealpath = await new Promise((resolve, reject) => fs.realpath.native(
                    "/tmp/data/value.txt",
                    (error, value) => error ? reject(error) : resolve(value),
                  ));
                  const callbackRead = await new Promise((resolve, reject) => {
                    const callbackFd = fs.openSync("/tmp/data/value.bin", "r");
                    const callbackBuffer = new Uint8Array(3);
                    fs.read(callbackFd, callbackBuffer, 0, 3, 0, (error, count, output) => {
                      fs.closeSync(callbackFd);
                      error ? reject(error) : resolve({ count, output: [...output] });
                    });
                  });
                  const callbackReadOptions = await new Promise((resolve, reject) => {
                    const overloadFd = fs.openSync("/tmp/data/value.bin", "r");
                    fs.read(overloadFd, { length: 3, position: 0 }, (error, count, output) => {
                      fs.closeSync(overloadFd);
                      error ? reject(error) : resolve({ count, output: [...output] });
                    });
                  });
                  const callbackReadBufferOptions = await new Promise((resolve, reject) => {
                    const overloadFd = fs.openSync("/tmp/data/value.bin", "r");
                    const overloadBuffer = new Uint8Array(3);
                    fs.read(overloadFd, overloadBuffer, { length: 3, position: 0 }, (error, count, output) => {
                      fs.closeSync(overloadFd);
                      error ? reject(error) : resolve({ count, output: [...output] });
                    });
                  });
                  for await (const match of fs.promises.glob("**/*.txt", { cwd: temp })) {
                    if (match !== "nested.txt" && match !== "hard.txt") throw new Error("bad glob");
                  }
                  return JSON.stringify({
                    accessCode,
                    hardLink: fs.readFileSync(`${temp}/hard.txt`, "utf8"),
                    fdAppend: fs.readFileSync("/tmp/data/fd-append.txt", "utf8"),
                    matches,
                    written,
                    read,
                    first: [...first],
                    second: [...second],
                    dirEntry: dirEntry.name,
                    recursiveNames,
                    handleText: String.fromCharCode(...handleRead.buffer),
                    writeResult: writeResult.bytesWritten,
                    handleWriteText,
                    blobText,
                    callbackRead,
                    callbackReadOptions,
                    callbackReadBufferOptions,
                    nativeRealpath,
                    bigint: String(fs.statSync(`${temp}/nested.txt`, { bigint: true }).size),
                  });
                })()"#,
            )?;
            let advanced_result = advanced.finish::<String>().map_err(|error| {
                format!("advanced filesystem test failed: {error}; {:?}", ctx.catch())
            })?;
            assert_eq!(
                advanced_result,
                r#"{"accessCode":"ENOENT","hardLink":"nested","fdAppend":"ab","matches":["hard.txt","nested.txt"],"written":4,"read":4,"first":[1,2],"second":[3,4],"dirEntry":"hard.txt","recursiveNames":["hard.txt","nested.txt"],"handleText":"hello","writeResult":5,"handleWriteText":"write!","blobText":"{\"enabled\":true}","callbackRead":{"count":3,"output":[1,2,3]},"callbackReadOptions":{"count":3,"output":[1,2,3]},"callbackReadBufferOptions":{"count":3,"output":[1,2,3]},"nativeRealpath":"/tmp/data/value.txt","bigint":"6"}"#
            );
            Ok(())
        })
    }

    #[test]
    fn gives_each_context_a_fresh_tmp_directory() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let runtime = Runtime::new()?;
        runtime.set_loader(
            BuiltinResolver::default()
                .with_module(MODULE_NAME)
                .with_module(PROMISES_MODULE_NAME),
            ModuleLoader::default()
                .with_module(MODULE_NAME, NodeFsModule)
                .with_module(PROMISES_MODULE_NAME, NodeFsPromisesModule),
        );

        let first = Context::full(&runtime)?;
        first.with(|ctx| -> Result<(), Box<dyn std::error::Error>> {
            let vfs = Arc::new(Mutex::new(VirtualFileSystem::new(Bundle::new(
                directory.path(),
            ))));
            install(&ctx, &vfs)?;
            ctx.eval::<(), _>(
                r#"process.getBuiltinModule("node:fs").writeFileSync("/tmp/only-here", "value")"#,
            )?;
            Ok(())
        })?;
        drop(first);

        let second = Context::full(&runtime)?;
        second.with(|ctx| -> Result<(), Box<dyn std::error::Error>> {
            let vfs = Arc::new(Mutex::new(VirtualFileSystem::new(Bundle::new(
                directory.path(),
            ))));
            install(&ctx, &vfs)?;
            let exists: bool =
                ctx.eval(r#"process.getBuiltinModule("node:fs").existsSync("/tmp/only-here")"#)?;
            assert!(!exists);
            Ok(())
        })?;
        Ok(())
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn exposes_native_stream_bindings() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let runtime = Runtime::new()?;
        runtime.set_loader(
            BuiltinResolver::default()
                .with_module(MODULE_NAME)
                .with_module(PROMISES_MODULE_NAME),
            ModuleLoader::default()
                .with_module(MODULE_NAME, NodeFsModule)
                .with_module(PROMISES_MODULE_NAME, NodeFsPromisesModule),
        );
        let context = Context::full(&runtime)?;
        context.with(|ctx| -> Result<(), Box<dyn std::error::Error>> {
            let vfs = Arc::new(Mutex::new(VirtualFileSystem::new(Bundle::new(
                directory.path(),
            ))));
            install(&ctx, &vfs)?;
            let result: String = ctx.eval(
                r#"(() => {
                  const nativeGetBuiltinModule = process.getBuiltinModule;
                  class Readable {
                    constructor() { this.chunks = []; this.ended = false; }
                    push(value) { if (value === null) this.ended = true; else this.chunks.push(value); return true; }
                    on(name, listener) {
                      if (name === "data") for (const chunk of this.chunks) listener(chunk);
                      if (name === "end" && this.ended) listener();
                      return this;
                    }
                  }
                  class Writable {}
                  process.getBuiltinModule = name => name === "node:stream"
                    ? { Readable, Writable }
                    : nativeGetBuiltinModule(name);
                  const fs = process.getBuiltinModule("node:fs");
                  fs.writeFileSync("/tmp/stream-source.txt", "source");
                  const read = new fs.ReadStream("/tmp/stream-source.txt");
                  let text = "";
                  read.on("data", chunk => text += String.fromCharCode(...chunk));
                  read.on("end", () => text += "!");
                  const fd = fs.openSync("/tmp/stream-source.txt", "r");
                  const readWithFd = fs.createReadStream("/tmp/stream-source.txt", { fd });
                  let fdText = "";
                  readWithFd.on("data", chunk => fdText += String.fromCharCode(...chunk));
                  const fdOpen = (() => { try { fs.fstatSync(fd); return true; } catch { return false; } })();
                  fs.closeSync(fd);
                  const write = new fs.WriteStream("/tmp/stream-destination.txt");
                  write.write("one");
                  write.end("two");
                  const writeFd = fs.openSync("/tmp/stream-with-fd.txt", "w+");
                  const writeWithFd = fs.createWriteStream("/tmp/stream-with-fd.txt", { fd: writeFd });
                  writeWithFd.write("fd");
                  writeWithFd.end("stream");
                  const writeFdOpen = (() => { try { fs.fstatSync(writeFd); return true; } catch { return false; } })();
                  fs.closeSync(writeFd);
                  return JSON.stringify({
                    text,
                    fdText,
                    fdOpen,
                    writeFdOpen,
                    written: fs.readFileSync("/tmp/stream-destination.txt", "utf8"),
                    writtenWithFd: fs.readFileSync("/tmp/stream-with-fd.txt", "utf8"),
                  });
                })()"#,
            )
            .map_err(|error| format!("stream test failed: {error}; {:?}", ctx.catch()))?;
            assert_eq!(
                result,
                r#"{"text":"source!","fdText":"source","fdOpen":true,"writeFdOpen":true,"written":"onetwo","writtenWithFd":"fdstream"}"#
            );
            Ok(())
        })
    }
}
