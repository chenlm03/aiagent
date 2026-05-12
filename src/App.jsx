import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import MessageRow from './components/MessageRow.jsx';
import ProviderBar from './components/ProviderBar.jsx';
import ServerBar from './components/ServerBar.jsx';

export default function App() {
  const [serverUrl, setServerUrl] = useState('http://127.0.0.1:8788');
  const [serverStatus, setServerStatus] = useState('unknown'); // unknown | ok | error

  const [providers, setProviders] = useState([]);
  const [providerId, setProviderId] = useState('');
  const [installed, setInstalled] = useState({});
  const [workingDir, setWorkingDir] = useState('');

  const [messages, setMessages] = useState([]);
  const [input, setInput] = useState('');
  const [sessionId, setSessionId] = useState(null);
  const [busy, setBusy] = useState(false);
  const endRef = useRef(null);

  // Initial load: config + ping + providers
  useEffect(() => {
    (async () => {
      const cfg = await invoke('load_config').catch(() => ({}));
      if (cfg.server_url) setServerUrl(cfg.server_url);
      if (cfg.working_dir) setWorkingDir(cfg.working_dir);
      await refreshFromServer(cfg.active_provider);
    })();
  }, []);

  useEffect(() => {
    const unlisten = listen('agent:event', (e) => {
      const evt = e.payload;
      setMessages((prev) => [...prev, evt]);
      if (evt.type === 'finished') setBusy(false);
      if (evt.type === 'error') {
        // keep busy on error so user can decide whether to cancel; finished event will clear it
      }
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
    } catch (err) {
      setServerStatus('error');
      setProviders([]);
      setInstalled({});
    }
  };

  const persistConfig = async (patch) => {
    const next = {
      server_url: serverUrl,
      active_provider: providerId,
      working_dir: workingDir,
      ...patch,
    };
    await invoke('save_config', { config: next }).catch(() => {});
  };

  const onServerUrlCommit = async (url) => {
    setServerUrl(url);
    await persistConfig({ server_url: url });
    await refreshFromServer(providerId);
  };

  const send = async () => {
    const prompt = input.trim();
    if (!prompt || !providerId || busy) return;
    setMessages((prev) => [...prev, { type: 'user', delta: prompt }]);
    setBusy(true);
    setInput('');
    try {
      const sid = await invoke('send_message', {
        args: {
          providerId,
          prompt,
          workingDir: workingDir || null,
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
          workingDir={workingDir}
          onWorkingDirChange={(v) => { setWorkingDir(v); persistConfig({ working_dir: v }); }}
        />
      </header>

      <main className="messages">
        {messages.length === 0 && (
          <div className="empty">
            {serverStatus === 'ok'
              ? 'Choose a provider above, then send a message.'
              : 'Connect to a server first.'}
          </div>
        )}
        {messages.map((m, i) => <MessageRow key={i} msg={m} />)}
        <div ref={endRef} />
      </main>

      <footer className="composer">
        <textarea
          value={input}
          placeholder="Ask anything…  (Ctrl/Cmd+Enter to send)"
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              send();
            }
          }}
        />
        <div className="actions">
          {busy ? (
            <button className="btn cancel" onClick={cancel}>Cancel</button>
          ) : (
            <button
              className="btn send"
              onClick={send}
              disabled={!input.trim() || serverStatus !== 'ok'}
            >Send</button>
          )}
        </div>
      </footer>
    </div>
  );
}
