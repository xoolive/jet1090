use makiko::{
    ChannelConfig, Client, ClientConfig, ClientReceiver, Privkey,
    TunnelReceiver, TunnelStream,
};
use once_cell::sync::Lazy;
use ssh2_config::{ParseRule, SshConfig};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    process::{ChildStdin, ChildStdout, Command},
};
use tracing::{debug, info, warn};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub static CONNECTION_MAP: Lazy<Arc<Mutex<HashMap<String, Client>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Prevent concurrent first-time connections from racing before the cache is
/// populated. This matters when several sources share one ProxyJump host.
static CONNECTION_SETUP_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub struct TunnelledTcp {
    pub address: String,
    pub port: u16,
    pub jump: String,
}

pub struct TunnelledWebsocket {
    pub address: String,
    pub port: u16,
    pub url: String,
    pub jump: String,
}

pub struct TunnelledSero {
    pub jump: String,
}

async fn authenticate_server(
    mut client_rx: ClientReceiver,
    host: String,
    port: u16,
) -> Result<(), BoxError> {
    let mut hosts_path = dirs::home_dir().ok_or_else(|| {
        BoxError::from("Could not determine the home directory")
    })?;
    hosts_path.push(".ssh");
    hosts_path.push("known_hosts");
    let hosts_data = std::fs::read(&hosts_path).map_err(|error| {
        BoxError::from(format!(
            "Could not read {}: {error}",
            hosts_path.display()
        ))
    })?;
    let mut hosts_file = makiko::host_file::File::decode(hosts_data.into());

    loop {
        let event = client_rx.recv().await.map_err(|error| {
            BoxError::from(format!(
                "Error while receiving SSH client event: {error}"
            ))
        })?;
        let Some(event) = event else { break };

        if let makiko::ClientEvent::ServerPubkey(pubkey, accept) = event {
            match hosts_file.match_host_port_key(&host, port, &pubkey) {
                makiko::host_file::KeyMatch::Accepted(_) => accept.accept(),
                makiko::host_file::KeyMatch::Revoked(_) => {
                    return Err(BoxError::from(
                        "The SSH server key was revoked in known_hosts",
                    ));
                }
                makiko::host_file::KeyMatch::OtherKeys(_) => {
                    return Err(BoxError::from(
                        "SSH host key differs from known_hosts; refusing connection",
                    ));
                }
                makiko::host_file::KeyMatch::NotFound => {
                    accept.accept();
                    hosts_file.append_entry(
                        makiko::host_file::File::entry_builder()
                            .host_port(&host, port)
                            .key(pubkey),
                    );
                    std::fs::write(&hosts_path, hosts_file.encode()).map_err(
                        |error| {
                            BoxError::from(format!(
                                "Could not update {}: {error}",
                                hosts_path.display()
                            ))
                        },
                    )?;
                }
            }
        }
    }
    Ok(())
}

async fn authenticate_by_private_key(
    client: &Client,
    user: &str,
    privkey: &Privkey,
) -> Result<(), BoxError> {
    let pubkey = privkey.pubkey();
    for pubkey_algo in pubkey.algos().iter().copied() {
        if client
            .check_pubkey(user.to_string(), &pubkey, pubkey_algo)
            .await
            .map_err(|error| {
                BoxError::from(format!(
                    "Error when checking an SSH public key: {error}"
                ))
            })?
        {
            match client
                .auth_pubkey(user.to_string(), privkey.clone(), pubkey_algo)
                .await
                .map_err(|error| {
                    BoxError::from(format!(
                        "Error authenticating with SSH key: {error}"
                    ))
                })? {
                makiko::AuthPubkeyResult::Success => return Ok(()),
                makiko::AuthPubkeyResult::Failure(failure) => {
                    info!("The server rejected authentication with {pubkey_algo:?}: {failure:?}");
                }
            }
        }
    }
    Err(BoxError::from(
        "The SSH server does not accept the private key",
    ))
}

fn get_params() -> Result<SshConfig, BoxError> {
    let config_path = dirs::home_dir()
        .ok_or_else(|| {
            BoxError::from("Could not determine the home directory")
        })?
        .join(".ssh")
        .join("config");
    let file = File::open(&config_path).map_err(|error| {
        BoxError::from(format!(
            "Could not read {}: {error}",
            config_path.display()
        ))
    })?;
    let mut reader = BufReader::new(file);

    SshConfig::default()
        .parse(
            &mut reader,
            ParseRule::ALLOW_UNKNOWN_FIELDS
                | ParseRule::ALLOW_UNSUPPORTED_FIELDS,
        )
        .map_err(|error| {
            BoxError::from(format!(
                "Failed to parse {}: {error}",
                config_path.display()
            ))
        })
}

fn get_default_username() -> Result<String, BoxError> {
    #[cfg(target_os = "windows")]
    let username = std::env::var("USERNAME").map_err(|_| {
        BoxError::from("Could not determine the current Windows user name")
    })?;
    #[cfg(not(target_os = "windows"))]
    let username = std::env::var("USER").map_err(|_| {
        BoxError::from("Could not determine the current user name")
    })?;
    Ok(username)
}

enum Io {
    Tcp(TcpStream),
    Tunnel(TunnelStream),
    Proxy(ProxyCommand),
}

/**
 * This function connects to a server using SSH. It handles proxy commands
 * and proxy jumps. It also handles authentication using private keys.
 * It returns a Client object that can be used to interact with the server.
 */
async fn connect_server(
    server: &str,
    params: &SshConfig,
    connection_map: Arc<Mutex<HashMap<String, Client>>>,
) -> Result<Client, BoxError> {
    let _setup_lock = CONNECTION_SETUP_LOCK.lock().await;
    connect_server_inner(server, params, connection_map).await
}

#[async_recursion::async_recursion]
async fn connect_server_inner(
    server: &str,
    params: &SshConfig,
    connection_map: Arc<Mutex<HashMap<String, Client>>>,
) -> Result<Client, BoxError> {
    debug!(server, "Starting SSH connection setup");
    // Check if the server is already connected
    // If so, return the existing connection
    if connection_map.lock().await.contains_key(server) {
        info!("Reusing existing connection to {server}");
        return connection_map.lock().await.get(server).cloned().ok_or_else(
            || BoxError::from("SSH connection cache entry disappeared"),
        );
    }

    // Otherwise create a new connection
    let server_params = params.query(server);
    let hostname = server_params.host_name.ok_or_else(|| {
        BoxError::from(format!(
            "No hostname configured for SSH host '{server}'"
        ))
    })?;
    let port = server_params.port.unwrap_or(22);
    let user = match server_params.user {
        Some(user) => user,
        None => get_default_username()?,
    };

    // ProxyJump is a parsed ssh2-config field. Reading it from
    // `unsupported_fields` ignores normal ProxyJump entries and attempts a
    // direct TCP connection instead.
    let io = match server_params.proxy_jump.as_ref() {
        None => match server_params.unsupported_fields.get("proxycommand") {
            None => {
                Io::Tcp(TcpStream::connect((hostname.to_owned(), port)).await?)
            }
            Some(args) => {
                let command_name = args.first().ok_or_else(|| {
                    BoxError::from("SSH ProxyCommand is empty")
                })?;
                let mut command = Command::new(command_name);
                for arg in args[1..].iter() {
                    let arg = arg
                        // Replace %% with %
                        .replace("%%", "%")
                        // Replace the following placeholders with actual values
                        .replace("%h", &hostname)
                        .replace("%p", &port.to_string());
                    command.arg(arg);
                }
                info!("Executing proxy command: {command:?}");
                Io::Proxy(ProxyCommand::new(
                    command
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped()),
                )?)
            }
        },
        Some(jump) => {
            let jump_server = jump
                .first()
                .ok_or_else(|| BoxError::from("No SSH jump host specified"))?;
            let jump_client = connect_server_inner(
                jump_server,
                params,
                connection_map.clone(),
            )
            .await?;
            let channel_config = ChannelConfig::default();
            let origin_addr = ("127.0.0.1".into(), 0);
            let (tunnel, tunnel_rx) = jump_client
                .connect_tunnel(
                    channel_config,
                    (hostname.to_owned(), port),
                    origin_addr,
                )
                .await
                .map_err(|error| {
                    BoxError::from(format!(
                        "Could not open SSH tunnel: {error}"
                    ))
                })?;
            Io::Tunnel(TunnelStream::new(tunnel, tunnel_rx))
        }
    };

    debug!(server, hostname, port, "Opening SSH client transport");
    let config = ClientConfig::default();
    let (client, client_rx) = match io {
        Io::Tcp(socket) => {
            let (client, client_rx, client_fut) = Client::open(socket, config)?;
            tokio::spawn(async move {
                if let Err(error) = client_fut.await {
                    warn!("SSH client connection closed: {error}");
                }
            });
            (client, client_rx)
        }
        Io::Tunnel(io) => {
            let (client, client_rx, client_fut) = Client::open(io, config)?;
            tokio::spawn(async move {
                if let Err(error) = client_fut.await {
                    warn!("SSH client connection closed: {error}");
                }
            });
            (client, client_rx)
        }
        Io::Proxy(io) => {
            let (client, client_rx, client_fut) = Client::open(io, config)?;
            tokio::spawn(async move {
                if let Err(error) = client_fut.await {
                    warn!("SSH client connection closed: {error}");
                }
            });
            (client, client_rx)
        }
    };

    tokio::spawn(async move {
        if let Err(error) = authenticate_server(client_rx, hostname, port).await
        {
            warn!("SSH host verification failed: {error}");
        }
    });

    let ssh_folder = dirs::home_dir()
        .ok_or_else(|| {
            BoxError::from("Could not determine the home directory")
        })?
        .join(".ssh");
    debug!(server, "Selecting SSH identity file");
    let mut decoded_privkey = None;
    let configured_identity_files =
        server_params.identity_file.unwrap_or_else(|| {
            vec![ssh_folder.join("id_rsa"), ssh_folder.join("id_ed25519")]
        });
    // OpenSSH's `IdentityFile none` clears inherited identities. ssh2-config
    // preserves it as a path, so apply the reset before trying keys.
    let identity_files = configured_identity_files
        .iter()
        .rposition(|path| path == std::path::Path::new("none"))
        .map(|index| configured_identity_files[index + 1..].to_vec())
        .unwrap_or(configured_identity_files);
    for file in identity_files.iter() {
        let filename = file.as_os_str();
        if let Ok(privkey) = tokio::fs::read(file).await {
            if let Ok(passphrase) = std::env::var("SSH_PASSPHRASE") {
                info!("Decoding private key {:?} with passphrase", &filename);
                if let Ok(res) = makiko::keys::decode_pem_privkey(
                    &privkey,
                    passphrase.as_bytes(),
                ) {
                    decoded_privkey = Some(res);
                    break;
                } else {
                    info!(
                        "Could not decode a private key from pem {:?}",
                        &filename
                    );
                    continue;
                }
            } else if let Ok(privkey) = std::fs::read(file) {
                if let Ok(data) =
                    makiko::keys::decode_pem_privkey_nopass(&privkey)
                {
                    if let Some(key) = data.privkey().cloned() {
                        info!(
                            "Successfully decoded a private key {:?} without passphrase",
                            &filename
                        );
                        decoded_privkey = Some(key);
                        break;
                    }
                } else {
                    info!(
                        "Could not decode a private key from pem {:?}",
                        &filename
                    );
                    continue;
                }
            } else {
                info!("Identity file not found {:?}", &filename);
                continue;
            };
        }
    }
    let privkey = decoded_privkey.ok_or_else(|| {
        BoxError::from(
            "None of the configured SSH identity files could be decoded",
        )
    })?;
    debug!(server, user, "Authenticating SSH client");
    authenticate_by_private_key(&client, &user, &privkey).await?;
    debug!(server, "SSH client authenticated");

    connection_map
        .lock()
        .await
        .insert(server.to_string(), client.clone());

    Ok(client)
}

impl TunnelledTcp {
    pub async fn connect(&self) -> Result<TunnelReceiver, BoxError> {
        let params = get_params()?;

        let target_client =
            connect_server(&self.jump, &params, CONNECTION_MAP.clone())
                .await
                .map_err(|e| {
                    let msg = format!(
                        "Could not connect to jump host {}: {}",
                        self.jump, e
                    );
                    BoxError::from(msg)
                })?;

        let channel_config = makiko::ChannelConfig::default();
        let connect_addr = (self.address.to_owned(), self.port);
        let origin_addr = ("0.0.0.0".into(), 0);

        let (_tunnel, tunnel_rx) = target_client
            .connect_tunnel(channel_config, connect_addr.clone(), origin_addr)
            .await
            .map_err(|error| {
                BoxError::from(format!(
                    "Could not open a tunnel to {connect_addr:?}: {error}"
                ))
            })?;

        Ok(tunnel_rx)
    }
}

impl TunnelledWebsocket {
    pub async fn connect(&self) -> Result<TunnelStream, BoxError> {
        let params = get_params()?;

        let target_client =
            connect_server(&self.jump, &params, CONNECTION_MAP.clone())
                .await
                .map_err(|e| {
                    let msg = format!(
                        "Could not connect to jump host {}: {}",
                        self.jump, e
                    );
                    BoxError::from(msg)
                })?;

        let channel_config = makiko::ChannelConfig::default();
        let connect_addr = (self.address.to_owned(), self.port);
        let origin_addr = ("0.0.0.0".into(), 0);

        let (tunnel, tunnel_rx) = target_client
            .connect_tunnel(channel_config, connect_addr.clone(), origin_addr)
            .await
            .map_err(|error| {
                BoxError::from(format!(
                    "Could not open a tunnel to {connect_addr:?}: {error}"
                ))
            })?;

        Ok(TunnelStream::new(tunnel, tunnel_rx))
    }
}

impl TunnelledSero {
    pub async fn connect(&self) -> Result<TunnelStream, BoxError> {
        let params = get_params()?;
        let target_client =
            connect_server(&self.jump, &params, CONNECTION_MAP.clone())
                .await
                .map_err(|error| {
                    BoxError::from(format!(
                        "Could not connect to Sero jump host {}: {error}",
                        self.jump
                    ))
                })?;
        let channel_config = makiko::ChannelConfig::default();
        let connect_addr = ("api.secureadsb.com".to_string(), 4201);
        let origin_addr = ("0.0.0.0".into(), 0);
        let (tunnel, tunnel_rx) = target_client
            .connect_tunnel(channel_config, connect_addr, origin_addr)
            .await
            .map_err(|error| {
                BoxError::from(format!(
                    "Could not open a tunnel to api.secureadsb.com: {error}"
                ))
            })?;

        Ok(TunnelStream::new(tunnel, tunnel_rx))
    }
}

#[derive(Debug)]
pub struct ProxyCommand {
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl ProxyCommand {
    pub fn new(command: &mut Command) -> Result<Self, BoxError> {
        let mut command = command.spawn().map_err(|error| {
            BoxError::from(format!("Failed to spawn SSH ProxyCommand: {error}"))
        })?;
        let stdin = command.stdin.take().ok_or_else(|| {
            BoxError::from("SSH ProxyCommand did not provide stdin")
        })?;
        let stdout = command.stdout.take().ok_or_else(|| {
            BoxError::from("SSH ProxyCommand did not provide stdout")
        })?;
        Ok(ProxyCommand { stdin, stdout })
    }
}

impl AsyncRead for ProxyCommand {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.stdout).poll_read(cx, buf)
    }
}

impl AsyncWrite for ProxyCommand {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.stdin).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.stdin).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.stdin).poll_shutdown(cx)
    }
}
