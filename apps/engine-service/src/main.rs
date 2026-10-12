#[cfg(any(target_os = "macos", target_os = "linux"))]
use mixrelay_pcm_ring as _;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(test, target_os = "macos"))]
mod macos_application_rate_tests;
#[cfg(target_os = "macos")]
mod macos_devices;
#[cfg(target_os = "macos")]
mod macos_pacing;
#[cfg(all(test, target_os = "macos"))]
mod shared_epoch_tests;

#[cfg(all(test, target_os = "macos"))]
#[test]
fn native_callback_and_hal_contracts() {
    mixrelay_macos_test_support::assert_native_contracts();
}

#[cfg(target_os = "linux")]
mod linux;

mod latency_maintenance;

fn main() {
    if std::env::args().skip(1).eq(["--version"]) {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .with_writer(std::io::stderr)
        .init();
    let mut arguments = std::env::args().skip(1);
    if let Some(mode) = arguments.next() {
        if mode == mixrelay_engine_process::CONTROLLER_ARGUMENT {
            if arguments.next().is_some() || cfg!(target_os = "macos") {
                tracing::error!("invalid controller-owned engine launch");
                std::process::exit(2);
            }
            let owner = match mixrelay_engine_process::EngineOwner::from_stdin() {
                Ok(owner) => owner,
                Err(error) => {
                    tracing::error!(%error, "could not observe controller ownership");
                    std::process::exit(1);
                }
            };
            run_engine_service(Some(owner));
            return;
        }
        run_child_mode(&mode, arguments);
        return;
    }
    run_engine_service(None);
}

fn run_child_mode(mode: &str, mut arguments: impl Iterator<Item = String>) {
    match mode {
        mixrelay_processor_host::PLUGIN_SCAN_ARGUMENT => {
            let Some(format) = arguments.next() else {
                eprintln!("MixRelay processor scan requires a format");
                std::process::exit(2);
            };
            let format = match mixrelay_processor_host::RackFormat::parse(&format) {
                Ok(format) => format,
                Err(error) => {
                    eprintln!("MixRelay processor scan failed: {error}");
                    std::process::exit(2);
                }
            };
            let paths = arguments.map(std::path::PathBuf::from).collect::<Vec<_>>();
            if let Err(error) = mixrelay_processor_host::run_plugin_scan(format, &paths) {
                eprintln!("MixRelay processor scan failed: {error}");
                std::process::exit(1);
            }
        }
        mixrelay_processor_host::PLUGIN_PROBE_ARGUMENT => {
            let Some(locator) = arguments.next() else {
                eprintln!("MixRelay processor probe requires a locator");
                std::process::exit(2);
            };
            let locator = match mixrelay_processor_host::RackLocator::decode(&locator) {
                Ok(locator) => locator,
                Err(error) => {
                    eprintln!("MixRelay processor probe failed: {error}");
                    std::process::exit(2);
                }
            };
            if let Err(error) = mixrelay_processor_host::run_plugin_probe(locator) {
                eprintln!("MixRelay processor probe failed: {error}");
                std::process::exit(1);
            }
        }
        mixrelay_processor_host::PLUGIN_WORKER_ARGUMENT => {
            let worker_arguments = arguments.collect::<Vec<_>>();
            if let Err(error) = mixrelay_processor_host::run_plugin_worker(&worker_arguments) {
                eprintln!("MixRelay processor worker failed: {error}");
                std::process::exit(1);
            }
        }
        argument => {
            eprintln!("unknown MixRelay engine argument: {argument}");
            std::process::exit(2);
        }
    }
}

fn run_engine_service(owner: Option<mixrelay_engine_process::EngineOwner>) {
    #[cfg(target_os = "macos")]
    {
        debug_assert!(owner.is_none());
        drop(owner);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("MixRelay engine async runtime should initialize");
        if let Err(error) = runtime.block_on(macos::run()) {
            eprintln!("MixRelay engine service failed: {error}");
            std::process::exit(1);
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Err(error) = linux::run(owner) {
            eprintln!("MixRelay Linux engine service failed: {error}");
            std::process::exit(1);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        eprintln!("MixRelay engine service is unsupported on this platform");
        std::process::exit(1);
    }
}
