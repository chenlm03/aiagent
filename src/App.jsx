import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import MessageRow from './components/MessageRow.jsx';
import ProviderBar from './components/ProviderBar.jsx';
import ServerBar from './components/ServerBar.jsx';
import ConversationList from './components/ConversationList.jsx';
import LoginScreen from './components/LoginScreen.jsx';
import ChangePasswordModal from './components/ChangePasswordModal.jsx';
import UserAdmin from './components/UserAdmin.jsx';

export default function App() {
  const [serverUrl, setServerUrl] = useState('http://127.0.0.1:8788');
  const [serverStatus, setServerStatus] = useState('unknown');

  const [me, setMe] = useState(null); // { username, role, workspace_root, ... } | null
  const [authChecked, setAuthChecked] = useState(false);

  const [providers, setProviders] = useState([]);
  const [providerId, setProviderId] = useState('');
  const [installed, setInstalled] = useState({});

  const [workspaceStatus, setWorkspaceStatus] = useState('unknown');
  const [workspaceError, setWorkspaceError] = useState('');

  const [conversations, setConversations] = useState([]);
  const [activeConvId, setActiveConvId] = useState(null);

  const [messages, setMessages] = useState([]);
  const [diagBuf, setDiagBuf] = useState([]);
  const [input, setInput] = useState('');
  const [sessionId, setSessionId] = useState(null);
  const [busy, setBusy] = useState(false);
  const endRef = useRef(null);

  const [showPwModal, setShowPwModal] = useState(false);
  const [showAdmin, setShowAdmin] = useState(false);

  const [theme, setTheme] = useState('light');

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  // Initial load: read config, try /me, then bootstrap.
  useEffect(() => {
    (async () => {
      const cfg = await invoke('load_config').catch(() => ({}));
      if (cfg.server_url) setServerUrl(cfg.server_url);
      if (cfg.theme === 'dark' || cfg.theme === 'light') setTheme(cfg.theme);
      await invoke('ping_server').then(() => setServerStatus('ok')).catch(() => setServerStatus('error'));
      if (cfg.auth_token) {
        try {
          const meResp = await invoke('me');
          setMe(meResp);
          await onLoggedIn(meResp, cfg.active_provider, cfg.active_conversation_id);
        } catch {
          // stale token — clear it
          await invoke('logout').catch(() => {});
        }
      }
      setAuthChecked(true);
    })();
  }, []);

  useEffect(() => {
    const unlisten = listen('agent:event', (e) => {
      const evt = e.payload;
      if (evt.type === 'thinking') {
        setMessages((prev) => appendThinking(prev, evt));
        return;
      }
      // Diagnostic events: shown live in the bottom strip while busy,
      // dropped from the message list. Server-side JSONL still records them.
      if (evt.type === 'started') {
        setDiagBuf((prev) => [...prev, `▸ 会话：${shortId(evt.session_id)}`]);
        return;
      }
      if (evt.type === 'provider_session_id') {
        setDiagBuf((prev) => [...prev, `▣ 模型会话：${shortId(evt.provider_session_id)}`]);
        return;
      }
      if (evt.type === 'finished') {
        setMessages((prev) => finishAssistantThinking(prev, evt.session_id));
        setDiagBuf([]);
        setBusy(false);
        return;
      }
      if (evt.type === 'error') {
        setMessages((prev) => appendOrMergeText(finishAssistantThinking(prev, evt.session_id), evt));
        setBusy(false);
        return;
      }
      setMessages((prev) => appendOrMergeText(prev, evt));
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const refreshProviders = async (preferredId) => {
    try {
      const list = await invoke('list_providers');
      setProviders(list);
      const preferred = preferredId && list.find((p) => p.id === preferredId)
        ? preferredId
        : null;
      const chosen = preferred
        || list.find((p) => p.id === 'codex-cli')?.id
        || list.find((p) => p.id === 'claude-code-cli')?.id
        || list[0]?.id
        || '';
      setProviderId(chosen);

      const map = {};
      await Promise.all(list.map(async (p) => {
        map[p.id] = await invoke('detect_provider', { providerId: p.id }).catch(() => false);
      }));
      setInstalled(map);
    } catch {
      setProviders([]);
      setInstalled({});
    }
  };

  const refreshConversations = async () => {
    try {
      const convs = await invoke('list_conversations');
      setConversations(convs);
      setWorkspaceStatus('ok');
      setWorkspaceError('');
      return convs;
    } catch (err) {
      const msg = String(err);
      setConversations([]);
      if (msg.includes('尚未分配工作区')) {
        setWorkspaceStatus('unassigned');
      } else {
        setWorkspaceStatus('error');
      }
      setWorkspaceError(msg);
      return [];
    }
  };

  const onLoggedIn = async (meObj, preferredProvider, preferredConvId) => {
    setMe(meObj);
    await invoke('ping_server').then(() => setServerStatus('ok')).catch(() => setServerStatus('error'));
    await refreshProviders(preferredProvider);
    if (meObj.workspace_root) {
      const convs = await refreshConversations();
      if (preferredConvId) {
        const conv = convs.find((c) => c.id === preferredConvId);
        if (conv) {
          setActiveConvId(preferredConvId);
          if (conv.provider_id && conv.provider_id !== providerId) {
            setProviderId(conv.provider_id);
            persistConfig({ active_provider: conv.provider_id });
          }
          await loadHistory(preferredConvId);
        }
      }
    } else {
      setWorkspaceStatus('unassigned');
      setConversations([]);
    }
  };

  const handleLoggedIn = async (loginResp) => {
    // loginResp = { token, user }
    await onLoggedIn(loginResp.user, null, null);
  };

  const onLogout = async () => {
    await invoke('logout').catch(() => {});
    setMe(null);
    setProviders([]);
    setConversations([]);
    setActiveConvId(null);
    setMessages([]);
    setShowAdmin(false);
    setShowPwModal(false);
  };

  const loadHistory = async (convId) => {
    setDiagBuf([]);
    if (!convId) {
      setMessages([]);
      return;
    }
    try {
      const events = await invoke('get_conversation_history', { conversationId: convId });
      const merged = events.reduce((acc, evt) => appendOrMergeText(acc, evt), []);
      setMessages(merged);
    } catch (err) {
      setMessages([{ type: 'error', message: `加载历史失败：${err}` }]);
    }
  };

  const persistConfig = async (patch) => {
    const cur = await invoke('load_config').catch(() => ({}));
    const next = { ...cur, ...patch };
    await invoke('save_config', { config: next }).catch(() => {});
  };

  const onServerUrlCommit = async (url) => {
    setServerUrl(url);
    await persistConfig({ server_url: url });
    await invoke('ping_server').then(() => setServerStatus('ok')).catch(() => setServerStatus('error'));
  };

  const onSelectConv = async (id) => {
    const conv = conversations.find((c) => c.id === id);
    setActiveConvId(id);
    if (conv?.provider_id && conv.provider_id !== providerId) {
      setProviderId(conv.provider_id);
    }
    persistConfig({
      active_conversation_id: id,
      ...(conv?.provider_id ? { active_provider: conv.provider_id } : {}),
    });
    await loadHistory(id);
  };

  const onDeleteConv = async (conv) => {
    const ok = window.confirm(
      `确认删除会话「${conv.name}」吗？\n\n` +
      `服务器上的目录 ${conv.subdir} 及其所有文件都会被删除，无法恢复。`
    );
    if (!ok) return;
    try {
      await invoke('delete_conversation', { conversationId: conv.id });
      setConversations((prev) => prev.filter((c) => c.id !== conv.id));
      if (activeConvId === conv.id) {
        setActiveConvId(null);
        setMessages([]);
        persistConfig({ active_conversation_id: null });
      }
    } catch (err) {
      setMessages((prev) => [...prev, { type: 'error', message: `删除失败：${err}` }]);
    }
  };

  const onNewConv = async () => {
    if (workspaceStatus !== 'ok' || !providerId) return;
    try {
      const conv = await invoke('create_conversation', { providerId, name: null });
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
    const activeConv = conversations.find((c) => c.id === activeConvId);
    if (activeConv && activeConv.provider_id !== providerId) {
      setMessages((prev) => [...prev, {
        type: 'error',
        message: `当前会话属于 ${activeConv.provider_id}，不能用 ${providerId} 继续。请切回匹配模型或新建会话。`,
      }]);
      return;
    }
    setMessages((prev) => [...prev, { type: 'user', delta: prompt }]);
    setDiagBuf([]);
    setBusy(true);
    setInput('');
    try {
      const sid = await invoke('send_message', {
        args: {
          providerId,
          prompt,
          conversationId: activeConvId,
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
    // Optimistically release the UI; the synthetic 'finished' event from the
    // client will also fire, but we don't want the user staring at a spinner
    // while the cancel round-trip happens.
    setBusy(false);
    setDiagBuf([]);
    setMessages((prev) => [...prev, { type: 'meta_info', text: '— 已取消 —' }]);
    await invoke('cancel_session', { sessionId }).catch(() => {});
  };

  if (!authChecked) {
    return <div className="boot">加载中…</div>;
  }
  if (!me) {
    return (
      <>
        <LoginScreen onLoggedIn={handleLoggedIn} />
      </>
    );
  }

  const activeConv = conversations.find((c) => c.id === activeConvId);
  const providerMatches = !activeConv || activeConv.provider_id === providerId;
  const onProviderChange = (id) => {
    setProviderId(id);
    const patch = { active_provider: id };
    if (activeConv && activeConv.provider_id !== id) {
      setActiveConvId(null);
      setMessages([]);
      setDiagBuf([]);
      patch.active_conversation_id = null;
    }
    persistConfig(patch);
  };

  const canSend = serverStatus === 'ok'
    && workspaceStatus === 'ok'
    && !!providerId
    && !!activeConvId
    && providerMatches
    && !busy;

  return (
    <div className="app">
      <header className="app-header">
        <div className="header-top">
          <h1>AI Agent</h1>
          <div className="user-area">
            <span className="user-tag">
              {me.username}
              <span className={`role-pill ${me.role}`}>
                {me.role === 'admin' ? '管理员' : '用户'}
              </span>
            </span>
            <button
              className="theme-toggle"
              title={theme === 'light' ? '切换到深色' : '切换到浅色'}
              onClick={() => {
                const next = theme === 'light' ? 'dark' : 'light';
                setTheme(next);
                persistConfig({ theme: next });
              }}
            >{theme === 'light' ? '🌙 深色' : '☀ 浅色'}</button>
            {me.role === 'admin' && (
              <button className="btn ghost" onClick={() => setShowAdmin(true)}>用户管理</button>
            )}
            <button className="btn ghost" onClick={() => setShowPwModal(true)}>修改密码</button>
            <button className="btn ghost" onClick={onLogout}>退出登录</button>
          </div>
        </div>
        <ServerBar
          serverUrl={serverUrl}
          status={serverStatus}
          onServerUrlCommit={onServerUrlCommit}
          onRetry={() => invoke('ping_server').then(() => setServerStatus('ok')).catch(() => setServerStatus('error'))}
        />
        <ProviderBar
          providers={providers}
          installed={installed}
          providerId={providerId}
          onProviderChange={onProviderChange}
          workspaceRoot={me.workspace_root || ''}
          workspaceStatus={workspaceStatus}
          workspaceError={workspaceError}
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
            onDelete={onDeleteConv}
          />
        </aside>

        <div className="chat-pane">
          <main className="messages">
            {workspaceStatus === 'unassigned' && (
              <div className="empty">
                你的账户还没有分配工作区，请联系管理员到「用户管理」里为你指派一个工作区根目录。
              </div>
            )}
            {workspaceStatus === 'ok' && messages.length === 0 && (
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

          {busy && diagBuf.length > 0 && (
            <div className="diag-strip">
              {diagBuf.map((line, i) => <span className="diag-chip" key={i}>{line}</span>)}
            </div>
          )}

          <footer className="composer">
            <textarea
              value={input}
              placeholder={activeConvId
                ? providerMatches
                  ? '说点什么…（Ctrl/Cmd + Enter 发送）'
                  : '当前会话属于其他模型，请选择匹配会话或新建会话'
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

      {showPwModal && <ChangePasswordModal onClose={() => setShowPwModal(false)} />}
      {showAdmin && (
        <UserAdmin
          currentUsername={me.username}
          onClose={async () => {
            setShowAdmin(false);
            // Refresh own info — admin may have changed own workspace.
            try {
              const meResp = await invoke('me');
              setMe(meResp);
              if (meResp.workspace_root) {
                await refreshConversations();
              } else {
                setWorkspaceStatus('unassigned');
                setConversations([]);
              }
            } catch {}
          }}
        />
      )}
    </div>
  );
}

function shortId(id) {
  return (id || '').slice(0, 8);
}

function appendThinking(messages, evt) {
  const delta = evt.delta || '';
  if (!delta) return messages;
  const idx = findLiveAssistantIndex(messages, evt.session_id);
  if (idx >= 0) {
    const next = [...messages];
    const msg = next[idx];
    next[idx] = { ...msg, thinking: (msg.thinking || '') + delta };
    return next;
  }
  return [...messages, {
    type: 'text',
    session_id: evt.session_id,
    delta: '',
    thinking: delta,
  }];
}

// Fold consecutive text deltas from the same provider session into one bubble.
// Thinking is kept in that same bubble until the server sends `finished`.
function appendOrMergeText(messages, evt) {
  if (evt.type !== 'text') return [...messages, evt];
  const idx = findLiveAssistantIndex(messages, evt.session_id);
  if (idx >= 0) {
    const next = [...messages];
    const msg = next[idx];
    next[idx] = { ...msg, delta: (msg.delta || '') + (evt.delta || '') };
    return next;
  }
  return [...messages, evt];
}

function finishAssistantThinking(messages, sessionId) {
  if (!sessionId) return messages;
  return messages.flatMap((msg) => {
    if (msg.type !== 'text' || msg.session_id !== sessionId || !msg.thinking) {
      return [msg];
    }
    const { thinking, ...rest } = msg;
    return rest.delta ? [rest] : [];
  });
}

function findLiveAssistantIndex(messages, sessionId) {
  if (!sessionId) return -1;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const msg = messages[i];
    if (msg.type === 'text' && msg.session_id === sessionId) return i;
    if (msg.type === 'user' || msg.type === 'error' || msg.type === 'meta_info') break;
  }
  return -1;
}
