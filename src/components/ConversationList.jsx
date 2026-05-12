export default function ConversationList({ conversations, activeId, canCreate, onSelect, onNew }) {
  const sorted = [...conversations].sort((a, b) => (b.updated_at || 0) - (a.updated_at || 0));

  return (
    <div className="conv-list">
      <button className="btn ghost block" onClick={onNew} disabled={!canCreate}>
        + New conversation
      </button>
      <div className="conv-items">
        {sorted.length === 0 && (
          <div className="conv-empty">No conversations yet.</div>
        )}
        {sorted.map((c) => (
          <button
            key={c.id}
            className={`conv-item ${activeId === c.id ? 'active' : ''}`}
            onClick={() => onSelect(c.id)}
            title={c.subdir}
          >
            <div className="conv-name">{c.name}</div>
            <div className="conv-meta">
              {c.provider_session_id ? '↻ resume' : '✦ new'} · {c.subdir}
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
