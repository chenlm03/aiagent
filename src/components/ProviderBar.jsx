export default function ProviderBar({
  providers,
  installed,
  providerId,
  onProviderChange,
  workingDir,
  onWorkingDirChange,
}) {
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
        <span>Working dir</span>
        <input
          type="text"
          placeholder="(optional, defaults to app cwd)"
          value={workingDir}
          onChange={(e) => onWorkingDirChange(e.target.value)}
        />
      </label>

      {current && (
        <div className="hint">
          <span className={`pill ${current.kind}`}>{current.kind}</span>
          <span className="desc">{current.description}</span>
        </div>
      )}
    </div>
  );
}
