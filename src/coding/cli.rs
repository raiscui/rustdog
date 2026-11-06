use crate::coding::model::{CodingError, CodingReply, CodingRequest, ErrorCode, InteractionAnswer};
use crate::control_frames::ControlFrame;
use crate::control_invocation::{resolve_control_invocation, ControlInvocation};
use clap::{Args, Subcommand, ValueEnum};
use std::{
    io::{self, Read},
    path::PathBuf,
};

#[derive(Debug, Args)]
pub struct CodingCli {
    #[arg(long, global = true)]
    namespace: Option<String>,
    #[arg(long, global = true)]
    target_name: Option<String>,
    #[arg(long = "entry-point", global = true)]
    entry_point: Vec<String>,
    #[arg(long, global = true, value_name = "TARGET")]
    target: Option<String>,
    #[command(subcommand)]
    command: CodingCommand,
}

#[derive(Debug, Subcommand)]
enum CodingCommand {
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    Interaction {
        #[command(subcommand)]
        command: InteractionCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Open {
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        cwd: PathBuf,
        #[arg(long)]
        profile: Option<String>,
    },
    List,
    Status {
        session_id: String,
    },
    Close {
        session_id: String,
        #[arg(long)]
        request_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum TaskCommand {
    Submit {
        session_id: String,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    Status {
        session_id: String,
        task_id: String,
    },
    Output {
        session_id: String,
        task_id: String,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long)]
        limit: Option<u16>,
    },
    Cancel {
        session_id: String,
        task_id: String,
        #[arg(long)]
        request_id: String,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum AnswerKind {
    Confirm,
    Select,
    Input,
    Editor,
}

#[derive(Debug, Subcommand)]
enum InteractionCommand {
    List {
        session_id: String,
    },
    Respond {
        session_id: String,
        interaction_id: String,
        #[arg(long)]
        request_id: String,
        #[arg(long, value_enum)]
        kind: AnswerKind,
        #[arg(long, conflicts_with = "value")]
        confirmed: Option<bool>,
        #[arg(long)]
        value: Option<String>,
    },
}

impl CodingCli {
    fn request(command: CodingCommand) -> Result<CodingRequest, CodingError> {
        let request = match command {
            CodingCommand::Session { command } => match command {
                SessionCommand::Open {
                    request_id,
                    cwd,
                    profile,
                } => CodingRequest::SessionOpen {
                    request_id,
                    cwd,
                    profile,
                },
                SessionCommand::List => CodingRequest::SessionList {},
                SessionCommand::Status { session_id } => {
                    CodingRequest::SessionStatus { session_id }
                }
                SessionCommand::Close {
                    session_id,
                    request_id,
                } => CodingRequest::SessionClose {
                    session_id,
                    request_id,
                },
            },
            CodingCommand::Task { command } => match command {
                TaskCommand::Submit {
                    session_id,
                    request_id,
                    file,
                } => {
                    let mut prompt = String::new();
                    match file {
                        Some(path) => std::fs::File::open(path)?
                            .take(128 * 1024 + 1)
                            .read_to_string(&mut prompt)?,
                        None => io::stdin()
                            .lock()
                            .take(128 * 1024 + 1)
                            .read_to_string(&mut prompt)?,
                    };
                    CodingRequest::TaskSubmit {
                        session_id,
                        request_id,
                        prompt,
                    }
                }
                TaskCommand::Status {
                    session_id,
                    task_id,
                } => CodingRequest::TaskStatus {
                    session_id,
                    task_id,
                },
                TaskCommand::Output {
                    session_id,
                    task_id,
                    cursor,
                    limit,
                } => CodingRequest::TaskOutput {
                    session_id,
                    task_id,
                    cursor,
                    limit,
                },
                TaskCommand::Cancel {
                    session_id,
                    task_id,
                    request_id,
                } => CodingRequest::TaskCancel {
                    session_id,
                    task_id,
                    request_id,
                },
            },
            CodingCommand::Interaction { command } => match command {
                InteractionCommand::List { session_id } => {
                    CodingRequest::InteractionList { session_id }
                }
                InteractionCommand::Respond {
                    session_id,
                    interaction_id,
                    request_id,
                    kind,
                    confirmed,
                    value,
                } => {
                    let answer = match kind {
                        AnswerKind::Confirm => InteractionAnswer::Confirm {
                            confirmed: confirmed.ok_or_else(|| {
                                CodingError::new(
                                    ErrorCode::InvalidRequest,
                                    "confirm requires --confirmed",
                                )
                            })?,
                        },
                        AnswerKind::Select if confirmed.is_none() => {
                            InteractionAnswer::Select { value }
                        }
                        AnswerKind::Input if confirmed.is_none() => {
                            InteractionAnswer::Input { value }
                        }
                        AnswerKind::Editor if confirmed.is_none() => {
                            InteractionAnswer::Editor { value }
                        }
                        _ => {
                            return Err(CodingError::new(
                                ErrorCode::InvalidRequest,
                                "--confirmed only applies to confirm",
                            ))
                        }
                    };
                    CodingRequest::InteractionRespond {
                        session_id,
                        interaction_id,
                        request_id,
                        answer,
                    }
                }
            },
        };
        request.validate()?;
        Ok(request)
    }
}

pub fn run(options: CodingCli) -> Result<(), String> {
    let result = run_inner(options).unwrap_or_else(CodingReply::failure);
    let ok = result.is_ok();
    let json = serde_json::to_string(&result).map_err(|err| err.to_string())?;
    println!("{json}");
    if ok {
        Ok(())
    } else {
        Err("coding request failed".to_owned())
    }
}

fn run_inner(options: CodingCli) -> Result<CodingReply, CodingError> {
    let request = CodingCli::request(options.command)?;
    let invocation = resolve_control_invocation(
        None,
        None,
        options.namespace,
        options.target_name,
        options.entry_point,
        options.target.into_iter().collect(),
    )
    .map_err(|err| CodingError::new(ErrorCode::InvalidRequest, err))?;
    let (namespace, target, endpoints) = match invocation {
        ControlInvocation::Zenoh {
            namespace,
            target_name,
            entry_point,
        } => (namespace, target_name, entry_point),
        ControlInvocation::ZenohLocal { namespace } => {
            let target = crate::zenoh_runtime::find_local_daemon_name(namespace.as_deref())?;
            let namespace = namespace
                .or_else(|| crate::zenoh_identity::infer_namespace_from_daemon_name(&target));
            (namespace, Some(target), Vec::new())
        }
        _ => {
            return Err(CodingError::new(
                ErrorCode::AuthenticationRequired,
                "coding requires Zenoh",
            ))
        }
    };
    let payload = serde_json::to_string(&request)?;
    if payload.len() > 256 * 1024 {
        return Err(CodingError::new(
            ErrorCode::InvalidRequest,
            "coding request exceeds wire limit",
        ));
    }
    let line = format!("@coding:{payload}");
    let frames = crate::zenoh_control::send_control_lines_collect_frames(
        namespace,
        target,
        endpoints,
        5000,
        &[line],
    )
    .map_err(|err| {
        CodingError::new(
            ErrorCode::UnknownOutcome,
            format!("control transport failed: {err}"),
        )
    })?;
    let mut responses = frames
        .into_iter()
        .flatten()
        .filter_map(|frame| match frame {
            ControlFrame::ResponseLine(line) => Some(line),
            _ => None,
        });
    let response = responses
        .next()
        .ok_or_else(|| CodingError::new(ErrorCode::UnknownOutcome, "missing coding response"))?;
    if responses.next().is_some() {
        return Err(CodingError::new(
            ErrorCode::UnknownOutcome,
            "multiple coding responses",
        ));
    }
    let json = response
        .strip_prefix("@response ")
        .ok_or_else(|| CodingError::new(ErrorCode::UnknownOutcome, "invalid control response"))?;
    serde_json::from_str(json).map_err(|err| {
        CodingError::new(
            ErrorCode::UnknownOutcome,
            format!("invalid coding response: {err}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn nested_commands_preserve_requests_and_global_target() {
        let opts = crate::input::Opts::try_parse_from([
            "rdog",
            "coding",
            "task",
            "output",
            "session_1",
            "task_1",
            "--cursor",
            "next",
            "--limit",
            "12",
            "--target-name",
            "worker.lab",
        ])
        .unwrap();
        let crate::input::Command::Coding { options } = opts.command else {
            panic!("expected coding");
        };
        assert_eq!(options.target_name.as_deref(), Some("worker.lab"));
        let request = CodingCli::request(options.command).unwrap();
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["op"], "task.output");
        assert_eq!(value["cursor"], "next");
        assert_eq!(value["limit"], 12);
    }

    #[test]
    fn confirm_requires_explicit_decision() {
        let command = CodingCommand::Interaction {
            command: InteractionCommand::Respond {
                session_id: "s".into(),
                interaction_id: "i".into(),
                request_id: "r".into(),
                kind: AnswerKind::Confirm,
                confirmed: None,
                value: None,
            },
        };
        assert!(CodingCli::request(command).is_err());
    }
}
