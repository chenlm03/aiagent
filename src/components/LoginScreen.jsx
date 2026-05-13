import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export default function LoginScreen({ onLoggedIn }) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

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

  return (
    <div className="login-screen">
      <form className="login-card" onSubmit={submit}>
        <h1>AI Agent 登录</h1>
        <label className="field">
          <span>用户名</span>
          <input
            type="text"
            value={username}
            autoFocus
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
        <button type="submit" className="btn send block" disabled={busy}>
          {busy ? '登录中…' : '登录'}
        </button>
        <div className="login-hint">
          首次使用：默认管理员 <code>nick</code> / <code>123456</code>，建议登录后到「修改密码」改掉。
        </div>
      </form>
    </div>
  );
}
