import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import MessageRow from './components/MessageRow.jsx';
import ProviderBar from './components/ProviderBar.jsx';
import ServerBar from './components/ServerBar.jsx';
import ConversationList from './components/ConversationList.jsx';

export default function App() {
  const [serverUrl, setServerUrl] = useState('http://127.0.0.1:8788');
  const [serverStatus, setServerStatus] = useState('unknown');

  const [providers, setProviders] = useState([]);
  const [providerId, setProviderId] = useState('');
  const [installed, setInstalled] = useState({});

  const [workspaceRoot, setWorkspaceRoot] = useState('');
  const [workspaceStatus, setWorkspaceStatus] = useState('unknown');
  const [workspaceError, setWorkspaceError] = useState('');

  const [conversations, setConversations] = useState([]);
  const [activeConvId, setActiveConvId] = useState(null);

  const [messages, setMessages] = useState([]);
  const [input, setInput] = useState('');
  const [sessionId, setSessionId] = useState(null);
  const [busy, setBusy] = useState(false);
  const endRef = useRef(null);

  useEffect(() => {
    (async () => {
      const cfg = await invoke('load_config').catch(() => ({}));
      if (cfg.server_url) setServerUrl(cfg.server_url);
      if (cfg.workspace_root) setWorkspaceRoot(cfg.workspace_root);
      await refreshFromServer(cfg.active_provider);
      if (cfg.workspace_root) {
        await refreshWorkspace(cfg.workspace_root);
      }
      if (cfg.active_conversation_id && cfg.workspace_root) {
        setActiveConvId(cfg.active_conversation_id);
        await loadHistory(cfg.active_conversation_id, cfg.workspace_root);
      }
    })();
  }, []);

  useEffect(() => {
    const unlisten = listen('agent:event', (e) => {
      const evt = e.payload;
      setMessages((prev) => [...prev, evt]);
      if (evt.type === 'finished') setBusy(false);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const refreshFromServer = async (preferredId) => {
    try {
      await invoke('ping_server');
      setServerStatus('ok');
      const list = await invoke('list_providers');
      setProviders(list);
      const chosen = preferredId && list.find((p) => p.id === preferredId)
        ? preferredId
        : list[0]?.id || '';
      setProviderId(chosen);

      const map = {};
      await Promise.all(list.map(async (p) => {
        map[p.id] = await invoke('detect_provider', { providerId: p.id }).catch(() => false);
      }));
      setInstalled(map);
    } catch {
      setServerStatus('error');
      setProviders([]);
      setInstalled({});
    }
  };

  const refreshWorkspace = async (root) => {
    if (!root) {
      setWorkspaceStatus('unknown');
      setConversations([]);
      return;
    }
    try {
      const check = await invoke('check_workspace', { workspaceRoot: root });
      if (!check.ok) {
        setWorkspaceStatus('error');
        setWorkspaceError(check.message || '工作区出错');
        setConversations([]);
        return;
      }
      setWorkspaceStatus('ok');
      setWorkspaceError('');
      const convs = await invoke('list_conversations', { workspaceRoot: root });
      setConversations(convs);
    } catch (err) {
      setWorkspaceStatus('error');
      setWorkspaceError(String(err));
      setConversations([]);
    }
  };

  const loadHistory = async (convId, root) => {
    if (!convId || !root) {
      setMessages([]);
      return;
    }
    try {
      const events = await invoke('get_conversation_history', {
        conversationId: convId,
        workspaceRoot: root,
      });
      setMessages(events);
    } catch (err) {
      setMessages([{ type: 'error', message: `加载历史失败：${err}` }]);
    }
  };

  const persistConfig = async (patch) => {
    const next = {
      server_url: serverUrl,
      active_provider: providerId,
      workspace_root: workspaceRoot,
      active_conversation_id: activeConvId,
      ...patch,
    };
    await invoke('save_config', { config: next }).catch(() => {});
  };

  const onServerUrlCommit = async (url) => {
    setServerUrl(url);
    await persistConfig({ server_url: url });
    await refreshFromServer(providerId);
  };

  const onWorkspaceCommit = async (root) => {
    setWorkspaceRoot(root);
    setActiveConvId(null);
    setMessages([]);
    await persistConfig({ workspace_root: root, active_conversation_id: null });
    await refreshWorkspace(root);
  };

  const onSelectConv = async (id) => {
    setActiveConvId(id);
    persistConfig({ active_conversation_id: id });
    await loadHistory(id, workspaceRoot);
  };

  const onNewConv = async () => {
    if (workspaceStatus !== 'ok' || !providerId) return;
    try {
      const conv = await invoke('create_conversation', {
        workspaceRoot,
        providerId,
        name: null,
      });
      setConversations((prev) => [...prev, conv]);
      setActiveConvId(conv.id);
      setMessages([{ type: 'meta_info', text: `新会话已创建：${conv.name}（目录：${conv.subdir}）` }]);
      persistConfig({ active_conversation_id: conv.id });
    } catch (err) {
      setMessages((prev) => [...prev, { type: 'error', message: `创建会话失败：${err}` }]);
    }
  };

  const send = async () => {
    const prompt = input.trim();
    if (!prompt || !providerId || !activeConvId || busy) return;
    setMessages((prev) => [...prev, { type: 'user', delta: prompt }]);
    setBusy(true);
    setInput('');
    try {
      const sid = await invoke('send_message', {
        args: {
          providerId,
          prompt,
          conversationId: activeConvId,
          workspaceRoot,
          providerConfig: {},
        },
      });
      setSessionId(sid);
    } catch (err) {
      setMessages((prev) => [...prev, { type: 'error', message: String(err) }]);
      setBusy(false);
    }
  };

  const cancel = async () => {
    if (!sessionId) return;
    await invoke('cancel_session', { sessionId });
  };

  const canSend = serverStatus === 'ok'
    && workspaceStatus === 'ok'
    && !!providerId
    && !!activeConvId
    && !busy;

  return (
    <div className="app">
      <header className="app-header">
        <h1>AI Agent</h1>
        <ServerBar
          serverUrl={serverUrl}
          status={serverStatus}
          onServerUrlCommit={onServerUrlCommit}
          onRetry={() => refreshFromServer(providerId)}
        />
        <ProviderBar
          providers={providers}
          installed={installed}
          providerId={providerId}
          onProviderChange={(id) => { setProviderId(id); persistConfig({ active_provider: id }); }}
          workspaceRoot={workspaceRoot}
          workspaceStatus={workspaceStatus}
          workspaceError={workspaceError}
          onWorkspaceCommit={onWorkspaceCommit}
        />
      </header>

      <div className="body">
        <aside className="sidebar">
          <ConversationList
            conversations={conversations}
            activeId={activeConvId}
            canCreate={workspaceStatus === 'ok' && !!providerId}
            onSelect={onSelectConv}
            onNew={onNewConv}
          />
        </aside>

        <div className="chat-pane">
          <main className="messages">
            {messages.length === 0 && (
              <div className="empty">
                {!activeConvId
                  ? '在左侧选择或新建一个会话。'
                  : serverStatus === 'ok'
                    ? '输入消息开始对话。'
                    : '请先连接服务器。'}
              </div>
            )}
            {messages.map((m, i) => <MessageRow key={i} msg={m} />)}
            <div ref={endRef} />
          </main>

          <footer className="composer">
            <textarea
              value={input}
              placeholder={activeConvId
                ? '说点什么…（Ctrl/Cmd + Enter 发送）'
                : '请先选择一个会话'
              }
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
                  e.preventDefault();
                  send();
                }
              }}
              disabled={!activeConvId}
            />
            <div className="actions">
              {busy ? (
                <button className="btn cancel" onClick={cancel}>取消</button>
              ) : (
                <button
                  className="btn send"
                  onClick={send}
                  disabled={!canSend || !input.trim()}
                >发送</button>
              )}
            </div>
          </footer>
        </div>
      </div>
    </div>
  );
}
