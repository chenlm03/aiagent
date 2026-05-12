import { useEffect, useState } from 'react';

export default function ServerBar({ serverUrl, status, onServerUrlCommit, onRetry }) {
  const [draft, setDraft] = useState(serverUrl);

  useEffect(() => { setDraft(serverUrl); }, [serverUrl]);

  const commit = () => {
    if (draft.trim() && draft !== serverUrl) onServerUrlCommit(draft.trim());
  };

  return (
    <div className="server-bar">
      <label className="field grow">
        <span>Relay server</span>
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
        {status === 'ok' ? 'connected' : status === 'error' ? 'unreachable' : 'unknown'}
      </div>
      <button className="btn ghost" onClick={onRetry}>Retry</button>
    </div>
  );
}
