use std::{
	collections::HashMap,
	fmt::Display,
	net::{Ipv6Addr, SocketAddr},
	sync::Arc,
};

use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use log::{error, info, warn};
use once_cell::sync::Lazy;
use tokio::{
	fs::OpenOptions,
	io::AsyncReadExt,
	io::AsyncWriteExt,
	net::{TcpListener, TcpStream},
	sync::Mutex,
};

enum SyncAction {
	Upload,
}

struct BadSyncAction;

impl TryFrom<u8> for SyncAction {
	type Error = BadSyncAction;

	fn try_from(value: u8) -> Result<Self, Self::Error> {
		match value {
			0 => Ok(SyncAction::Upload),
			_ => Err(BadSyncAction),
		}
	}
}

pub const SERVER_PORT: u16 = 2121;
pub static PRIV_KEY: Lazy<Aes256Gcm> = Lazy::new(|| {
	Aes256Gcm::new_from_slice(&hex::decode(std::env::var("BLOG_ENC_KEY").unwrap()).unwrap())
		.unwrap()
});

#[allow(dead_code)]
pub async fn run_mirror_server() -> Result<()> {
	info!("Starting sync server");

	let sock = TcpListener::bind((Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0), SERVER_PORT)).await?;
	let expired_nonces = Arc::new(Mutex::new(HashMap::new()));

	loop {
		if let Ok((mut sock, addr)) = sock.accept().await {
			let expired_nonces = expired_nonces.clone();

			tokio::task::spawn(async move {
				// FIXME: unwrap
				let action_byte = match sock.read_u8().await {
					Ok(b) => b,
					Err(err) => match err.kind() {
						std::io::ErrorKind::UnexpectedEof => return,
						_ => {
							return;
						},
					},
				};
				if let Ok(action) = SyncAction::try_from(action_byte) {
					if let Err(err) = match action {
						SyncAction::Upload => {
							handle_upload(sock, addr, expired_nonces.clone()).await
						},
					} {
						match err.to_string().contains("Internal Server Error:") {
							true => error!("{err}"),
							false => info!("Error handling file upload: {err}"),
						}
					}
				} else {
					let _ = sock
						.write_all(format!("Bad sync action {action_byte}").as_bytes())
						.await;
				}
			});
		}
	}
}

struct NonceInfo {
	nonce: [u8; 12],
	time_seen: DateTime<Utc>,
}

async fn handle_upload(
	sock: TcpStream, _addr: SocketAddr,
	expired_nonces: Arc<Mutex<HashMap<blake3::Hash, NonceInfo>>>,
) -> Result<()> {
	let mut buffer = [0_u8; 8192];

	// Read at most 8192 bytes
	let mut handle = sock.take(8192);
	let mut n = 0;

	loop {
		match handle.read(&mut buffer[n..]).await {
			Ok(0) => break,
			Ok(s) => n += s,
			Err(err) => return Err(anyhow!("Error reading from socket: {err}")),
		};
	}

	let nonce = Nonce::from_slice(&buffer[..12]);

	let dec_data = PRIV_KEY
		.decrypt(nonce, &buffer[12..n])
		.map_err(|err| anyhow!("Error decrypting data (likely invalid private key): {err}"))?;

	let dec_data_hash = blake3::hash(&dec_data);

	{
		let expired_nonces = &mut expired_nonces.lock().await;

		if let Some(exp_nonce_info) = expired_nonces.get(&dec_data_hash) {
			if exp_nonce_info.nonce == buffer[..12] {
				warn!(
					"Replay attack attempted: nonce {} has been seen before at {} UTC",
					hex::encode(exp_nonce_info.nonce),
					exp_nonce_info.time_seen
				);
				return Err(anyhow!("Using expired nonce!"));
			}
		}

		let nonce_info = NonceInfo {
			nonce: buffer[..12].try_into().unwrap(),
			time_seen: Utc::now(),
		};
		expired_nonces.insert(dec_data_hash, nonce_info);
	}

	let file_name_len = dec_data[0] as usize;
	let file_name = String::from_utf8(dec_data[1..=file_name_len].to_vec())?;

	// no null bytes
	// FIXME: Why do we send so many null bytes?
	let md: Vec<u8> = dec_data[file_name_len + 1..]
		.iter()
		.filter(|b| **b != 0)
		.copied()
		.collect();

	let mut file = OpenOptions::new()
		.create(true)
		.truncate(true)
		.write(true)
		.open(format!("./md/{file_name}"))
		.await
		.map_err(internal_server_error)?;

	file.write_all(&md).await.map_err(internal_server_error)?;

	info!("Uploaded file {file_name}");

	Ok(())
}

#[inline]
fn internal_server_error<E: Display>(err: E) -> anyhow::Error {
	anyhow!("Internal Server Error: {err}")
}
