mod config;

use crate::config::TargetKind;
use clap::Parser;
use cross_exec::CommandExt;
use regex::{Regex, RegexBuilder};
use skim::prelude::*;
use skim::tui::options::TuiLayout;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Warpgate CLI
#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    /// Config file path, use env vars between %, e.g. %ONEDRIVE%
    #[arg(
        id = "config",
        long,
        default_value = "%ONEDRIVE%\\warpgate-cli-config.json",
        env = "WARPGATE_CLI_CONFIG_PATH"
    )]
    config_path: String,

    /// Force a refresh of Warpgate API Token
    #[arg(id = "refresh-token", long, default_value_t = false)]
    force_refresh_token: bool,
}

fn main() {
    let Args {
        config_path: config_path_raw,
        force_refresh_token,
    } = Args::parse();

    let config_path_string = parse_env_vars(config_path_raw);
    let config_path = Path::new(&config_path_string);

    let mut config = config::get_config(config_path);

    let wg_info = config.fetch_info(force_refresh_token, config_path);
    let targets = config.fetch_targets(false, config_path);

    let (selected_name, selected_kind) = skim_select(targets);

    match selected_kind {
        TargetKind::Http => {
            println!("This target is an HTTP Target, to access it, use this link :");
            println!(
                "{}/?warpgate_target={}",
                config.warpgate_url(),
                selected_name
            );
        }
        TargetKind::Ssh => {
            let proto = wg_info.get_protocol_info(&selected_kind).unwrap();
            let _ = Command::new("ssh")
                .arg(format!(
                    "{}:{}@{}",
                    wg_info.username(),
                    selected_name,
                    proto.host()
                ))
                .arg("-p")
                .arg(proto.port().to_string())
                .cross_exec();
            panic!("Failed to execute SSH command");
        }
        _ => panic!("Unsupported Target"),
    }
}

fn parse_env_vars(s: String) -> String {
    let mut result = s;
    for (key, value) in std::env::vars() {
        let (key, value) = (regex::escape(&key), regex::escape(&value));

        let regex = RegexBuilder::new(format!(r"%{}%", key).as_str())
            .case_insensitive(true)
            .build()
            .expect("Failed to create env var regex");

        result = Regex::replace_all(&regex, &result, value).to_string();
    }
    result
}

fn skim_select(targets: HashMap<String, TargetKind>) -> (String, TargetKind) {
    let options = SkimOptionsBuilder::default()
        .layout(TuiLayout::Reverse)
        .prompt("Select a Target : ")
        .info("inline-right")
        .highlight_line(true)
        .multi(false)
        .build()
        .unwrap();

    println!();
    let target_keys = {
        let mut t = targets.keys().cloned().collect::<Vec<_>>();
        t.sort();
        t
    };

    let skim_output = Skim::run_items(options, target_keys).unwrap();

    if skim_output.is_abort {
        println!("Aborted!");
        std::process::exit(0);
    }

    let selected_key = skim_output.selected_items.first().unwrap().item.to_string();
    let target_kind = targets.get(&selected_key).unwrap();

    (selected_key, *target_kind)
}
