use crate::config::validate_domain_name;
use crate::*;
use anyhow::Context;
use std::fmt::Display;
use std::str::FromStr;
use wezterm_dynamic::{FromDynamic, ToDynamic};

/// Represents a `[user@]host[:port]` string, as accepted by `wezterm ssh`
/// and used to bootstrap a TLS mux connection via an ssh session.
#[derive(Clone, Debug)]
pub struct SshParameters {
    pub username: Option<String>,
    pub host_and_port: String,
}

impl Display for SshParameters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(user) = &self.username {
            write!(f, "{}@{}", user, self.host_and_port)
        } else {
            write!(f, "{}", self.host_and_port)
        }
    }
}

pub fn username_from_env() -> anyhow::Result<String> {
    #[cfg(unix)]
    const USER: &str = "USER";
    #[cfg(windows)]
    const USER: &str = "USERNAME";

    std::env::var(USER).with_context(|| format!("while resolving {} env var", USER))
}

impl FromStr for SshParameters {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('@').collect();

        if parts.len() == 2 {
            Ok(Self {
                username: Some(parts[0].to_string()),
                host_and_port: parts[1].to_string(),
            })
        } else if parts.len() == 1 {
            Ok(Self {
                username: None,
                host_and_port: parts[0].to_string(),
            })
        } else {
            anyhow::bail!("failed to parse ssh parameters from `{}`", s);
        }
    }
}

#[derive(Default, Debug, Clone, FromDynamic, ToDynamic)]
pub struct TlsDomainServer {
    /// The address:port combination on which the server will listen
    /// for client connections
    pub bind_address: String,

    /// the path to an x509 PEM encoded private key file
    pub pem_private_key: Option<PathBuf>,

    /// the path to an x509 PEM encoded certificate file
    pub pem_cert: Option<PathBuf>,

    /// the path to an x509 PEM encoded CA chain file
    pub pem_ca: Option<PathBuf>,

    /// A set of paths to load additional CA certificates.
    /// Each entry can be either the path to a directory
    /// or to a PEM encoded CA file.  If an entry is a directory,
    /// then its contents will be loaded as CA certs and added
    /// to the trust store.
    #[dynamic(default)]
    pub pem_root_certs: Vec<PathBuf>,
}

#[derive(Default, Debug, Clone, FromDynamic, ToDynamic)]
pub struct TlsDomainClient {
    /// The name of this specific domain.  Must be unique amongst
    /// all types of domain in the configuration file.
    #[dynamic(validate = "validate_domain_name")]
    pub name: String,

    /// If set, use ssh to connect, start the server, and obtain
    /// a certificate.
    /// The value is "user@host:port", just like "wezterm ssh" accepts.
    pub bootstrap_via_ssh: Option<String>,

    /// identifies the host:port pair of the remote server.
    pub remote_address: String,

    /// the path to an x509 PEM encoded private key file
    pub pem_private_key: Option<PathBuf>,

    /// the path to an x509 PEM encoded certificate file
    pub pem_cert: Option<PathBuf>,

    /// the path to an x509 PEM encoded CA chain file
    pub pem_ca: Option<PathBuf>,

    /// A set of paths to load additional CA certificates.
    /// Each entry can be either the path to a directory or to a PEM encoded
    /// CA file.  If an entry is a directory, then its contents will be
    /// loaded as CA certs and added to the trust store.
    #[dynamic(default)]
    pub pem_root_certs: Vec<PathBuf>,

    /// explicitly control whether the client checks that the certificate
    /// presented by the server matches the hostname portion of
    /// `remote_address`.  The default is true.  This option is made
    /// available for troubleshooting purposes and should not be used outside
    /// of a controlled environment as it weakens the security of the TLS
    /// channel.
    #[dynamic(default)]
    pub accept_invalid_hostnames: bool,

    /// the hostname string that we expect to match against the common name
    /// field in the certificate presented by the server.  This defaults to
    /// the hostname portion of the `remote_address` configuration and you
    /// should not normally need to override this value.
    pub expected_cn: Option<String>,

    /// If true, connect to this domain automatically at startup
    #[dynamic(default)]
    pub connect_automatically: bool,

    #[dynamic(default = "default_read_timeout")]
    pub read_timeout: Duration,

    #[dynamic(default = "default_write_timeout")]
    pub write_timeout: Duration,

    #[dynamic(default = "default_local_echo_threshold_ms")]
    pub local_echo_threshold_ms: Option<u64>,

    /// The path to the wezterm binary on the remote host
    pub remote_wezterm_path: Option<String>,

    /// Show time since last response when waiting for a response.
    /// It is recommended to use
    /// <https://wezterm.org/config/lua/pane/get_metadata.html#since_last_response_ms>
    /// instead.
    #[dynamic(default)]
    pub overlay_lag_indicator: bool,
}

impl TlsDomainClient {
    pub fn ssh_parameters(&self) -> Option<anyhow::Result<SshParameters>> {
        self.bootstrap_via_ssh
            .as_ref()
            .map(|user_at_host_and_port| user_at_host_and_port.parse())
    }
}
