import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

function Markdown({ children }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        a: ({ node, ...props }) => <a {...props} target="_blank" rel="noreferrer" />,
      }}
    >
      {children || ''}
    </ReactMarkdown>
  );
}

export default function MessageRow({ msg }) {
  switch (msg.type) {
    case 'user':
      return <div className="msg user md"><Markdown>{msg.delta}</Markdown></div>;
    case 'text':
      return <div className="msg assistant md"><Markdown>{msg.delta}</Markdown></div>;
    case 'tool_call':
      return (
        <div className="msg tool-call">
          <div className="tool-head">→ 工具调用：{msg.name}</div>
          <pre>{safeJson(msg.input)}</pre>
        </div>
      );
    case 'tool_result':
      return (
        <div className="msg tool-result">
          <div className="tool-head">← 工具结果：{msg.name}</div>
          <pre>{msg.output}</pre>
        </div>
      );
    case 'error':
      return (
        <div className="msg error md">
          <span className="error-prefix">⚠ </span>
          <Markdown>{msg.message}</Markdown>
        </div>
      );
    case 'meta_info':
      return <div className="msg meta">{msg.text}</div>;
    // Diagnostic events — kept in history JSONL but not rendered.
    case 'started':
    case 'finished':
    case 'provider_session_id':
      return null;
    default:
      return null;
  }
}

function safeJson(v) {
  try { return JSON.stringify(v, null, 2); } catch { return String(v); }
}
