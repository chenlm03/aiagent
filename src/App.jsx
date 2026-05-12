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
  const [workspaceStatus, setWorkspaceStatus] = useState('unknown'); // unknown | ok | error
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
      if (cfg.active_conversation_id) setActiveConvId(cfg.active_conversation_id);
      await refreshFromServer(cfg.active_provider);
      if (cfg.workspace_root) {
        await refreshWorkspace(cfg.workspace_root);
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
        setWorkspaceError(check.message || 'workspace error');
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

  const onSelectConv = (id) => {
    setActiveConvId(id);
    setMessages([]);
    persistConfig({ active_conversation_id: id });
  };

  const onNewConv = async () => {
    if (workspaceStatus !== 'ok' || !providerId) return;
    try {
      const conv = await invoke('create_conversation', {
        workspaceRoot: workspaceRoot,
        providerId,
        name: null,
      });
      setConversations((prev) => [...prev, conv]);
      setActiveConvId(conv.id);
      setMessages([{ type: 'meta_info', text: `New conversation: ${conv.name}  (cwd: ${conv.subdir})` }]);
      persistConfig({ active_conversation_id: conv.id });
    } catch (err) {
      setMessages((prev) => [...prev, { type: 'error', message: `create conversation: ${err}` }]);
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
          workspaceRoot: workspaceRoot,
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
                {!activeConvId ? 'Select or create a conversation in the sidebar.' :
                  serverStatus === 'ok' ? 'Send a message to start.' :
                  'Connect to a server first.'}
              </div>
            )}
            {messages.map((m, i) => <MessageRow key={i} msg={m} />)}
            <div ref={endRef} />
          </main>

          <footer className="composer">
            <textarea
              value={input}
              placeholder={activeConvId
                ? 'Ask anything…  (Ctrl/Cmd+Enter to send)'
                : 'Select a conversation first'
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
                <button className="btn cancel" onClick={cancel}>Cancel</button>
              ) : (
                <button
                  className="btn send"
                  onClick={send}
                  disabled={!canSend || !input.trim()}
                >Send</button>
              )}
            </div>
          </footer>
        </div>
      </div>
    </div>
  );
}
