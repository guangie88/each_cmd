#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate futures_cpupool;

#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate structopt;

#[macro_use]
extern crate structopt_derive;
extern crate tokio_timer;

use std::fs::File;
use futures::Future;
use futures_cpupool::CpuPool;
use std::io::{self, Read, Write};
use std::process::{self, Command, Output};
use std::time::Duration;
use structopt::StructOpt;
use tokio_timer::Timer;

mod errors {
    error_chain! {
        errors {
            CommandLaunch {
                description("command launch error")
                display("command launch error")
            }
            Timeout {
                description("execution timeout")
                display("execution timeout")
            }
        }
    }
}

use errors::*;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Config {
    hostnames: Vec<String>,
    cmd_to_run: String,
    hostname_tag: String,
    thread_count: usize,
    timeout_ms: u64,
}

#[derive(StructOpt, Debug)]
#[structopt(name = "Each Command Program", about = "Runs command on given list of hostnames.")]
struct MainArgMap {
    #[structopt(short = "c", long = "config", help = "Path to config file")]
    config_path: String,
}

fn run_cmd(cmd: &str) -> Result<Output> {
    if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(&["/C", cmd])
            .output()
    } else {
        Command::new("sh")
            .args(&["-c", cmd])
            .output()
    }
    .chain_err(|| ErrorKind::CommandLaunch)
}

fn run() -> Result<()> {
    // reads the configuration
    let main_arg_map = MainArgMap::from_args();

    let config_content = {
        let mut config_file = File::open(&main_arg_map.config_path)
            .chain_err(|| format!("Unable to open config file at {}", main_arg_map.config_path))?;

        let mut buf = String::new();
        let _ = config_file.read_to_string(&mut buf)
            .chain_err(|| "Unable to read config file into string")?;

        buf
    };
    
    let config: Config = serde_json::from_str(&config_content)
        .chain_err(|| "Unable to parse config content into structure!")?;

    // executes the command for each given hostname
    let pool = CpuPool::new(config.thread_count);
    let timeout = Duration::from_millis(config.timeout_ms);

    let exec_futs: Vec<_> = config.hostnames.iter()
        .map(|hostname| {
            let hostname = hostname.to_owned();
            let cmd_to_run = config.cmd_to_run.replace(&config.hostname_tag, &hostname);

            // timeout + action
            let timer = Timer::default();

            let action_fut = pool.spawn_fn(move || {
                println!("Running command: {}", cmd_to_run);
                run_cmd(&cmd_to_run)
            });

            timer.sleep(timeout)
                .then(|_| bail!(ErrorKind::Timeout))
                .select(action_fut)
                .map(|(win, _)| win)
        })
        .collect();

    let stderr = &mut io::stderr();

    for exec_fut in exec_futs.into_iter() {
        match exec_fut.wait() {
            Ok(output) => {
                println!("Command completion: [stdout: '{}', stderr: '{}']",
                    String::from_utf8_lossy(&output.stdout).trim(),
                    String::from_utf8_lossy(&output.stderr).trim());
            },

            Err((e, _)) => {
                let _ = writeln!(stderr, "Command error: {}", e);
            },
        }
    }

    Ok(())
}

fn main() {
    match run() {
        Ok(_) => {
            println!("Program completed!");
            process::exit(0)
        },
        Err(ref e) => {
            let stderr = &mut io::stderr();

            writeln!(stderr, "Error: {}", e)
                .expect("Unable to write error into stderr!");

            for e in e.iter().skip(1) {
                writeln!(stderr, "- Caused by: {}", e)
                    .expect("Unable to write error causes into stderr!");
            }

            process::exit(1);
        },
    }
}
