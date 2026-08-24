use rquickjs::loader::{Loader, Resolver};
use rquickjs::{Context, Ctx, Module, Runtime, WriteOptions, WriteOptionsEndianness};

use super::api::{Error, Result};

pub(crate) fn compile(source: &[u8]) -> Result<Vec<u8>> {
    compile_module("appd-worker.mjs", source)
}

pub(crate) fn compile_module(name: &str, source: &[u8]) -> Result<Vec<u8>> {
    let runtime = Runtime::new().map_err(|error| stage_error("runtime", &error))?;
    // QuickJS resolves static imports while compiling a module. The emitted
    // modules are linked again by the runtime loader, so compilation only
    // needs placeholder declarations for those imports.
    runtime.set_loader(CompileResolver, CompileLoader);
    let context = Context::full(&runtime).map_err(|error| stage_error("context", &error))?;
    context.with(|ctx| {
        let module = Module::declare(ctx.clone(), name, source)
            .map_err(|error| stage_error("declare", &error))?;
        module
            .write(WriteOptions {
                endianness: WriteOptionsEndianness::Little,
                ..WriteOptions::default()
            })
            .map_err(|error| stage_error("write", &error))
    })
}

struct CompileResolver;

impl Resolver for CompileResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        Ok(super::api::resolve_module_name(base, name))
    }
}

struct CompileLoader;

impl Loader for CompileLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js>> {
        Module::declare(ctx.clone(), name, b"export {};" as &[u8])
    }
}

fn stage_error(stage: &str, error: &rquickjs::Error) -> Error {
    Error::Engine(format!("{stage}: {error}"))
}
