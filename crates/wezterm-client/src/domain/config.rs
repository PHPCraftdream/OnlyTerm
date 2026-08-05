use config::UnixDomain;

#[derive(Clone, Debug)]
pub enum ClientDomainConfig {
    Unix(UnixDomain),
}

impl ClientDomainConfig {
    pub fn name(&self) -> &str {
        match self {
            ClientDomainConfig::Unix(unix) => &unix.name,
        }
    }

    pub fn local_echo_threshold_ms(&self) -> Option<u64> {
        match self {
            ClientDomainConfig::Unix(unix) => unix.local_echo_threshold_ms,
        }
    }

    pub fn overlay_lag_indicator(&self) -> bool {
        match self {
            ClientDomainConfig::Unix(unix) => unix.overlay_lag_indicator,
        }
    }

    pub fn label(&self) -> String {
        match self {
            ClientDomainConfig::Unix(unix) => format!("unix mux {}", unix.socket_path().display()),
        }
    }

    pub fn connect_automatically(&self) -> bool {
        match self {
            ClientDomainConfig::Unix(unix) => unix.connect_automatically,
        }
    }
}
