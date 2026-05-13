use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::provider::{AgentProvider, ProviderInfo, ProviderKind, SendOptions};
use crate::AgentEvent;

pub struct CodexCli;

impl CodexCli {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentProvider for CodexCli {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "codex-cli".into(),
            name: "Codex (CLI)".into(),
            kind: ProviderKind::Subprocess,
            description: "Spawns the `codex` CLI in non-interactive mode and forwards stdout."
                .into(),
        }
    }

    async fn detect(&self) -> bool {
        which::which("codex").is_ok()
    }

    async fn run(
        &self,
        opts: SendOptions,
        tx: mpsc::Sender<AgentEvent>,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        let session_id = opts.session_id.clone();

        let extra_args: Vec<String> = opts
            .provider_config
            .get("extra_args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let mut cmd = Command::new("codex");
        // OpenAI Codex CLI: `codex exec "<prompt>"`.
        // - `--skip-git-repo-check`: workspace subdirs aren't git repos, codex
        //   would otherwise refuse to run.
        // - stdin redirected to null: without this codex waits on stdin and
        //   hangs forever in headless mode.
        cmd.arg("exec")
            .arg("--skip-git-repo-check")
            .arg(&opts.prompt)
            .args(&extra_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &opts.working_dir {
            cmd.current_dir(cwd);
        }

        let mut child = cmd.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("no stdout"))?;
        let mut reader = BufReader::new(stdout).lines();

        let _ = tx
            .send(AgentEvent::Started {
                session_id: session_id.clone(),
            })
            .await;

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = child.kill().await;
                    let _ = tx.send(AgentEvent::Finished {
                        session_id: session_id.clone(),
                        reason: "cancelled".into(),
                    }).await;
                    return Ok(());
                }
                line = reader.next_line() => {
                    match line? {
                        Some(l) => {
                            // Until we wire structured output, surface raw stdout as text.
                            let _ = tx.send(AgentEvent::Text {
                                session_id: session_id.clone(),
                                delta: format!("{}\n", l),
                            }).await;
                        }
                        None => break,
                    }
                }
            }
        }

        let status = child.wait().await?;
        let _ = tx
            .send(AgentEvent::Finished {
                session_id,
                reason: format!("exit:{}", status.code().unwrap_or(-1)),
            })
            .await;
        Ok(())
    }
}
