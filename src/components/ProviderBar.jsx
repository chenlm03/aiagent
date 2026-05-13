export default function ProviderBar({
  providers,
  installed,
  providerId,
  onProviderChange,
  workspaceRoot,
  workspaceStatus,
  workspaceError,
}) {
  const current = providers.find((p) => p.id === providerId);
  const kindLabel = (k) => k === 'subprocess' ? '子进程' : k === 'api' ? 'API' : k;
  const statusLabel = workspaceStatus === 'ok'
    ? '工作区正常'
    : workspaceStatus === 'unassigned'
      ? '未分配工作区（请联系管理员）'
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

      <div className="field grow">
        <span>
          工作区根目录 <span className="muted">（由管理员指派）</span>
        </span>
        <div className="workspace-display">
          {workspaceRoot || <span className="muted">未分配</span>}
        </div>
      </div>

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
