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
    case 'started':
      return <div className="msg meta">▸ 会话开始（{short(msg.session_id)}）</div>;
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
    case 'finished':
      return <div className="msg meta">— 完成（{msg.reason}） —</div>;
    case 'provider_session_id':
      return <div className="msg meta">▣ 模型会话 ID：{short(msg.provider_session_id)}</div>;
    case 'meta_info':
      return <div className="msg meta">{msg.text}</div>;
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
