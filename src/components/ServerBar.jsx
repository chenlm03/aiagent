import { useEffect, useState } from 'react';

export default function ServerBar({ serverUrl, status, onServerUrlCommit, onRetry }) {
  const [draft, setDraft] = useState(serverUrl);

  useEffect(() => { setDraft(serverUrl); }, [serverUrl]);

  const commit = () => {
    if (draft.trim() && draft !== serverUrl) onServerUrlCommit(draft.trim());
  };

  const label = status === 'ok'
    ? '已连接'
    : status === 'error'
      ? '无法连接'
      : '未检测';

  return (
    <div className="server-bar">
      <label className="field grow">
        <span>中转服务器</span>
        <input
          type="text"
          value={draft}
          spellCheck={false}
          placeholder="http://127.0.0.1:8788"
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => { if (e.key === 'Enter') { e.target.blur(); } }}
        />
      </label>
      <div className={`status status-${status}`}>
        <span className="dot" />
        {label}
      </div>
      <button className="btn ghost" onClick={onRetry}>重试</button>
    </div>
  );
}
