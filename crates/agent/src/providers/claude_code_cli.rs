use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::provider::{AgentProvider, ProviderInfo, ProviderKind, SendOptions};
use crate::AgentEvent;

pub struct ClaudeCodeCli;

impl ClaudeCodeCli {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentProvider for ClaudeCodeCli {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "claude-code-cli".into(),
            name: "Claude Code (CLI)".into(),
            kind: ProviderKind::Subprocess,
            description: "Spawns the `claude` CLI in headless mode and streams JSONL output.".into(),
        }
    }

    async fn detect(&self) -> bool {
        which::which("claude").is_ok()
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
        let resume_id: Option<String> = opts
            .provider_config
            .get("resume")
            .and_then(|v| v.as_str())
            .map(String::from);
        // Headless mode has no human to answer permission prompts. Default to
        // bypassPermissions; user can pick a stricter mode via provider_config
        // (e.g. "acceptEdits" with --allowedTools in extra_args).
        let permission_mode: String = opts
            .provider_config
            .get("permission_mode")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| "bypassPermissions".into());

        let mut cmd = Command::new("claude");
        cmd.arg("-p")
            .arg(&opts.prompt)
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose")
            .arg("--permission-mode")
            .arg(&permission_mode);
        if let Some(id) = &resume_id {
            cmd.arg("--resume").arg(id);
        }
        cmd.args(&extra_args)
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
                        Some(l) => emit_from_claude_jsonl(&session_id, &l, &tx).await,
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

/// Best-effort translation of one line of Claude Code's stream-json output
/// into our internal AgentEvent format. Extend as the upstream schema grows.
async fn emit_from_claude_jsonl(session_id: &str, line: &str, tx: &mpsc::Sender<AgentEvent>) {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => {
            let _ = tx
                .send(AgentEvent::Text {
                    session_id: session_id.into(),
                    delta: line.into(),
                })
                .await;
            return;
        }
    };

    let kind = v.get("type").and_then(|x| x.as_str()).unwrap_or("");

    // Claude Code surfaces its own session id in two places:
    //   - `system/init` line at the very start
    //   - `result` line at the end
    // Capture it from either; server uses it to feed --resume next turn.
    if let Some(provider_session_id) = v.get("session_id").and_then(|x| x.as_str()) {
        let _ = tx
            .send(AgentEvent::ProviderSessionId {
                session_id: session_id.into(),
                provider_session_id: provider_session_id.into(),
            })
            .await;
    }

    match kind {
        "assistant" => {
            if let Some(blocks) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in blocks {
                    match block.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                        "text" => {
                            let text = block
                                .get("text")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string();
                            let _ = tx
                                .send(AgentEvent::Text {
                                    session_id: session_id.into(),
                                    delta: text,
                                })
                                .await;
                        }
                        "tool_use" => {
                            let name = block
                                .get("name")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string();
                            let input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
                            let _ = tx
                                .send(AgentEvent::ToolCall {
                                    session_id: session_id.into(),
                                    name,
                                    input,
                                })
                                .await;
                        }
                        _ => {}
                    }
                }
            }
        }
        "user" => {
            if let Some(blocks) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in blocks {
                    if block.get("type").and_then(|x| x.as_str()) == Some("tool_result") {
                        let name = block
                            .get("tool_use_id")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        let output = block
                            .get("content")
                            .map(|c| c.to_string())
                            .unwrap_or_default();
                        let _ = tx
                            .send(AgentEvent::ToolResult {
                                session_id: session_id.into(),
                                name,
                                output,
                            })
                            .await;
                    }
                }
            }
        }
        "result" => {
            let reason = v
                .get("subtype")
                .and_then(|x| x.as_str())
                .unwrap_or("done")
                .to_string();
            let _ = tx
                .send(AgentEvent::Finished {
                    session_id: session_id.into(),
                    reason,
                })
                .await;
        }
        _ => {}
    }
}
