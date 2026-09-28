mod api;
mod args;
mod auth;
mod config;
mod error;
mod operations;

use args::{Brain, Command};
use error::{Error, Result};
use serde_json::{Value, json};

fn run(args: Brain) -> Result<Value> {
    let origin = config::origin(
        &args
            .origin
            .or_else(|| std::env::var("BRAIN_ORIGIN").ok())
            .unwrap_or_else(|| config::DEFAULT_ORIGIN.into()),
    )?;
    let project = config::project(args.project.as_deref())?;
    let bypass_selection = matches!(
        args.command,
        Command::Login(_)
            | Command::Logout
            | Command::Use(_)
            | Command::RequestId
            | Command::Account(_)
            | Command::Brains(args::Brains {
                command: args::BrainCommand::List
            })
    );
    let selected = if let Some(id) = args.brain {
        Some(config::identifier(&id, "org")?)
    } else if bypass_selection {
        None
    } else {
        config::selected(&project)?
    };
    let request_id = match &args.command {
        Command::Record(options)
        | Command::Knowledge(args::Knowledge {
            command: args::KnowledgeCommand::Record(options),
        }) => Some(options.request_id.clone()),
        Command::Brains(args::Brains {
            command: args::BrainCommand::Create(options),
        }) => Some(options.request_id.clone()),
        _ => None,
    };
    let mut context = operations::Context {
        api: api::Api::new(origin.clone())?,
        project,
        selected,
    };
    let result = context.execute(args.command);
    let metadata =
        json!({"origin": origin, "brain_id": context.selected, "request_id": request_id});
    match result {
        Ok(data) => {
            let status = data.get("status").and_then(Value::as_str);
            if matches!(
                status,
                Some("failed" | "cancelled" | "interrupted" | "not_saved")
            ) {
                let mut error = Error::new(
                    "run_failed",
                    "Recording did not save successfully. Inspect the run result before retrying.",
                );
                error.details = Some(json!({"context": metadata, "result": data}));
                return Err(error);
            }
            Ok(json!({"ok": true, "data": data, "context": metadata}))
        }
        Err(mut error) => {
            error.details = Some(json!({"context": metadata, "operation": error.details.take()}));
            Err(error)
        }
    }
}

fn main() {
    let words = std::env::args_os().skip(1).collect::<Vec<_>>();
    let args = match Brain::embedded_outcome(&words) {
        usage::embedded::Outcome::Parsed(args) => args,
        usage::embedded::Outcome::Exit(exit) => {
            if exit.code == 0 {
                print!("{}", exit.text);
            } else {
                println!(
                    "{}",
                    json!({"ok": false, "error": Error::invalid(exit.text)})
                );
            }
            std::process::exit(exit.code);
        }
    };
    if let Command::Completions(options) = &args.command {
        let shell = match options.shell.as_str() {
            "bash" => usage::complete::Shell::Bash,
            "fish" => usage::complete::Shell::Fish,
            _ => usage::complete::Shell::Zsh,
        };
        print!("{}", Brain::completion_script(shell));
        return;
    }
    let pretty = args.pretty;
    let (output, exit) = match run(args) {
        Ok(value) => (value, 0),
        Err(error) => {
            let exit = error.exit_code();
            (json!({"ok": false, "error": error}), exit)
        }
    };
    let encoded = if pretty {
        serde_json::to_string_pretty(&output)
    } else {
        serde_json::to_string(&output)
    };
    println!(
        "{}",
        encoded.unwrap_or_else(|_| "{\"ok\":false,\"error\":{\"code\":\"encoding\"}}".into())
    );
    std::process::exit(exit);
}
