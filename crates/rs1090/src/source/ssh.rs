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
use tracing::{info, warn};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub static CONNECTION_MAP: Lazy<Arc<Mutex<HashMap<String, Client>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

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
) {
    let mut hosts_path = dirs::home_dir().unwrap();
    hosts_path.push(".ssh");
    hosts_path.push("known_hosts");

    let hosts_data =
        std::fs::read(&hosts_path).expect("Could not read known_hosts file");

    let mut hosts_file = makiko::host_file::File::decode(hosts_data.into());

    loop {
        // Wait for the next event.
        let event = client_rx
            .recv()
            .await
            .expect("Error while receiving client event");

        // Exit the loop when the client has closed.
        let Some(event) = event else { break };

        if let makiko::ClientEvent::ServerPubkey(pubkey, accept) = event {
            info!(
                "Server pubkey type {}, fingerprint {}",
                pubkey.type_str(),
                pubkey.fingerprint()
            );

            match hosts_file.match_host_port_key(&host, port, &pubkey) {
                makiko::host_file::KeyMatch::Accepted(entries) => {
                    info!("Found the server key in known_hosts file");
                    for entry in entries.iter() {
                        info!("At line {}", entry.line());
                    }
                    accept.accept();
                }
                makiko::host_file::KeyMatch::Revoked(_entry) => {
                    panic!("The server key was revoked in known_hosts file");
                }
                makiko::host_file::KeyMatch::OtherKeys(entries) => {
                    warn!("The known_hosts file specifies other keys for this server:");
                    for entry in entries.iter() {
                        println!(
                            "At line {}, pubkey type {}, fingerprint {}",
                            entry.line(),
                            entry.pubkey().type_str(),
                            entry.pubkey().fingerprint()
                        );
                    }
                    panic!("Aborting, you might be target of a man-in-the-middle attack!");
                }
                makiko::host_file::KeyMatch::NotFound => {
                    info!("Did not find any key for this server in known_hosts file, \
                            adding it to the file");

                    accept.accept();

                    hosts_file.append_entry(
                        makiko::host_file::File::entry_builder()
                            .host_port(&host, port)
                            .key(pubkey),
                    );
                    let hosts_data = hosts_file.encode();
                    std::fs::write(&hosts_path, &hosts_data).expect(
                        "Could not write the modified known_hosts file",
                    );
                }
            }
        }
    }
}

async fn authenticate_by_private_key(
    client: &Client,
    user: &str,
    privkey: &Privkey,
) {
    let pubkey = privkey.pubkey();
    for pubkey_algo in pubkey.algos().iter().copied() {
        // Check whether this combination of a public key and algorithm would be
        // acceptable to the server.
        if client
            .check_pubkey(user.to_string(), &pubkey, pubkey_algo)
            .await
            .expect("Error when checking a public key")
        {
            // Try to authenticate with the private key
            let auth_res = client
                .auth_pubkey(user.to_string(), privkey.clone(), pubkey_algo)
                .await
                .expect("Error when trying to authenticate");

            // Deal with the possible outcomes of public key authentication.
            match auth_res {
                makiko::AuthPubkeyResult::Success => {
                    info!("We have successfully authenticated using a private key");
                    return;
                }
                makiko::AuthPubkeyResult::Failure(failure) => {
                    info!(
                        "The server rejected authentication with {pubkey_algo:?}: {failure:?}"
                    );
                }
            }
        }
    }
    panic!("The server does not accept the public key");
}

fn get_params() -> SshConfig {
    let config_path = dirs::home_dir().unwrap().join(".ssh").join("config");

    let err_msg = format!("{config_path:?} does not exist");
    let mut reader = BufReader::new(File::open(&config_path).expect(&err_msg));

    SshConfig::default()
        .parse(
            &mut reader,
            ParseRule::ALLOW_UNKNOWN_FIELDS
                | ParseRule::ALLOW_UNSUPPORTED_FIELDS,
        )
        .unwrap_or_else(|_| {
            panic!("Failed to parse configuration file {config_path:?}")
        })
}

fn get_default_username() -> String {
    #[cfg(target_os = "windows")]
    let username = std::env::var("USERNAME").unwrap_or_else(|_| {
        panic!("Could not determine the current Windows user name")
    });
    #[cfg(not(target_os = "windows"))]
    let username = std::env::var("USER").unwrap_or_else(|_| {
        panic!("Could not determine the current user name")
    });
    username
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
#[async_recursion::async_recursion]
async fn connect_server(
    server: &str,
    params: &SshConfig,
    connection_map: Arc<Mutex<HashMap<String, Client>>>,
) -> Result<Client, BoxError> {
    // Check if the server is already connected
    // If so, return the existing connection
    if connection_map.lock().await.contains_key(server) {
        info!("Reusing existing connection to {server}");
        return Ok(connection_map.lock().await.get(server).unwrap().clone());
    }

    // Otherwise create a new connection
    let server_params = params.query(server);
    let hostname = server_params.host_name.unwrap();
    let port = server_params.port.unwrap_or(22);
    let user = server_params.user.unwrap_or(get_default_username());

    let io = match server_params.unsupported_fields.get("proxyjump") {
        None => match server_params.unsupported_fields.get("proxycommand") {
            None => {
                Io::Tcp(TcpStream::connect((hostname.to_owned(), port)).await?)
            }
            Some(args) => {
                let mut command = Command::new(args[0].clone());
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
                ))
            }
        },
        Some(jump) => {
            let jump_server = jump.first().expect("No jump host specified");
            let jump_client =
                connect_server(jump_server, params, connection_map.clone())
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
                .expect("Could not open a tunnel");
            Io::Tunnel(TunnelStream::new(tunnel, tunnel_rx))
        }
    };

    let config = ClientConfig::default();
    let (client, client_rx) = match io {
        Io::Tcp(socket) => {
            let (client, client_rx, client_fut) = Client::open(socket, config)?;
            tokio::spawn(async move {
                client_fut.await.expect("Error in client future");
            });
            (client, client_rx)
        }
        Io::Tunnel(io) => {
            let (client, client_rx, client_fut) = Client::open(io, config)?;
            tokio::spawn(async move {
                client_fut.await.expect("Error in client future");
            });
            (client, client_rx)
        }
        Io::Proxy(io) => {
            let (client, client_rx, client_fut) = Client::open(io, config)?;
            tokio::spawn(async move {
                client_fut.await.expect("Error in client future");
            });
            (client, client_rx)
        }
    };

    tokio::task::spawn(authenticate_server(client_rx, hostname, port));

    let ssh_folder = dirs::home_dir().unwrap().join(".ssh");
    let mut decoded_privkey = None;
    let identity_files = server_params.identity_file.unwrap_or_else(|| {
        vec![ssh_folder.join("id_rsa"), ssh_folder.join("id_ed25519")]
    });
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
    let privkey =
        decoded_privkey.expect("None of the identity files could be decoded");
    authenticate_by_private_key(&client, &user, &privkey).await;

    connection_map
        .lock()
        .await
        .insert(server.to_string(), client.clone());

    Ok(client)
}

impl TunnelledTcp {
    pub async fn connect(&self) -> Result<TunnelReceiver, BoxError> {
        let params = get_params();

        let target_client =
            connect_server(&self.jump, &params, CONNECTION_MAP.clone())
                .await
                .map_err(|e| {
                    let msg = format!(
                        "Could not connect to jump host {}: {}",
                        &self.jump, e
                    );
                    BoxError::from(msg)
                })?;

        let channel_config = makiko::ChannelConfig::default();
        let connect_addr = (self.address.to_owned(), self.port);
        let origin_addr = ("0.0.0.0".into(), 0);

        let err_msg = format!("Could not open a tunnel to {connect_addr:?}");
        let (_tunnel, tunnel_rx) = target_client
            .connect_tunnel(channel_config, connect_addr, origin_addr)
            .await
            .expect(&err_msg);

        Ok(tunnel_rx)
    }
}

impl TunnelledWebsocket {
    pub async fn connect(&self) -> Result<TunnelStream, BoxError> {
        let params = get_params();

        let target_client =
            connect_server(&self.jump, &params, CONNECTION_MAP.clone())
                .await
                .map_err(|e| {
                    let msg = format!(
                        "Could not connect to jump host {}: {}",
                        &self.jump, e
                    );
                    BoxError::from(msg)
                })?;

        let channel_config = makiko::ChannelConfig::default();
        let connect_addr = (self.address.to_owned(), self.port);
        let origin_addr = ("0.0.0.0".into(), 0);

        let err_msg = format!("Could not open a tunnel to {connect_addr:?}");
        let (tunnel, tunnel_rx) = target_client
            .connect_tunnel(channel_config, connect_addr, origin_addr)
            .await
            .expect(&err_msg);

        Ok(TunnelStream::new(tunnel, tunnel_rx))
    }
}

impl TunnelledSero {
    pub async fn connect(&self) -> TunnelStream {
        let params = get_params();

        let target_client =
            connect_server(&self.jump, &params, CONNECTION_MAP.clone())
                .await
                .expect("Could not connect to jump host");
        let channel_config = makiko::ChannelConfig::default();
        let connect_addr = ("api.secureadsb.com".to_string(), 4201);
        let origin_addr = ("0.0.0.0".into(), 0);

        let (tunnel, tunnel_rx) = target_client
            .connect_tunnel(channel_config, connect_addr, origin_addr)
            .await
            .expect("Could not open a tunnel to api.secureadsb.com");

        TunnelStream::new(tunnel, tunnel_rx)
    }
}

#[derive(Debug)]
pub struct ProxyCommand {
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl ProxyCommand {
    pub fn new(command: &mut Command) -> Self {
        let mut command = command.spawn().expect("failed to spawn");
        let stdin = command.stdin.take().expect("failed to open stdin");
        let stdout = command.stdout.take().expect("failed to open stdout");
        ProxyCommand { stdin, stdout }
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
