use std::{
    collections::HashSet,
    fs::{
        File,
        OpenOptions,
    },
    io::{
        Read,
        Seek,
        Write,
    },
    str::FromStr,
};

use anyhow::{
    anyhow,
    Context,
};
use clap::Parser;
use ipnetwork::IpNetwork;
use serde::Deserialize;

const GITHUB_API_META_URL: &str = "https://api.github.com/meta";
const ACCEPT_HEADER_VALUE: &str = "application/vnd.github+json";

#[derive(Debug)]
#[derive(Parser)]
struct Args {
    /// Path to config file
    config: String,
}

#[derive(Deserialize)]
struct Config {
    /// GitHub API token
    pub token:             String,
    /// Path to file where Nginx allow list show be written
    pub allow_file:        String,
    /// Time interval in seconds between checks
    pub repeat:            u64,
    /// Command to execute after allow lsit change
    pub after_update_hook: String,
}

impl Config {
    pub fn read_from_file(file_path: &str) -> Result<Self, ConfigReadError> {
        let file_content = std::fs::read_to_string(file_path)?;

        let config = toml::from_str(&file_content)?;

        Ok(config)
    }
}

#[derive(Debug)]
#[derive(thiserror::Error)]
enum ConfigReadError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Parse(#[from] toml::de::Error),
}

#[derive(Deserialize)]
struct MetaInfo {
    pub hooks: Vec<IpNetwork>,
}

#[derive(Debug)]
struct AllowList {
    file_handler: File,
    allow_list:   HashSet<IpNetwork>,
}

impl AllowList {
    pub fn load(file_path: &str) -> Result<Self, std::io::Error> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(file_path)?;

        let mut content = String::new();

        file.read_to_string(&mut content)?;

        let mut allow_list = HashSet::new();

        for (line_number, line) in content.lines().enumerate() {
            let line = line.replace(';', "");
            let line = line.replace("allow ", "");
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            let cidr = match IpNetwork::from_str(line) {
                Ok(cidr) => cidr,
                Err(err) => {
                    log::error!(
                        "Failed to parse CIDR [{}] at line {}, cause: {:#}, skipping rest of the \
                         file",
                        line,
                        line_number,
                        err
                    );
                    allow_list.clear();
                    break;
                }
            };

            allow_list.insert(cidr);
        }

        Ok(Self {
            file_handler: file,
            allow_list,
        })
    }

    pub fn update(&mut self, new_allow_list: HashSet<IpNetwork>) -> std::io::Result<bool> {
        if self.allow_list != new_allow_list {
            self.allow_list = new_allow_list;
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        self.file_handler.set_len(0)?;
        self.file_handler.seek(std::io::SeekFrom::Start(0))?;

        for cidr in &self.allow_list {
            self.file_handler
                .write_fmt(format_args!("allow {};\n", cidr))?;
        }

        Ok(())
    }
}

#[derive(Debug)]
#[derive(thiserror::Error)]
enum AllowFileLoadError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Parse(#[from] ipnetwork::IpNetworkError),
}

fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    env_logger::init();

    let config: Config = Config::read_from_file(&args.config)
        .with_context(|| anyhow!("Failed to read configuration"))?;

    let mut allow_file = AllowList::load(&config.allow_file)
        .with_context(|| anyhow!("Failed to load allow list"))?;

    let authorization_header_value = format!("token {}", config.token);

    loop {
        match update_cycle(
            &authorization_header_value,
            &mut allow_file,
            &config.after_update_hook,
        ) {
            Ok(is_changed) => {
                log::info!("Update cycle completed");
                if is_changed {
                    log::info!("Allow list is CHANGED");
                } else {
                    log::info!("Allow list is UNCHANGED");
                }
            }
            Err(err) => log::error!("Update cycle failed. {:#}", err),
        };
        std::thread::sleep(std::time::Duration::from_secs(config.repeat));
    }

    Ok(())
}

fn try_fetch(authorization_header_value: &str) -> Result<HashSet<IpNetwork>, anyhow::Error> {
    let client = reqwest::blocking::Client::new();

    let response = client
        .request(reqwest::Method::GET, GITHUB_API_META_URL)
        .header(reqwest::header::ACCEPT, ACCEPT_HEADER_VALUE)
        .header(reqwest::header::AUTHORIZATION, authorization_header_value)
        .header(reqwest::header::USER_AGENT, "reqwest")
        .send()
        .with_context(|| anyhow!("Failed to fetch GitHub meta information"))?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "GitHub API responded with code {}, text: {}",
            response.status().as_u16(),
            response.text().unwrap_or_default()
        ));
    }

    let meta_info: MetaInfo = response
        .json()
        .with_context(|| anyhow!("Failed to deserialize GitHub meta information"))?;

    Ok(HashSet::from_iter(meta_info.hooks.into_iter()))
}

fn update_cycle(
    authorization_header_value: &str,
    allow_list: &mut AllowList,
    after_update_hook: &str,
) -> Result<bool, anyhow::Error> {
    let hook_server_ips = try_fetch(authorization_header_value)
        .with_context(|| anyhow!("Failed to get hook server ip addresses"))?;

    if allow_list.update(hook_server_ips)? {
        execute_after_update_hook(after_update_hook)
            .with_context(|| anyhow!("Failed to execute after update hook"))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn execute_after_update_hook(after_update_hook: &str) -> Result<(), anyhow::Error> {
    let exit_code = std::process::Command::new("bash")
        .arg("-c")
        .arg(after_update_hook)
        .status()
        .with_context(|| anyhow!("Failed to run after_update_hook"))?;

    match exit_code.code() {
        Some(0) => Ok(()),
        Some(code) => Err(anyhow!("after_update_hook exited with non zero code")),
        None => Err(anyhow!("Failed to get after_update_hook exit code")),
    }
}
