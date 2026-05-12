export default function MessageRow({ msg }) {
  switch (msg.type) {
    case 'user':
      return <div className="msg user">{msg.delta}</div>;
    case 'started':
      return <div className="msg meta">▸ session started ({short(msg.session_id)})</div>;
    case 'text':
      return <div className="msg assistant">{msg.delta}</div>;
    case 'tool_call':
      return (
        <div className="msg tool-call">
          <div className="tool-head">→ tool: {msg.name}</div>
          <pre>{safeJson(msg.input)}</pre>
        </div>
      );
    case 'tool_result':
      return (
        <div className="msg tool-result">
          <div className="tool-head">← result: {msg.name}</div>
          <pre>{msg.output}</pre>
        </div>
      );
    case 'error':
      return <div className="msg error">⚠ {msg.message}</div>;
    case 'finished':
      return <div className="msg meta">— done ({msg.reason}) —</div>;
    default:
      return <div className="msg meta">{JSON.stringify(msg)}</div>;
  }
}

function short(id) {
  return (id || '').slice(0, 8);
}

function safeJson(v) {
  try { return JSON.stringify(v, null, 2); } catch { return String(v); }
}
