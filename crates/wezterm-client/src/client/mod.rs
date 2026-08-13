use crate::domain::{ClientDomain, ClientDomainConfig};
use anyhow::{anyhow, bail, Context};
use codec::*;
use config::{configuration, UnixDomain};
use mux::client::ClientId;
use mux::connui::ConnectionUI;
use mux::domain::DomainId;
use mux::pane::PaneId;
use mux::Mux;
use smol::block_on;
use smol::channel::{bounded, unbounded, Receiver, Sender};
use smol::prelude::*;
use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use thiserror::Error;

mod conn;
mod unilateral;

use conn::Reconnectable;
pub use conn::{unix_connect_with_retry, AsyncReadAndWrite};
use unilateral::process_unilateral;

#[derive(Error, Debug)]
#[error("Timeout")]
struct Timeout;

#[derive(Error, Debug)]
#[error("ChannelSendError")]
struct ChannelSendError;

enum ReaderMessage {
    SendPdu {
        pdu: Pdu,
        promise: Sender<anyhow::Result<Pdu>>,
    },
}

#[derive(Clone)]
pub struct Client {
    sender: Sender<ReaderMessage>,
    local_domain_id: Option<DomainId>,
    pub client_id: ClientId,
    client_domain_config: ClientDomainConfig,
    pub is_reconnectable: bool,
    pub is_local: bool,
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[error(
    "Please install the same version of OnlyTerm on both the client and server!\n\
     The server version is {} (codec version {}),\n\
     which is not compatible with our version \n\
     {} (codec version {}).",
    version,
    codec_vers,
    config::wezterm_version(),
    CODEC_VERSION
)]
pub struct IncompatibleVersionError {
    pub version: String,
    pub codec_vers: usize,
}

macro_rules! rpc {
    ($method_name:ident, $request_type:ident, $response_type:ident) => {
        pub async fn $method_name(&self, pdu: $request_type) -> anyhow::Result<$response_type> {
            let start = std::time::Instant::now();
            let result = self.send_pdu(Pdu::$request_type(pdu)).await;
            let elapsed = start.elapsed();
            metrics::histogram!("rpc", "method" => stringify!($method_name)).record(elapsed);
            metrics::counter!("rpc.count", "method" => stringify!($method_name)).increment(1);
            match result {
                Ok(Pdu::$response_type(res)) => Ok(res),
                Ok(_) => bail!("unexpected response {:?}", result),
                Err(err) => Err(err),
            }
        }
    };

    // This variant allows omitting the request parameter; this is useful
    // in the case where the struct is empty and present only for the purpose
    // of typing the request.
    ($method_name:ident, $request_type:ident=(), $response_type:ident) => {
        #[allow(dead_code)]
        pub async fn $method_name(&self) -> anyhow::Result<$response_type> {
            let start = std::time::Instant::now();
            let result = self.send_pdu(Pdu::$request_type($request_type{})).await;
            let elapsed = start.elapsed();
            metrics::histogram!("rpc", "method" => stringify!($method_name)).record(elapsed);
            metrics::counter!("rpc.count", "method" => stringify!($method_name)).increment(1);
            match result {
                Ok(Pdu::$response_type(res)) => Ok(res),
                Ok(_) => bail!("unexpected response {:?}", result),
                Err(err) => Err(err),
            }
        }
    };
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
enum NotReconnectableError {
    #[error("Client was destroyed")]
    ClientWasDestroyed,
}

fn client_thread(
    reconnectable: &mut Reconnectable,
    local_domain_id: Option<DomainId>,
    rx: &mut Receiver<ReaderMessage>,
) -> anyhow::Result<()> {
    block_on(client_thread_async(reconnectable, local_domain_id, rx))
}

async fn client_thread_async(
    reconnectable: &mut Reconnectable,
    local_domain_id: Option<DomainId>,
    rx: &mut Receiver<ReaderMessage>,
) -> anyhow::Result<()> {
    let stream = reconnectable.take_stream().unwrap();
    let (reader, writer) = futures::AsyncReadExt::split(stream);

    // Channel for writer to register promises with the reader task.
    // Writer sends (serial, promise_sender) before writing the PDU to the socket,
    // so the reader always has the promise registered before the response can arrive.
    let (promise_tx, promise_rx) = smol::channel::unbounded::<(u64, Sender<anyhow::Result<Pdu>>)>();

    let writer_fut = async {
        let mut writer = writer;
        let mut next_serial = 1u64;

        loop {
            match rx.recv().await {
                Ok(ReaderMessage::SendPdu { pdu, promise }) => {
                    let serial = next_serial;
                    next_serial += 1;

                    // Register promise with reader before writing to socket
                    promise_tx
                        .send((serial, promise))
                        .await
                        .map_err(|_| anyhow!("reader task gone"))?;

                    pdu.encode_async(&mut writer, serial)
                        .await
                        .context("encoding a PDU to send to the server")?;
                    writer.flush().await.context("flushing PDU to server")?;
                }
                Err(_) => {
                    return Err(NotReconnectableError::ClientWasDestroyed.into());
                }
            }
        }
    };

    let reader_fut = async {
        let mut reader = reader;
        let mut promises = PromiseMap::new();

        loop {
            // Pass None for max_serial: with split read/write, the reader cannot
            // track the writer's next_serial without a race condition, since new
            // serials may be assigned while decode_async is awaiting.
            match Pdu::decode_async(&mut reader, None).await {
                Ok(decoded) => {
                    log::debug!(
                        "decoded serial {} {}",
                        decoded.serial,
                        decoded.pdu.pdu_name()
                    );

                    // Drain any newly registered promises from the writer
                    while let Ok((serial, promise)) = promise_rx.try_recv() {
                        promises.map.insert(serial, promise);
                    }

                    if decoded.serial == 0 {
                        process_unilateral(local_domain_id, decoded)
                            .context("processing unilateral PDU from server")
                            .map_err(|e| {
                                log::error!("process_unilateral: {:?}", e);
                                e
                            })?;
                    } else if let Some(promise) = promises.map.remove(&decoded.serial) {
                        if promise.try_send(Ok(decoded.pdu)).is_err() {
                            return Err(NotReconnectableError::ClientWasDestroyed.into());
                        }
                    } else {
                        let reason =
                            format!("got serial {:?} without a corresponding promise", decoded);
                        promises.fail_all(&reason);
                        anyhow::bail!("{}", reason);
                    }
                }
                Err(err) => {
                    let reason = format!("Error while decoding response pdu: {:#}", err);
                    log::error!("{}", reason);
                    promises.fail_all(&reason);
                    return Err(err).context("Error while decoding response pdu");
                }
            }
        }
    };

    // Run both tasks concurrently; first error terminates both
    smol::future::race(writer_fut, reader_fut).await
}

struct PromiseMap {
    map: HashMap<u64, Sender<anyhow::Result<Pdu>>>,
}

impl PromiseMap {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    fn fail_all(&mut self, reason: &str) {
        log::trace!("failing all promises: {}", reason);
        for (_, promise) in self.map.drain() {
            let _ = promise.try_send(Err(anyhow!("{}", reason)));
        }
    }
}

impl Drop for PromiseMap {
    fn drop(&mut self) {
        self.fail_all("Client was destroyed");
    }
}

impl Client {
    pub fn new(local_domain_id: Option<DomainId>, mut reconnectable: Reconnectable) -> Self {
        let client_domain_config = reconnectable.config.clone();
        let is_reconnectable = reconnectable.reconnectable();
        let is_local = reconnectable.is_local();
        let (sender, mut receiver) = unbounded();
        let client_id = ClientId::new();

        thread::spawn(move || {
            const BASE_INTERVAL: Duration = Duration::from_secs(1);
            const MAX_INTERVAL: Duration = Duration::from_secs(10);

            let mut backoff = BASE_INTERVAL;
            loop {
                if let Err(e) = client_thread(&mut reconnectable, local_domain_id, &mut receiver) {
                    if !reconnectable.reconnectable() || local_domain_id.is_none() {
                        log::debug!("client thread ended: {}", e);
                        break;
                    }

                    let local_domain_id = local_domain_id.expect("checked above");

                    if let Some(ioerr) = e.root_cause().downcast_ref::<std::io::Error>() {
                        if let std::io::ErrorKind::UnexpectedEof = ioerr.kind() {
                            // Don't reconnect for a simple EOF
                            log::error!("server closed connection ({})", e);
                            break;
                        }
                    }

                    if let Some(err) = e.root_cause().downcast_ref::<NotReconnectableError>() {
                        log::error!("{}; won't try to reconnect", err);
                        break;
                    }

                    let mut ui = ConnectionUI::new();
                    ui.title("wezterm: Reconnecting...");

                    loop {
                        ui.sleep_with_reason(
                            &format!("client disconnected {}; will reconnect", e),
                            backoff,
                        )
                        .ok();
                        let initial = false;
                        let no_auto_start = true; // Don't auto-start on a reconnect
                        match reconnectable.connect(initial, &mut ui, no_auto_start) {
                            Ok(_) => {
                                backoff = BASE_INTERVAL;
                                log::error!("Reconnected!");
                                promise::spawn::spawn_into_main_thread(async move {
                                    ClientDomain::reattach(local_domain_id, ui).await.ok();
                                })
                                .detach();
                                break;
                            }
                            Err(err) => {
                                backoff = (backoff + backoff).min(MAX_INTERVAL);
                                ui.output_str(&format!(
                                    "problem reconnecting: {}; will reconnect in {:?}\n",
                                    err, backoff
                                ));
                            }
                        }
                    }
                } else {
                    log::error!("client_thread returned without any error condition");
                    break;
                }
            }

            async fn detach(local_domain_id: DomainId) -> anyhow::Result<()> {
                if let Some(mux) = Mux::try_get() {
                    let client_domain = mux
                        .get_domain(local_domain_id)
                        .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
                    let client_domain =
                        client_domain
                            .downcast_ref::<ClientDomain>()
                            .ok_or_else(|| {
                                anyhow!("domain {} is not a ClientDomain instance", local_domain_id)
                            })?;
                    client_domain.perform_detach();
                }
                Ok(())
            }
            if let Some(domain_id) = local_domain_id {
                promise::spawn::spawn_into_main_thread(async move {
                    detach(domain_id).await.ok();
                })
                .detach();
            }
        });

        Self {
            sender,
            local_domain_id,
            is_reconnectable,
            is_local,
            client_id,
            client_domain_config,
        }
    }

    pub fn into_client_domain_config(self) -> ClientDomainConfig {
        self.client_domain_config
    }

    /// Create a Client from an already-connected UnixStream.
    /// This is for the elevated-tab path where the connection is already established
    /// and authenticated via the WebSocket rendezvous handshake.
    ///
    /// # Cancel-safety
    /// This function performs only a cheap `Async::new` wrap and spawns a thread.
    /// The `Async::new` call is synchronous but fast (just async-signal-safe registration),
    /// and the thread spawn is infallible. Cancellation cannot leave state inconsistent.
    pub fn new_with_stream(
        local_domain_id: Option<DomainId>,
        client_domain_config: ClientDomainConfig,
        stream: wezterm_uds::UnixStream,
    ) -> anyhow::Result<Self> {
        use smol::Async;

        // `stream` is already connected and authenticated by the caller (via the
        // WebSocket rendezvous handshake) -- this just registers it with the
        // async runtime, matching the wrapping already done in `conn.rs`.
        let stream: Box<dyn conn::AsyncReadAndWrite> = Box::new(Async::new(stream)?);
        let reconnectable = conn::Reconnectable::new(client_domain_config, Some(stream));
        Ok(Self::new(local_domain_id, reconnectable))
    }

    pub async fn verify_version_compat(
        &self,
        ui: &ConnectionUI,
    ) -> anyhow::Result<GetCodecVersionResponse> {
        match self
            .get_codec_version(GetCodecVersion {})
            .or(async {
                smol::Timer::after(Duration::from_secs(60)).await;
                Err(Timeout).context("Timeout")
            })
            .await
        {
            Ok(info) if info.codec_vers == CODEC_VERSION => {
                log::trace!(
                    "Server version is {} (codec version {})",
                    info.version_string,
                    info.codec_vers
                );
                self.set_client_id(SetClientId {
                    client_id: self.client_id.clone(),
                    is_proxy: false,
                })
                .await?;
                Ok(info)
            }
            Ok(info) => {
                let err = IncompatibleVersionError {
                    version: info.version_string,
                    codec_vers: info.codec_vers,
                };
                ui.output_str(&err.to_string());
                log::error!("{:?}", err);
                Err(err.into())
            }
            Err(err) => {
                log::trace!("{:?}", err);
                let msg = if err.root_cause().is::<Timeout>() {
                    "Timed out while parsing the response from the server. \
                    This may be due to network connectivity issues"
                        .to_string()
                } else if err.root_cause().is::<CorruptResponse>() {
                    "Received an implausible and likely corrupt response from \
                    the server. This can happen if the remote host outputs \
                    to stdout prior to running commands. \
                    Check your shell startup!"
                        .to_string()
                } else if err.root_cause().is::<ChannelSendError>() {
                    "Internal channel was closed prior to sending request. \
                    This may indicate that the remote host output invalid data \
                    to stdout prior to running the requested command. \
                    Check your shell startup!"
                        .to_string()
                } else {
                    format!(
                        "Please install the same version of OnlyTerm on both \
                     the client and server! \
                     The server reported error '{err}' while being asked for its \
                     version.  This likely means that the server is older \
                     than the client, but it could also happen if the remote \
                     host outputs to stdout prior to running commands. \
                     Check your shell startup!",
                    )
                };
                ui.output_str(&msg);
                bail!("{}", msg);
            }
        }
    }

    #[allow(dead_code)]
    pub fn local_domain_id(&self) -> Option<DomainId> {
        self.local_domain_id
    }

    fn compute_unix_domain(
        prefer_mux: bool,
        class_name: &str,
    ) -> anyhow::Result<config::UnixDomain> {
        match std::env::var_os("ONLYTERM_UNIX_SOCKET") {
            Some(path) if !path.is_empty() => Ok(config::UnixDomain {
                socket_path: Some(path.into()),
                ..Default::default()
            }),
            Some(_) | None => {
                if !prefer_mux {
                    if let Ok(gui) = crate::discovery::resolve_gui_sock_path(class_name) {
                        return Ok(config::UnixDomain {
                            socket_path: Some(gui),
                            no_serve_automatically: true,
                            ..Default::default()
                        });
                    }
                }

                let config = configuration();
                Ok(config
                    .unix_domains
                    .first()
                    .ok_or_else(|| {
                        anyhow!(
                            "no default unix domain is configured and ONLYTERM_UNIX_SOCKET \
                             is not set in the environment"
                        )
                    })?
                    .clone())
            }
        }
    }

    pub fn new_default_unix_domain(
        initial: bool,
        ui: &mut ConnectionUI,
        no_auto_start: bool,
        prefer_mux: bool,
        class_name: &str,
    ) -> anyhow::Result<Self> {
        let unix_dom = Self::compute_unix_domain(prefer_mux, class_name)?;
        Self::new_unix_domain(None, &unix_dom, initial, ui, no_auto_start)
    }

    pub fn new_unix_domain(
        local_domain_id: Option<DomainId>,
        unix_dom: &UnixDomain,
        initial: bool,
        ui: &mut ConnectionUI,
        no_auto_start: bool,
    ) -> anyhow::Result<Self> {
        let mut reconnectable =
            Reconnectable::new(ClientDomainConfig::Unix(unix_dom.clone()), None);
        reconnectable.connect(initial, ui, no_auto_start)?;
        Ok(Self::new(local_domain_id, reconnectable))
    }

    pub async fn send_pdu(&self, pdu: Pdu) -> anyhow::Result<Pdu> {
        let (promise, rx) = bounded(1);
        self.sender
            .send(ReaderMessage::SendPdu { pdu, promise })
            .await
            .map_err(|_| ChannelSendError)
            .context("send_pdu send")?;
        rx.recv().await.context("send_pdu recv")?
    }

    pub async fn resolve_pane_id(&self, pane_id: Option<PaneId>) -> anyhow::Result<PaneId> {
        let pane_id: PaneId = match pane_id {
            Some(p) => p,
            None => {
                if let Ok(pane) = std::env::var("ONLYTERM_PANE") {
                    pane.parse()?
                } else {
                    let mut clients = self.list_clients().await?.clients;
                    clients.retain(|client| client.focused_pane_id.is_some());
                    clients.sort_by_key(|c| std::cmp::Reverse(c.last_input));
                    if clients.is_empty() {
                        anyhow::bail!(
                            "--pane-id was not specified and $ONLYTERM_PANE
                         is not set in the environment, and I couldn't
                         determine which pane was currently focused"
                        );
                    }

                    clients[0]
                        .focused_pane_id
                        .expect("to have filtered out above")
                }
            }
        };
        Ok(pane_id)
    }

    rpc!(ping, Ping = (), Pong);
    rpc!(list_panes, ListPanes = (), ListPanesResponse);
    rpc!(spawn_v2, SpawnV2, SpawnResponse);
    rpc!(split_pane, SplitPane, SpawnResponse);
    rpc!(
        move_pane_to_new_tab,
        MovePaneToNewTab,
        MovePaneToNewTabResponse
    );
    rpc!(write_to_pane, WriteToPane, UnitResponse);
    rpc!(send_paste, SendPaste, UnitResponse);
    rpc!(key_down, SendKeyDown, UnitResponse);
    rpc!(mouse_event, SendMouseEvent, UnitResponse);
    rpc!(resize, Resize, UnitResponse);
    rpc!(set_zoomed, SetPaneZoomed, UnitResponse);
    rpc!(activate_pane_direction, ActivatePaneDirection, UnitResponse);
    rpc!(
        swap_active_pane_with_index,
        SwapActivePaneWithIndex,
        UnitResponse
    );
    rpc!(rotate_panes, RotatePanes, UnitResponse);
    rpc!(
        get_pane_render_changes,
        GetPaneRenderChanges,
        LivenessResponse
    );
    rpc!(get_lines, GetLines, GetLinesResponse);
    rpc!(
        get_dimensions,
        GetPaneRenderableDimensions,
        GetPaneRenderableDimensionsResponse
    );
    rpc!(get_codec_version, GetCodecVersion, GetCodecVersionResponse);
    rpc!(
        search_scrollback,
        SearchScrollbackRequest,
        SearchScrollbackResponse
    );
    rpc!(kill_pane, KillPane, UnitResponse);
    rpc!(set_client_id, SetClientId, UnitResponse);
    rpc!(list_clients, GetClientList = (), GetClientListResponse);
    rpc!(set_window_workspace, SetWindowWorkspace, UnitResponse);
    rpc!(set_focused_pane_id, SetFocusedPane, UnitResponse);
    rpc!(get_image_cell, GetImageCell, GetImageCellResponse);
    rpc!(set_configured_palette_for_pane, SetPalette, UnitResponse);
    rpc!(set_tab_title, TabTitleChanged, UnitResponse);
    rpc!(set_window_title, WindowTitleChanged, UnitResponse);
    rpc!(rename_workspace, RenameWorkspace, UnitResponse);
    rpc!(erase_scrollback, EraseScrollbackRequest, UnitResponse);
    rpc!(
        get_pane_direction,
        GetPaneDirection,
        GetPaneDirectionResponse
    );
    rpc!(adjust_pane_size, AdjustPaneSize, UnitResponse);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promise_map_fail_all_resolves_every_promise_with_an_error() {
        let mut promises = PromiseMap::new();
        let (tx1, rx1) = unbounded::<anyhow::Result<Pdu>>();
        let (tx2, rx2) = unbounded::<anyhow::Result<Pdu>>();
        promises.map.insert(1, tx1);
        promises.map.insert(2, tx2);

        promises.fail_all("boom");

        let err1 = rx1.try_recv().unwrap().unwrap_err();
        let err2 = rx2.try_recv().unwrap().unwrap_err();
        assert_eq!(err1.to_string(), "boom");
        assert_eq!(err2.to_string(), "boom");
        assert!(promises.map.is_empty());
    }

    #[test]
    fn promise_map_drop_fails_all_promises() {
        let rx = {
            let mut promises = PromiseMap::new();
            let (tx, rx) = unbounded::<anyhow::Result<Pdu>>();
            promises.map.insert(42, tx);
            rx
        };
        let err = rx.try_recv().unwrap().unwrap_err();
        assert_eq!(err.to_string(), "Client was destroyed");
    }
}
