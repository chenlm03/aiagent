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
  const [input, setInput] = useState('');
  const [sessionId, setSessionId] = useState(null);
  const [busy, setBusy] = useState(false);
  const endRef = useRef(null);

  const [showPwModal, setShowPwModal] = useState(false);
  const [showAdmin, setShowAdmin] = useState(false);

  // Initial load: read config, try /me, then bootstrap.
  useEffect(() => {
    (async () => {
      const cfg = await invoke('load_config').catch(() => ({}));
      if (cfg.server_url) setServerUrl(cfg.server_url);
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
      setMessages((prev) => [...prev, evt]);
      if (evt.type === 'finished') setBusy(false);
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
    } catch (err) {
      const msg = String(err);
      setConversations([]);
      if (msg.includes('尚未分配工作区')) {
        setWorkspaceStatus('unassigned');
      } else {
        setWorkspaceStatus('error');
      }
      setWorkspaceError(msg);
    }
  };

  const onLoggedIn = async (meObj, preferredProvider, preferredConvId) => {
    setMe(meObj);
    await invoke('ping_server').then(() => setServerStatus('ok')).catch(() => setServerStatus('error'));
    await refreshProviders(preferredProvider);
    if (meObj.workspace_root) {
      await refreshConversations();
      if (preferredConvId) {
        setActiveConvId(preferredConvId);
        await loadHistory(preferredConvId);
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
    if (!convId) {
      setMessages([]);
      return;
    }
    try {
      const events = await invoke('get_conversation_history', { conversationId: convId });
      setMessages(events);
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
    setActiveConvId(id);
    persistConfig({ active_conversation_id: id });
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
    setMessages((prev) => [...prev, { type: 'user', delta: prompt }]);
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
    await invoke('cancel_session', { sessionId });
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

  const canSend = serverStatus === 'ok'
    && workspaceStatus === 'ok'
    && !!providerId
    && !!activeConvId
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
          onProviderChange={(id) => { setProviderId(id); persistConfig({ active_provider: id }); }}
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
