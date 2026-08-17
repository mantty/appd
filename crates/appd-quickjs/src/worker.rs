use rquickjs::{Context, Module, Runtime, WriteOptions, WriteOptionsEndianness};

use crate::{Error, Result};

pub(crate) fn compile(source: &[u8]) -> Result<Vec<u8>> {
    let runtime = Runtime::new().map_err(|error| stage_error("runtime", &error))?;
    let context = Context::full(&runtime).map_err(|error| stage_error("context", &error))?;
    context.with(|ctx| {
        let module = Module::declare(ctx.clone(), "appd-worker.mjs", source)
            .map_err(|error| stage_error("declare", &error))?;
        module
            .write(WriteOptions {
                endianness: WriteOptionsEndianness::Little,
                ..WriteOptions::default()
            })
            .map_err(|error| stage_error("write", &error))
    })
}

fn stage_error(stage: &str, error: &rquickjs::Error) -> Error {
    Error::Engine(format!("{stage}: {error}"))
}
