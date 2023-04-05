mod sync;

use aes_gcm::{
	aead::{Aead, OsRng},
	Aes256Gcm, KeyInit, Nonce,
};
use anyhow::{anyhow, Error, Result};
use rand::prelude::*;
use std::{
	env::args,
	fs::DirEntry,
	net::IpAddr,
	sync::atomic::{AtomicUsize, Ordering},
	time::Duration,
};
use tokio::{fs, io::AsyncWriteExt, net::TcpStream};

use sync::{PRIV_KEY, SERVER_PORT};

static NUM_COMPLETED: AtomicUsize = AtomicUsize::new(0);

#[tokio::main]
async fn main() -> Result<()> {
	let mut args = args();
	args.next().unwrap();

	let arg = args.next().ok_or_else(help)?;

	if arg == "gen_key" {
		println!("{}", gen_private_key());
		return Ok(());
	}

	let server_ip: IpAddr = arg.parse()?;

	let files: Vec<_> = std::fs::read_dir("./md/")?
		.flatten()
		.map(|file| send_file(file, server_ip))
		.collect();

	let num_to_complete = files.len();

	files.into_iter().for_each(|task| {
		tokio::spawn(async move {
			if let Err(e) = task.await {
				println!("Error running task: {e}");
			};
			NUM_COMPLETED.fetch_add(1, Ordering::Relaxed);
		});
	});

	while num_to_complete != NUM_COMPLETED.load(Ordering::Relaxed) {
		println!("Sending...");
		tokio::time::sleep(Duration::from_millis(100)).await;
	}

	println!("Done");

	Ok(())
}

async fn send_file(file: DirEntry, server_ip: IpAddr) -> Result<()> {
	let file_name = file.file_name();
	let file_name = file_name
		.to_str()
		.ok_or_else(|| anyhow!("File name not UTF-8: {file:?}"))?;
	let mut file_name_bytes: [u8; 256] = [0; 256];
	file_name_bytes[..file_name.as_bytes().len()].copy_from_slice(file_name.as_bytes());

	let file_path = file.path();

	let mut nonce_bytes: [u8; 12] = [0; 12];
	rand::thread_rng().fill(&mut nonce_bytes);

	let md = fs::read_to_string(file_path).await?;

	let mut msg = Vec::with_capacity(file_name_bytes.len() + md.len());
	msg.push(file_name.as_bytes().len().try_into()?);

	msg.extend_from_slice(&file_name_bytes);
	msg.extend_from_slice(md.as_bytes());

	let enc_bytes = PRIV_KEY
		.encrypt(Nonce::from_slice(&nonce_bytes), msg.as_ref())
		.map_err(|e| anyhow!("{e}"))?;

	// Used to test nonce is working (to prevent replay attacks)
	for _ in 0..3 {
		let mut conn = TcpStream::connect((server_ip, SERVER_PORT)).await?;

		// Ensure the total message is less than 8192 bytes
		if 1 + 1 + nonce_bytes.len() + enc_bytes.len() > 8192 {
			panic!("File {file_name} too big");
		}

		conn.write_u8(0).await?;
		conn.write_all(&nonce_bytes).await?;
		conn.write_all(&enc_bytes).await?;
	}

	println!("Sent file {file_name}");

	Ok(())
}

fn help() -> Error {
	anyhow!("Usage: ./sync_md <server_ip|gen_key>")
}

fn gen_private_key() -> String {
	let priv_key = Aes256Gcm::generate_key(&mut OsRng);
	hex::encode(priv_key)
}
