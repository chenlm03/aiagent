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
            description:
                "Spawns the `codex` CLI in non-interactive JSON mode and streams progress.".into(),
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
        let resume_id: Option<String> = opts
            .provider_config
            .get("resume")
            .and_then(|v| v.as_str())
            .map(String::from);

        let mut cmd = Command::new("codex");
        // OpenAI Codex CLI: `codex exec --json "<prompt>"`.
        // - `--skip-git-repo-check`: workspace subdirs aren't git repos, codex
        //   would otherwise refuse to run.
        // - `--dangerously-bypass-approvals-and-sandbox`: codex's built-in
        //   Linux sandbox uses bubblewrap, which needs unprivileged user
        //   namespaces — blocked on this host by AppArmor/kernel policy
        //   (bwrap fails with "loopback: Failed RTM_NEWADDR"). We rely on
        //   per-conversation working_dir + the server's Unix user as the
        //   isolation boundary instead.
        // - stdin redirected to null: without this codex waits on stdin and
        //   hangs forever in headless mode.
        cmd.arg("exec");
        if resume_id.is_some() {
            cmd.arg("resume");
        }
        cmd.arg("--json")
            .arg("--skip-git-repo-check")
            .arg("--dangerously-bypass-approvals-and-sandbox")
            .args(&extra_args);
        if let Some(id) = &resume_id {
            cmd.arg(id);
        }
        cmd.arg(&opts.prompt)
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
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("no stderr"))?;

        let _ = tx
            .send(AgentEvent::Started {
                session_id: session_id.clone(),
            })
            .await;

        let stdout_session_id = session_id.clone();
        let stdout_tx = tx.clone();
        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            let mut pending_agent_text = None;
            while let Some(line) = reader.next_line().await? {
                emit_from_codex_jsonl(
                    &stdout_session_id,
                    &line,
                    &stdout_tx,
                    &mut pending_agent_text,
                )
                .await;
            }
            if let Some(text) = pending_agent_text.take() {
                emit_text(&stdout_session_id, &stdout_tx, &text).await;
            }
            Ok::<(), anyhow::Error>(())
        });

        let stderr_session_id = session_id.clone();
        let stderr_tx = tx.clone();
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            let mut stderr_text = String::new();
            while let Some(line) = reader.next_line().await? {
                let line = line.trim();
                if !line.is_empty() {
                    stderr_text.push_str(line);
                    stderr_text.push('\n');
                    let _ = stderr_tx
                        .send(AgentEvent::Thinking {
                            session_id: stderr_session_id.clone(),
                            delta: format!("{line}\n"),
                        })
                        .await;
                }
            }
            Ok::<String, anyhow::Error>(stderr_text)
        });

        let status = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                stdout_task.abort();
                stderr_task.abort();
                let _ = tx.send(AgentEvent::Finished {
                    session_id: session_id.clone(),
                    reason: "cancelled".into(),
                }).await;
                return Ok(());
            }
            status = child.wait() => status?,
        };

        if let Ok(Err(e)) = stdout_task.await {
            let _ = tx
                .send(AgentEvent::Error {
                    session_id: session_id.clone(),
                    message: format!("codex stdout error: {e}"),
                })
                .await;
        }
        let mut stderr_text = String::new();
        match stderr_task.await {
            Ok(Ok(text)) => stderr_text = text,
            Ok(Err(e)) => {
                let _ = tx
                    .send(AgentEvent::Error {
                        session_id: session_id.clone(),
                        message: format!("codex stderr error: {e}"),
                    })
                    .await;
            }
            Err(e) if !e.is_cancelled() => {
                let _ = tx
                    .send(AgentEvent::Error {
                        session_id: session_id.clone(),
                        message: format!("codex stderr task error: {e}"),
                    })
                    .await;
            }
            _ => {}
        }

        if !status.success() {
            let msg = if stderr_text.trim().is_empty() {
                format!("codex exited with status {}", status.code().unwrap_or(-1))
            } else {
                stderr_text.trim().to_string()
            };
            let _ = tx
                .send(AgentEvent::Error {
                    session_id: session_id.clone(),
                    message: msg,
                })
                .await;
        }

        let _ = tx
            .send(AgentEvent::Finished {
                session_id,
                reason: format!("exit:{}", status.code().unwrap_or(-1)),
            })
            .await;
        Ok(())
    }
}

/// Translate one Codex CLI `exec --json` JSONL event into the provider-agnostic
/// event stream. Codex does not expose raw hidden chain-of-thought; the UI gets
/// high-level progress/status lines while waiting, then the final answer.
async fn emit_from_codex_jsonl(
    session_id: &str,
    line: &str,
    tx: &mpsc::Sender<AgentEvent>,
    pending_agent_text: &mut Option<String>,
) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }

    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => {
            let _ = tx
                .send(AgentEvent::Thinking {
                    session_id: session_id.into(),
                    delta: format!("{line}\n"),
                })
                .await;
            return;
        }
    };

    let kind = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    match kind {
        "thread.started" => {
            if let Some(thread_id) = v.get("thread_id").and_then(|x| x.as_str()) {
                let _ = tx
                    .send(AgentEvent::ProviderSessionId {
                        session_id: session_id.into(),
                        provider_session_id: thread_id.into(),
                    })
                    .await;
                emit_thinking(
                    session_id,
                    tx,
                    &format!("Codex 会话：{}\n", short_id(thread_id)),
                )
                .await;
            }
        }
        "turn.started" => {
            emit_thinking(session_id, tx, "Codex 正在思考...\n").await;
        }
        "item.started" => {
            if let Some(item) = v.get("item") {
                if let Some(line) = describe_codex_item(item, true) {
                    emit_thinking(session_id, tx, &line).await;
                }
            }
        }
        "item.completed" => {
            if let Some(item) = v.get("item") {
                emit_codex_item_completed(session_id, item, tx, pending_agent_text).await;
            }
        }
        "turn.completed" => {
            let text = pending_agent_text
                .take()
                .or_else(|| string_field(&v, &["last_agent_message"]).map(str::to_string));
            if let Some(text) = text {
                emit_text(session_id, tx, &text).await;
            }
        }
        _ if kind.contains("agent_message") && kind.contains("delta") => {
            if let Some(delta) = string_field(&v, &["delta", "text"]) {
                append_pending_agent_text(pending_agent_text, delta);
            }
        }
        _ if kind.contains("reasoning") && kind.contains("delta") => {
            if let Some(delta) = string_field(&v, &["summary", "text"]) {
                emit_thinking(session_id, tx, &format!("{delta}\n")).await;
            }
        }
        _ => {}
    }
}

async fn emit_codex_item_completed(
    session_id: &str,
    item: &serde_json::Value,
    tx: &mpsc::Sender<AgentEvent>,
    pending_agent_text: &mut Option<String>,
) {
    let item_type = item.get("type").and_then(|x| x.as_str()).unwrap_or("");
    match item_type {
        "agent_message" | "message" => {
            if let Some(text) = extract_text(item) {
                let phase = item.get("phase").and_then(|x| x.as_str()).unwrap_or("");
                if phase == "commentary" {
                    emit_thinking(session_id, tx, &format!("{text}\n")).await;
                } else {
                    replace_pending_agent_text(session_id, tx, pending_agent_text, text).await;
                }
            }
        }
        "reasoning" | "agent_reasoning" => {
            if let Some(summary) = extract_reasoning_summary(item) {
                emit_thinking(session_id, tx, &format!("{summary}\n")).await;
            }
        }
        _ => {
            if let Some(line) = describe_codex_item(item, false) {
                emit_thinking(session_id, tx, &line).await;
            }
        }
    }
}

fn append_pending_agent_text(pending_agent_text: &mut Option<String>, delta: &str) {
    if delta.is_empty() {
        return;
    }
    pending_agent_text
        .get_or_insert_with(String::new)
        .push_str(delta);
}

async fn replace_pending_agent_text(
    session_id: &str,
    tx: &mpsc::Sender<AgentEvent>,
    pending_agent_text: &mut Option<String>,
    text: &str,
) {
    if text.is_empty() {
        return;
    }
    if let Some(previous) = pending_agent_text.replace(text.to_string()) {
        emit_thinking(session_id, tx, &format!("{previous}\n")).await;
    }
}

fn describe_codex_item(item: &serde_json::Value, started: bool) -> Option<String> {
    let item_type = item.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let action = if started { "开始" } else { "完成" };
    match item_type {
        "reasoning" | "agent_reasoning" => Some(format!("Codex {action}推理...\n")),
        "function_call" | "tool_call" | "mcp_tool_call" => {
            let name = string_field(item, &["name", "tool_name"]).unwrap_or("tool");
            Some(format!("{action}调用工具：{name}\n"))
        }
        "local_shell_call" | "exec_command" | "command_execution" => {
            let cmd = string_field(item, &["command", "cmd"])
                .or_else(|| item.get("arguments").and_then(extract_text))
                .unwrap_or("shell");
            Some(format!("{action}执行命令：{cmd}\n"))
        }
        _ => None,
    }
}

fn extract_text(v: &serde_json::Value) -> Option<&str> {
    if let Some(text) = string_field(v, &["text", "message", "delta"]) {
        return Some(text);
    }
    let content = v.get("content")?.as_array()?;
    content.iter().find_map(|part| {
        if part.is_string() {
            part.as_str()
        } else {
            string_field(part, &["text", "output_text"])
        }
    })
}

fn extract_reasoning_summary(v: &serde_json::Value) -> Option<String> {
    let summary = v.get("summary")?.as_array()?;
    let lines: Vec<&str> = summary
        .iter()
        .filter_map(|part| {
            if part.is_string() {
                part.as_str()
            } else {
                string_field(part, &["text", "summary"])
            }
        })
        .filter(|s| !s.trim().is_empty())
        .collect();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn string_field<'a>(v: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| v.get(*key)?.as_str())
}

async fn emit_text(session_id: &str, tx: &mpsc::Sender<AgentEvent>, delta: &str) {
    if delta.is_empty() {
        return;
    }
    let _ = tx
        .send(AgentEvent::Text {
            session_id: session_id.into(),
            delta: delta.into(),
        })
        .await;
}

async fn emit_thinking(session_id: &str, tx: &mpsc::Sender<AgentEvent>, delta: &str) {
    if delta.is_empty() {
        return;
    }
    let _ = tx
        .send(AgentEvent::Thinking {
            session_id: session_id.into(),
            delta: delta.into(),
        })
        .await;
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}
