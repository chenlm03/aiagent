import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export default function LoginScreen({ onLoggedIn }) {
  const [serverUrl, setServerUrl] = useState('http://127.0.0.1:8788');
  const [serverDraft, setServerDraft] = useState('http://127.0.0.1:8788');
  const [serverStatus, setServerStatus] = useState('unknown');

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  // Load saved server URL from config on mount, then ping.
  useEffect(() => {
    (async () => {
      const cfg = await invoke('load_config').catch(() => ({}));
      const url = cfg.server_url || 'http://127.0.0.1:8788';
      setServerUrl(url);
      setServerDraft(url);
      await ping();
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const ping = async () => {
    setServerStatus('unknown');
    try {
      await invoke('ping_server');
      setServerStatus('ok');
    } catch {
      setServerStatus('error');
    }
  };

  const commitServer = async () => {
    const next = serverDraft.trim();
    if (!next || next === serverUrl) return;
    setServerUrl(next);
    // Persist BEFORE ping so the underlying client picks up the new URL.
    const cfg = await invoke('load_config').catch(() => ({}));
    await invoke('save_config', { config: { ...cfg, server_url: next } }).catch(() => {});
    await ping();
  };

  const submit = async (e) => {
    e?.preventDefault();
    if (busy) return;
    if (!username.trim() || !password) {
      setError('请输入用户名和密码');
      return;
    }
    setBusy(true);
    setError('');
    try {
      const resp = await invoke('login', { username: username.trim(), password });
      onLoggedIn(resp);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const statusLabel = serverStatus === 'ok'
    ? '已连接'
    : serverStatus === 'error'
      ? '无法连接'
      : '未检测';

  return (
    <div className="login-screen">
      <form className="login-card" onSubmit={submit}>
        <h1>AI Agent 登录</h1>

        <label className="field">
          <span>
            中转服务器地址
            <span className={`status status-${serverStatus} inline`} title={statusLabel}>
              <span className="dot" /> {statusLabel}
            </span>
          </span>
          <div className="row">
            <input
              type="text"
              value={serverDraft}
              spellCheck={false}
              placeholder="http://192.168.x.x:8788"
              onChange={(e) => setServerDraft(e.target.value)}
              onBlur={commitServer}
              onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); commitServer(); } }}
            />
            <button type="button" className="btn ghost" onClick={ping}>重试</button>
          </div>
        </label>

        <label className="field">
          <span>用户名</span>
          <input
            type="text"
            value={username}
            autoComplete="username"
            spellCheck={false}
            onChange={(e) => setUsername(e.target.value)}
          />
        </label>
        <label className="field">
          <span>密码</span>
          <input
            type="password"
            value={password}
            autoComplete="current-password"
            onChange={(e) => setPassword(e.target.value)}
          />
        </label>
        {error && <div className="login-error">{error}</div>}
        <button
          type="submit"
          className="btn send block"
          disabled={busy || serverStatus !== 'ok'}
        >
          {busy ? '登录中…' : serverStatus === 'ok' ? '登录' : '请先连接服务器'}
        </button>
        <div className="login-hint">
          首次使用：默认管理员 <code>nick</code> / <code>123456</code>，建议登录后到「修改密码」改掉。
        </div>
      </form>
    </div>
  );
}
