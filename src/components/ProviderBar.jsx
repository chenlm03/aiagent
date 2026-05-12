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
  const kindLabel = (k) => k === 'subprocess' ? '子进程' : k === 'api' ? 'API' : k;
  const statusLabel = workspaceStatus === 'ok'
    ? '工作区正常'
    : workspaceStatus === 'error'
      ? (workspaceError || '工作区错误')
      : '未检测';

  return (
    <div className="provider-bar">
      <label className="field">
        <span>模型</span>
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
          工作区根目录 <span className="muted">（服务器上的绝对路径）</span>
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
        {statusLabel}
      </div>

      {current && (
        <div className="hint full">
          <span className={`pill ${current.kind}`}>{kindLabel(current.kind)}</span>
          <span className="desc">{current.description}</span>
        </div>
      )}
    </div>
  );
}
