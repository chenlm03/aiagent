import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export default function UserAdmin({ currentUsername, onClose }) {
  const [users, setUsers] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState({ username: '', password: '', workspace_root: '' });

  // Per-row edit state for password / workspace fields.
  const [editPw, setEditPw] = useState({});      // { [username]: string }
  const [editWs, setEditWs] = useState({});      // { [username]: string }
  const [rowBusy, setRowBusy] = useState({});    // { [username]: bool }
  const [rowError, setRowError] = useState({});  // { [username]: string }

  const refresh = async () => {
    setLoading(true);
    setError('');
    try {
      const list = await invoke('admin_list_users');
      setUsers(list);
      const ws = {};
      for (const u of list) ws[u.username] = u.workspace_root || '';
      setEditWs(ws);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { refresh(); }, []);

  const create = async (e) => {
    e?.preventDefault();
    if (creating) return;
    if (!draft.username.trim() || !draft.password) {
      setError('用户名和密码必填');
      return;
    }
    setCreating(true);
    setError('');
    try {
      await invoke('admin_create_user', {
        username: draft.username.trim(),
        password: draft.password,
        workspaceRoot: draft.workspace_root.trim() || null,
      });
      setDraft({ username: '', password: '', workspace_root: '' });
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setCreating(false);
    }
  };

  const setRowBusyFor = (u, v) => setRowBusy((p) => ({ ...p, [u]: v }));
  const setRowErrorFor = (u, v) => setRowError((p) => ({ ...p, [u]: v }));

  const submitPw = async (username) => {
    const pw = (editPw[username] || '').trim();
    if (pw.length < 4) {
      setRowErrorFor(username, '密码至少 4 位');
      return;
    }
    setRowBusyFor(username, true);
    setRowErrorFor(username, '');
    try {
      await invoke('admin_set_password', { username, password: pw });
      setEditPw((p) => ({ ...p, [username]: '' }));
    } catch (err) {
      setRowErrorFor(username, String(err));
    } finally {
      setRowBusyFor(username, false);
    }
  };

  const submitWs = async (username) => {
    const ws = (editWs[username] || '').trim();
    setRowBusyFor(username, true);
    setRowErrorFor(username, '');
    try {
      const updated = await invoke('admin_set_workspace', {
        username,
        workspaceRoot: ws || null,
      });
      setUsers((prev) => prev.map((u) => u.username === username ? updated : u));
    } catch (err) {
      setRowErrorFor(username, String(err));
    } finally {
      setRowBusyFor(username, false);
    }
  };

  const remove = async (username) => {
    if (!window.confirm(`确认删除用户 ${username}？`)) return;
    setRowBusyFor(username, true);
    try {
      await invoke('admin_delete_user', { username });
      await refresh();
    } catch (err) {
      setRowErrorFor(username, String(err));
    } finally {
      setRowBusyFor(username, false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-card wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h2>用户管理</h2>
          <button type="button" className="modal-close" onClick={onClose}>×</button>
        </div>

        <div className="modal-body">
          <form className="user-create" onSubmit={create}>
            <h3>新增用户</h3>
            <div className="user-create-row">
              <input
                placeholder="用户名"
                value={draft.username}
                spellCheck={false}
                onChange={(e) => setDraft({ ...draft, username: e.target.value })}
              />
              <input
                type="password"
                placeholder="初始密码"
                value={draft.password}
                onChange={(e) => setDraft({ ...draft, password: e.target.value })}
              />
              <input
                placeholder="工作区根目录（可选，可稍后指派）"
                value={draft.workspace_root}
                spellCheck={false}
                onChange={(e) => setDraft({ ...draft, workspace_root: e.target.value })}
              />
              <button type="submit" className="btn send" disabled={creating}>
                {creating ? '创建中…' : '创建'}
              </button>
            </div>
            {error && <div className="modal-error">{error}</div>}
          </form>

          <h3>已有用户</h3>
          {loading ? (
            <div className="muted">加载中…</div>
          ) : (
            <table className="user-table">
              <thead>
                <tr>
                  <th>用户名</th>
                  <th>角色</th>
                  <th>工作区根目录</th>
                  <th>重置密码</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                {users.map((u) => (
                  <tr key={u.username}>
                    <td>
                      {u.username}
                      {u.username === currentUsername && <span className="me-tag">（你）</span>}
                    </td>
                    <td>{u.role === 'admin' ? '管理员' : '用户'}</td>
                    <td className="cell-ws">
                      <input
                        value={editWs[u.username] || ''}
                        placeholder="未分配"
                        spellCheck={false}
                        onChange={(e) => setEditWs({ ...editWs, [u.username]: e.target.value })}
                      />
                      <button
                        className="btn ghost"
                        disabled={rowBusy[u.username]}
                        onClick={() => submitWs(u.username)}
                      >保存</button>
                    </td>
                    <td className="cell-pw">
                      <input
                        type="password"
                        value={editPw[u.username] || ''}
                        placeholder="新密码"
                        onChange={(e) => setEditPw({ ...editPw, [u.username]: e.target.value })}
                      />
                      <button
                        className="btn ghost"
                        disabled={rowBusy[u.username]}
                        onClick={() => submitPw(u.username)}
                      >重置</button>
                    </td>
                    <td>
                      <button
                        className="btn ghost danger"
                        disabled={rowBusy[u.username] || u.username === currentUsername}
                        onClick={() => remove(u.username)}
                      >删除</button>
                      {rowError[u.username] && <div className="row-error">{rowError[u.username]}</div>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </div>
  );
}
