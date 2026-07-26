use config::ConfigHandle;
use mux::domain::{Domain, LocalDomain};
use mux::Mux;
use std::sync::Arc;
use wezterm_client::domain::{ClientDomain, ClientDomainConfig};

pub mod dispatch;
pub mod local;
pub mod sessionhandler;

fn client_domains(config: &config::ConfigHandle) -> Vec<ClientDomainConfig> {
    let mut domains = vec![];
    for unix_dom in &config.unix_domains {
        domains.push(ClientDomainConfig::Unix(unix_dom.clone()));
    }
    domains
}

pub fn update_mux_domains(config: &ConfigHandle) -> anyhow::Result<()> {
    update_mux_domains_impl(config, false)
}

pub fn update_mux_domains_for_server(config: &ConfigHandle) -> anyhow::Result<()> {
    update_mux_domains_impl(config, true)
}

/// Add any newly discovered WSL domains to the mux.
///
/// When `wsl_domains` is not explicitly set in the user's configuration,
/// `config.wsl_domains()` falls back to `WslDomain::default_domains()`,
/// which shells out to `wsl.exe -l -v` to enumerate installed distros.
/// That subprocess call can take many seconds (or tens of seconds) to
/// return on Windows: it may need to start the LxssManager service and/or
/// boot the WSL2 utility VM before it can report anything, even when the
/// user never intends to use a WSL domain from wezterm.
///
/// Because this used to run synchronously on the GUI startup path (via
/// `update_mux_domains` -> `setup_mux`, called before the window is even
/// created), a slow/first-time `wsl.exe` invocation directly translated
/// into a slow-to-appear wezterm window (see wezterm-gui issue #7782,
/// reporting 15-50 second startup delays with trace logs showing the gap
/// landing exactly around WSL/mux domain setup).
///
/// To avoid blocking startup, only add WSL domains here when the caller
/// already knows about them synchronously (i.e. the user explicitly
/// configured `wsl_domains`), and otherwise kick off the potentially slow
/// discovery on a background thread, wiring the results into the mux
/// once they are available. This keeps behavior identical (WSL domains
/// still show up automatically), it just no longer blocks the caller.
fn add_wsl_domains(config: &ConfigHandle) {
    if config.wsl_domains.is_some() {
        // The user explicitly configured the list: this is already
        // in-memory and cheap to use, no need to defer.
        add_wsl_domains_to_mux(config.wsl_domains().into_iter());
        return;
    }

    // No explicit configuration: discovering the default list may shell
    // out to `wsl.exe` and block for a long time. Do that on a background
    // thread so that mux/gui startup is never held hostage by it.
    std::thread::Builder::new()
        .name("wsl-domain-discovery".into())
        .spawn(|| {
            let domains = config::WslDomain::default_domains();
            if domains.is_empty() {
                return;
            }
            promise::spawn::spawn_into_main_thread(async move {
                add_wsl_domains_to_mux(domains.into_iter());
            })
            .detach();
        })
        .ok();
}

fn add_wsl_domains_to_mux(wsl_domains: impl Iterator<Item = config::WslDomain>) {
    let mux = Mux::get();
    for wsl_dom in wsl_domains {
        if mux.get_domain_by_name(&wsl_dom.name).is_some() {
            continue;
        }

        match LocalDomain::new_wsl(wsl_dom.clone()) {
            Ok(domain) => {
                let domain: Arc<dyn Domain> = Arc::new(domain);
                mux.add_domain(&domain);
            }
            Err(err) => {
                log::error!("Error adding wsl domain {}: {:#}", wsl_dom.name, err);
            }
        }
    }
}

fn update_mux_domains_impl(config: &ConfigHandle, is_standalone_mux: bool) -> anyhow::Result<()> {
    let mux = Mux::get();

    for client_config in client_domains(&config) {
        if mux.get_domain_by_name(client_config.name()).is_some() {
            continue;
        }

        let domain: Arc<dyn Domain> = Arc::new(ClientDomain::new(client_config));
        mux.add_domain(&domain);
    }

    add_wsl_domains(config);

    for exec_dom in &config.exec_domains {
        if mux.get_domain_by_name(&exec_dom.name).is_some() {
            continue;
        }

        let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new_exec_domain(exec_dom.clone())?);
        mux.add_domain(&domain);
    }

    for serial in &config.serial_ports {
        if mux.get_domain_by_name(&serial.name).is_some() {
            continue;
        }

        let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new_serial_domain(serial.clone())?);
        mux.add_domain(&domain);
    }

    if is_standalone_mux {
        if let Some(name) = &config.default_mux_server_domain {
            if let Some(dom) = mux.get_domain_by_name(name) {
                if dom.is::<ClientDomain>() {
                    anyhow::bail!("default_mux_server_domain cannot be set to a client domain!");
                }
                mux.set_default_domain(&dom);
            }
        }
    } else {
        if let Some(name) = &config.default_domain {
            if let Some(dom) = mux.get_domain_by_name(name) {
                mux.set_default_domain(&dom);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Regression test for wezterm-gui#7782 ("Extremely slow startup on
    /// Windows: 15-50 seconds delay in portable_pty::cmdbuilder and local
    /// gui-sock setup").
    ///
    /// Root cause: when the user has not explicitly configured
    /// `wsl_domains`, `update_mux_domains_impl` used to call
    /// `config.wsl_domains()` directly on the startup call stack. That
    /// falls back to `WslDomain::default_domains()`, which shells out to
    /// `wsl.exe -l -v` and can block for many seconds (starting the
    /// LxssManager service / booting the WSL2 utility VM) before the GUI
    /// window is even created. This landed exactly in the trace-log gap
    /// reported upstream, between `portable_pty::cmdbuilder` env setup and
    /// the mux/gui-sock startup continuing.
    ///
    /// This test proves that `add_wsl_domains` (called from
    /// `update_mux_domains_impl`) returns essentially immediately when
    /// `wsl_domains` is left unconfigured (`None`), regardless of how long
    /// the underlying `wsl.exe` discovery takes on this machine, because
    /// the discovery now happens on a detached background thread instead
    /// of being awaited inline.
    #[test]
    fn add_wsl_domains_does_not_block_when_unconfigured() {
        let config = ConfigHandle::default_config();
        assert!(
            config.wsl_domains.is_none(),
            "test assumes default config leaves wsl_domains unconfigured"
        );

        let start = Instant::now();
        add_wsl_domains(&config);
        let elapsed = start.elapsed();

        // A real `wsl.exe -l -v` invocation (when WSL is installed) can
        // take many seconds, especially on first use. If this call were
        // still synchronous, this assertion would fail/flake heavily on
        // any machine with WSL installed. Give ourselves a generous but
        // still far-below-"seconds" budget to catch a regression back to
        // the blocking behavior.
        assert!(
            elapsed < Duration::from_millis(500),
            "add_wsl_domains blocked for {:?}; WSL discovery must run \
             asynchronously, not on the startup call stack",
            elapsed
        );
    }
}
