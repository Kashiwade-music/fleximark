use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, ResourceLimiter, Store};
use wasmtime_wasi::{DirPerms, FilePerms, IoView, WasiCtx, WasiCtxBuilder, WasiView};

use super::{CancellationToken, PluginRuntime, RuntimeError, RuntimeOutput, SandboxPolicy};
use fleximark_plugin_sdk::HookRequest;

mod component_api {
    ::wasmtime::component::bindgen!({
        path: "../fleximark-plugin-sdk/wit",
        world: "fleximark-plugin",
    });
}

/// Production runtime for the versioned FlexiMark Component Model world.
/// WASI Preview 2 starts empty, then receives only granted roots and environment access.
pub struct WasmtimeRuntime {
    engine: Engine,
    component: Component,
    invoke_lock: std::sync::Mutex<()>,
}

struct WasmState {
    wasi: WasiCtx,
    table: ResourceTable,
    max_memory_bytes: usize,
    peak_memory_bytes: usize,
    memory_limit_hit: bool,
}

impl IoView for WasmState {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiView for WasmState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

impl ResourceLimiter for WasmState {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> ::wasmtime::Result<bool> {
        let allowed = desired <= self.max_memory_bytes;
        self.peak_memory_bytes = self.peak_memory_bytes.max(desired);
        self.memory_limit_hit |= !allowed;
        Ok(allowed)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> ::wasmtime::Result<bool> {
        Ok(desired <= 10_000)
    }
}

impl WasmtimeRuntime {
    pub fn new(wasm: &[u8]) -> Result<Self, RuntimeError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        let engine =
            Engine::new(&config).map_err(|error| RuntimeError::Malformed(error.to_string()))?;
        let component = Component::new(&engine, wasm)
            .map_err(|error| RuntimeError::Malformed(error.to_string()))?;
        Ok(Self {
            engine,
            component,
            invoke_lock: std::sync::Mutex::new(()),
        })
    }
}

impl PluginRuntime for WasmtimeRuntime {
    fn invoke(
        &self,
        request: HookRequest,
        sandbox: SandboxPolicy,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput, RuntimeError> {
        let _invoke_guard = self
            .invoke_lock
            .lock()
            .map_err(|_| RuntimeError::Trap("plugin invocation lock was poisoned".to_owned()))?;
        if cancellation.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        let mut wasi = WasiCtxBuilder::new();
        wasi.allow_blocking_current_thread(true);
        for (name, value) in &sandbox.environment {
            wasi.env(name, value);
        }
        for (index, root) in sandbox.read_roots.iter().enumerate() {
            wasi.preopened_dir(
                root,
                format!("workspace-read-{index}"),
                DirPerms::READ,
                FilePerms::READ,
            )
            .map_err(|error| RuntimeError::Policy(error.to_string()))?;
        }
        for (index, root) in sandbox.write_roots.iter().enumerate() {
            wasi.preopened_dir(
                root,
                format!("workspace-write-{index}"),
                DirPerms::MUTATE,
                FilePerms::WRITE,
            )
            .map_err(|error| RuntimeError::Policy(error.to_string()))?;
        }

        let mut store = Store::new(
            &self.engine,
            WasmState {
                wasi: wasi.build(),
                table: ResourceTable::new(),
                max_memory_bytes: usize::try_from(sandbox.max_linear_memory_bytes)
                    .unwrap_or(usize::MAX),
                peak_memory_bytes: 0,
                memory_limit_hit: false,
            },
        );
        store.limiter(|state| state);
        store
            .set_fuel(sandbox.max_fuel)
            .map_err(|error| RuntimeError::Malformed(error.to_string()))?;
        store.set_epoch_deadline(1);

        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)
            .map_err(|error| RuntimeError::Malformed(error.to_string()))?;
        let hook = serde_json::to_value(request.hook())
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| RuntimeError::Malformed("hook name is not a string".to_owned()))?;
        let request_json = serde_json::to_string(&request)
            .map_err(|error| RuntimeError::Malformed(error.to_string()))?;

        let done = Arc::new(AtomicBool::new(false));
        let done_by_watchdog = Arc::clone(&done);
        let cancelled_by_watchdog = cancellation.clone();
        let deadline = Instant::now() + sandbox.timeout;
        let watchdog_engine = self.engine.clone();
        let watchdog = thread::spawn(move || {
            while !done_by_watchdog.load(Ordering::Acquire) {
                if cancelled_by_watchdog.is_cancelled() || Instant::now() >= deadline {
                    watchdog_engine.increment_epoch();
                    return;
                }
                thread::sleep(Duration::from_millis(1));
            }
        });
        let started = Instant::now();
        let execution = (|| {
            let plugin =
                component_api::FleximarkPlugin::instantiate(&mut store, &self.component, &linker)
                    .map_err(|error| classify_wasm_error(error, false, false))?;
            let invocation = component_api::exports::fleximark::plugin::hooks::Invocation {
                api_version: fleximark_plugin_sdk::PLUGIN_API_VERSION,
                hook,
                request_json,
            };
            let response = plugin
                .fleximark_plugin_hooks()
                .call_invoke(&mut store, &invocation)
                .map_err(|error| classify_wasm_error(error, false, false))?
                .map_err(RuntimeError::Trap)?;
            if response.api_version != fleximark_plugin_sdk::PLUGIN_API_VERSION {
                return Err(RuntimeError::Malformed(format!(
                    "guest returned plugin API version {}",
                    response.api_version
                )));
            }
            if response.response_json.len() > sandbox.max_output_bytes {
                return Err(RuntimeError::OutputLimit(
                    "guest response exceeds the output limit".to_owned(),
                ));
            }
            let response = serde_json::from_str(&response.response_json)
                .map_err(|error| RuntimeError::Malformed(error.to_string()))?;
            Ok(RuntimeOutput {
                response,
                peak_memory_bytes: store.data().peak_memory_bytes as u64,
            })
        })();
        done.store(true, Ordering::Release);
        let _ = watchdog.join();
        let timed_out = started.elapsed() >= sandbox.timeout;
        if store.data().memory_limit_hit {
            return Err(RuntimeError::MemoryLimit(
                "linear memory limit exceeded".to_owned(),
            ));
        }
        if cancellation.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        if timed_out {
            return Err(RuntimeError::Timeout(
                "execution deadline exceeded".to_owned(),
            ));
        }
        execution
    }
}

fn classify_wasm_error(error: ::wasmtime::Error, cancelled: bool, timed_out: bool) -> RuntimeError {
    let message = format!("{error:#}");
    if cancelled {
        RuntimeError::Cancelled
    } else if timed_out
        || matches!(
            error.downcast_ref::<::wasmtime::Trap>(),
            Some(::wasmtime::Trap::Interrupt | ::wasmtime::Trap::OutOfFuel)
        )
    {
        RuntimeError::Timeout(message)
    } else {
        RuntimeError::Trap(message)
    }
}
