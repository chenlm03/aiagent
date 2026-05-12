import { useEffect, useState } from 'react';

export default function ProviderBar({
  providers,
  installed,
  providerId,
  onProviderChange,
  workspaceRoot,
  workspaceStatus,
  workspaceError,
  onWorkspaceCommit,
}) {
  const [draft, setDraft] = useState(workspaceRoot);
  useEffect(() => { setDraft(workspaceRoot); }, [workspaceRoot]);

  const commit = () => {
    if (draft !== workspaceRoot) onWorkspaceCommit(draft.trim());
  };

  const current = providers.find((p) => p.id === providerId);

  return (
    <div className="provider-bar">
      <label className="field">
        <span>Provider</span>
        <select value={providerId} onChange={(e) => onProviderChange(e.target.value)}>
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name} {installed[p.id] ? '✓' : '○'}
            </option>
          ))}
        </select>
      </label>

      <label className="field grow">
        <span>
          Workspace root <span className="muted">(absolute path on server)</span>
        </span>
        <input
          type="text"
          spellCheck={false}
          placeholder="/home/nick/myproject"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => { if (e.key === 'Enter') e.target.blur(); }}
        />
      </label>

      <div className={`status status-${workspaceStatus}`} title={workspaceError}>
        <span className="dot" />
        {workspaceStatus === 'ok' ? 'workspace ok' :
          workspaceStatus === 'error' ? (workspaceError || 'workspace error') : 'unknown'}
      </div>

      {current && (
        <div className="hint full">
          <span className={`pill ${current.kind}`}>{current.kind}</span>
          <span className="desc">{current.description}</span>
        </div>
      )}
    </div>
  );
}
