import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export default function ChangePasswordModal({ onClose }) {
  const [oldPw, setOldPw] = useState('');
  const [newPw, setNewPw] = useState('');
  const [confirmPw, setConfirmPw] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [done, setDone] = useState(false);

  const submit = async (e) => {
    e?.preventDefault();
    if (busy) return;
    if (newPw !== confirmPw) {
      setError('两次新密码不一致');
      return;
    }
    if (newPw.length < 4) {
      setError('新密码至少 4 位');
      return;
    }
    setBusy(true);
    setError('');
    try {
      await invoke('change_password', { oldPassword: oldPw, newPassword: newPw });
      setDone(true);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <form className="modal-card" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <div className="modal-head">
          <h2>修改密码</h2>
          <button type="button" className="modal-close" onClick={onClose}>×</button>
        </div>
        {done ? (
          <div className="modal-body">
            <div className="modal-ok">密码已修改</div>
            <button type="button" className="btn ghost block" onClick={onClose}>关闭</button>
          </div>
        ) : (
          <div className="modal-body">
            <label className="field">
              <span>当前密码</span>
              <input type="password" value={oldPw} onChange={(e) => setOldPw(e.target.value)} autoFocus />
            </label>
            <label className="field">
              <span>新密码</span>
              <input type="password" value={newPw} onChange={(e) => setNewPw(e.target.value)} />
            </label>
            <label className="field">
              <span>确认新密码</span>
              <input type="password" value={confirmPw} onChange={(e) => setConfirmPw(e.target.value)} />
            </label>
            {error && <div className="modal-error">{error}</div>}
            <button type="submit" className="btn send block" disabled={busy}>
              {busy ? '提交中…' : '提交'}
            </button>
          </div>
        )}
      </form>
    </div>
  );
}
