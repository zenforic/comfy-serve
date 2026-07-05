import React, { useState, useEffect } from 'react';
import Prism from 'prismjs';
import 'prismjs/components/prism-json';
import 'prismjs/themes/prism-tomorrow.css';
import './index.css';

class ErrorBoundary extends React.Component<{children: React.ReactNode}, {hasError: boolean, error: any}> {
  constructor(props: any) {
    super(props);
    this.state = { hasError: false, error: null };
  }
  static getDerivedStateFromError(error: any) { return { hasError: true, error }; }
  render() {
    if (this.state.hasError) {
      return <div style={{padding: 20, color: 'red'}}><h2>UI Crash</h2><pre>{String(this.state.error?.stack || this.state.error)}</pre></div>;
    }
    return this.props.children;
  }
}

function App() {
  const [token, setToken] = useState<string | null>(localStorage.getItem('token'));
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  
  const [view, setView] = useState<'login' | 'onboarding' | 'dashboard'>('login');

  useEffect(() => {
    if (token) {
      checkAuth(token);
    }
  }, [token]);

  const checkAuth = async (t: string) => {
    try {
      const res = await fetch('/api/auth_check', {
        headers: { 'Authorization': `Bearer ${t}` }
      });
      if (res.ok) {
        checkConfig(t);
      } else {
        setToken(null);
        localStorage.removeItem('token');
        setView('login');
      }
    } catch (e) {
      console.error(e);
    }
  };

  const checkConfig = async (t: string) => {
    try {
      const res = await fetch('/api/config', {
        headers: { 'Authorization': `Bearer ${t}` }
      });
      if (res.ok) {
        const config = await res.json();
        if (config.comfyui_url === 'http://127.0.0.1:8188' && Object.keys(config.workflows).length === 0) {
          setView('onboarding');
        } else {
          setView('dashboard');
        }
      }
    } catch(e) {
      console.error(e);
    }
  };

  const handleLogin = async () => {
    try {
      const res = await fetch('/api/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ password })
      });
      if (res.ok) {
        const data = await res.json();
        setToken(data.token);
        localStorage.setItem('token', data.token);
        if (data.is_new) {
          setView('onboarding');
        } else {
          checkConfig(data.token);
        }
      } else {
        setError('Invalid password');
      }
    } catch (e) {
      setError('Login failed');
    }
  };

  if (view === 'login') {
    return (
      <div className="login-container">
        <div className="panel" style={{ width: 400 }}>
          <h2 style={{ marginBottom: 20 }}>Comfy-Serve Login</h2>
          {error && <p style={{ color: 'var(--danger)', marginBottom: 10 }}>{error}</p>}
          <div className="form-group">
            <label>Password</label>
            <input 
              type="password" 
              value={password} 
              onChange={e => setPassword(e.target.value)} 
              onKeyDown={e => e.key === 'Enter' && handleLogin()}
              placeholder="Enter dashboard password..."
            />
          </div>
          <button onClick={handleLogin} style={{ width: '100%', marginTop: 10 }}>Login / Setup</button>
        </div>
      </div>
    );
  }

  if (view === 'onboarding') {
    return <Onboarding token={token!} onComplete={() => setView('dashboard')} />;
  }

  return <Dashboard token={token!} />;
}

function Onboarding({ token, onComplete }: { token: string, onComplete: () => void }) {
  const [comfyUrl, setComfyUrl] = useState('http://127.0.0.1:8188');
  const [llmUrl, setLlmUrl] = useState('');
  const [llmModel, setLlmModel] = useState('');
  const [llmKey, setLlmKey] = useState('');
  const [openaiCompat, setOpenaiCompat] = useState(false);

  const saveConfig = async () => {
    const config = {
      comfyui_url: comfyUrl,
      enable_openai_compat: openaiCompat,
      llm: llmUrl ? { base_url: llmUrl, model: llmModel || null, api_key: llmKey } : null,
      workflows: {}
    };

    await fetch('/api/config', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${token}`
      },
      body: JSON.stringify(config)
    });
    onComplete();
  };

  return (
    <div className="onboarding-container">
      <div className="panel" style={{ width: 500 }}>
        <h2 style={{ marginBottom: 20 }}>Welcome to Comfy-Serve</h2>
        <p style={{ marginBottom: 20, color: 'var(--text-muted)' }}>Let's set up your environment.</p>
        
        <div className="form-group">
          <label>ComfyUI URL</label>
          <input type="text" value={comfyUrl} onChange={e => setComfyUrl(e.target.value)} />
        </div>

        <h3 style={{ marginTop: 20, marginBottom: 10 }}>LLM Assistant (Optional)</h3>
        <p style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 10 }}>Used for Assisted Restructure.</p>
        <div className="form-group">
          <label>OpenAI Compatible Endpoint URL</label>
          <input type="text" value={llmUrl} onChange={e => setLlmUrl(e.target.value)} placeholder="https://api.openai.com/v1" />
        </div>
        <div className="form-group">
          <label>Model (Optional)</label>
          <input type="text" value={llmModel} onChange={e => setLlmModel(e.target.value)} placeholder="e.g. gpt-4o (Leave blank for local vLLM)" />
        </div>
        <div className="form-group">
          <label>API Key</label>
          <input type="password" value={llmKey} onChange={e => setLlmKey(e.target.value)} />
        </div>

        <div className="form-group" style={{ flexDirection: 'row', alignItems: 'center', marginTop: 20 }}>
          <input type="checkbox" checked={openaiCompat} onChange={e => setOpenaiCompat(e.target.checked)} />
          <label>Enable OpenAI Compatible Image Gen API Endpoint</label>
        </div>

        <button onClick={saveConfig} style={{ width: '100%', marginTop: 20 }}>Save & Continue</button>
      </div>
    </div>
  );
}

function Dashboard({ token }: { token: string }) {
  const [workflows, setWorkflows] = useState<Record<string, any>>({});
  const [selectedWf, setSelectedWf] = useState<string | null>(null);
  const [config, setConfig] = useState<any>(null);
  
  useEffect(() => {
    fetch('/api/workflows', { headers: { 'Authorization': `Bearer ${token}` } })
      .then(r => r.json())
      .then(data => setWorkflows(data || {}));

    fetch('/api/config', { headers: { 'Authorization': `Bearer ${token}` } })
      .then(r => r.json())
      .then(data => setConfig(data));
  }, [token]);

  const [currentView, setCurrentView] = useState<'editor' | 'test_api'>('editor');

  const saveConfig = async () => {
    await fetch('/api/config', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${token}`
      },
      body: JSON.stringify(config)
    });
    alert('Config saved!');
  };

  const handleWorkflowClick = (wf: string) => {
    setSelectedWf(wf);
    setCurrentView('editor');
    if (!config.workflows[wf]) {
      setConfig({
        ...config,
        workflows: {
          ...config.workflows,
          [wf]: { active: true, file_name: wf, exposed_fields: [] }
        }
      });
    }
  };

  const workflowNames = Object.keys(workflows);

  return (
    <div className="dashboard-container">
      <div className="sidebar">
        <h3 style={{ marginBottom: 20 }}>Comfy-Serve</h3>
        <button onClick={saveConfig} style={{ marginBottom: 20 }}>Save & Reload</button>
        
        <h4 style={{ marginBottom: 10, color: 'var(--text-muted)' }}>Workflows</h4>
        <ul style={{ listStyle: 'none', padding: 0, marginBottom: 20 }}>
          {workflowNames.map(wf => (
            <li 
              key={wf} 
              onClick={() => handleWorkflowClick(wf)}
              style={{ 
                padding: '10px 10px', 
                borderBottom: '1px solid var(--border-color)', 
                cursor: 'pointer',
                backgroundColor: currentView === 'editor' && selectedWf === wf ? 'var(--accent)' : 'transparent',
                borderRadius: 4
              }}>
              {wf}
            </li>
          ))}
        </ul>
        {workflowNames.length === 0 && <p style={{ color: 'var(--text-muted)', fontSize: 14, marginBottom: 20 }}>No workflows found.</p>}

        <h4 style={{ marginBottom: 10, color: 'var(--text-muted)' }}>Tools</h4>
        <ul style={{ listStyle: 'none', padding: 0 }}>
          <li 
            onClick={() => setCurrentView('test_api')}
            style={{ 
              padding: '10px 10px', 
              cursor: 'pointer',
              backgroundColor: currentView === 'test_api' ? 'var(--accent)' : 'transparent',
              borderRadius: 4
            }}>
            Test API
          </li>
        </ul>

        <div style={{ marginTop: 'auto', paddingTop: 20, textAlign: 'center', fontSize: 13, color: 'var(--text-muted)' }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 6 }}>
            Made with 
            <svg width="18" height="18" viewBox="0 0 24 24" className="heart-pulse">
              <path d="M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z"/>
            </svg>
            by A'eala
          </div>
        </div>
      </div>
      <div className="workspace">
        {currentView === 'test_api' ? (
          <TestApiView config={config} />
        ) : (
          selectedWf && config && config.workflows[selectedWf] ? (
            <WorkspaceEditor wf={selectedWf} wfJson={workflows[selectedWf]} config={config} setConfig={setConfig} />
          ) : (
            <div style={{ display: 'flex', height: '100%', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)' }}>
              Select a workflow from the left to edit, or Test API.
            </div>
          )
        )}
      </div>
    </div>
  );
}

function WorkspaceEditor({ wf, wfJson, config, setConfig }: any) {
  const wfConfig = config.workflows[wf];
  const [showPopup, setShowPopup] = useState(false);
  const [llmPrompt, setLlmPrompt] = useState("Analyze this workflow and expose the fields that control prompt, CFG scale, steps, and random seed.");
  const [llmModelOverride, setLlmModelOverride] = useState("");
  
  // JSON stringified state for the right panel editor
  const [jsonText, setJsonText] = useState(JSON.stringify(wfJson || {}, null, 2) || "");

  // Sync state if a new workflow is clicked
  useEffect(() => {
    setJsonText(JSON.stringify(wfJson || {}, null, 2) || "");
  }, [wfJson]);

  const addExposedField = () => {
    const newField = {
      original_node_id: "1",
      original_field_name: "text",
      exposed_as: "prompt",
      required: false,
      input_target: "text",
      is_value_map: false,
      map_keys: "",
      map_values: ""
    };
    setConfig({
      ...config,
      workflows: {
        ...config.workflows,
        [wf]: {
          ...(wfConfig || { active: true, file_name: wf, exposed_fields: [] }),
          exposed_fields: [...(wfConfig?.exposed_fields || []), newField]
        }
      }
    });
  };

  const handleAssistedRestructure = async () => {
    try {
      const token = localStorage.getItem('token');
      const res = await fetch('/api/restructure', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${token}`
        },
        body: JSON.stringify({ workflow: wf, prompt: llmPrompt, model: llmModelOverride })
      });
      
      if (!res.ok) {
        const err = await res.text();
        alert(`Restructure failed: ${err}`);
        return;
      }
      
      const mappings = await res.json();
      
      setConfig((prev: any) => ({
        ...prev,
        workflows: {
          ...prev.workflows,
          [wf]: {
            ...(prev.workflows[wf] || { active: true, file_name: wf, exposed_fields: [] }),
            exposed_fields: [...(prev.workflows[wf]?.exposed_fields || []), ...mappings]
          }
        }
      }));
      
      setShowPopup(false);
      alert("Restructure mapping successful! Check your left panel.");
    } catch (e: any) {
      alert(`Error calling restructure: ${e.message}`);
    }
  };

  return (
    <div style={{ display: 'flex', height: '100%', gap: 20 }}>
      {/* Left pane: Manual Mappings */}
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflowY: 'auto', padding: '4px' }}>
        <h2>Editing: {wf}</h2>
        <div style={{ marginTop: 20 }}>
          <button onClick={addExposedField} style={{ marginRight: 10 }}>+ Add Mapped Field (Manual)</button>
          <button className="accent-btn" onClick={() => setShowPopup(true)}>Restructure (Assisted)</button>
        </div>

        <div style={{ marginTop: 30 }}>
          {wfConfig?.exposed_fields?.map((field: any, idx: number) => (
            <div key={idx} className="panel" style={{ marginBottom: 15 }}>
              <div className="form-group" style={{ flexDirection: 'row', alignItems: 'center', gap: 15 }}>
                <div>
                  <label style={{ fontSize: 12, display: 'block' }}>Node ID</label>
                  <input type="text" style={{ width: 60 }} value={field.original_node_id} onChange={e => {
                    const newFields = [...(wfConfig?.exposed_fields || [])];
                    newFields[idx].original_node_id = e.target.value;
                    setConfig({ ...config, workflows: { ...config.workflows, [wf]: { ...wfConfig, exposed_fields: newFields } } });
                  }} />
                </div>
                <div>
                  <label style={{ fontSize: 12, display: 'block' }}>Node Field</label>
                  <input type="text" style={{ width: 120 }} value={field.original_field_name} onChange={e => {
                    const newFields = [...(wfConfig?.exposed_fields || [])];
                    newFields[idx].original_field_name = e.target.value;
                    setConfig({ ...config, workflows: { ...config.workflows, [wf]: { ...wfConfig, exposed_fields: newFields } } });
                  }} />
                </div>
                <div style={{ fontSize: 20 }}>&rarr;</div>
                <div>
                  <label style={{ fontSize: 12, display: 'block' }}>Exposed API Param</label>
                  <input type="text" style={{ width: 120 }} value={field.exposed_as} onChange={e => {
                    const newFields = [...(wfConfig?.exposed_fields || [])];
                    newFields[idx].exposed_as = e.target.value;
                    setConfig({ ...config, workflows: { ...config.workflows, [wf]: { ...wfConfig, exposed_fields: newFields } } });
                  }} />
                </div>
              </div>
              <div className="form-group" style={{ flexDirection: 'row', alignItems: 'center', gap: 15, marginTop: 10 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
                  <input type="checkbox" checked={field.required} onChange={e => {
                    const newFields = [...(wfConfig?.exposed_fields || [])];
                    newFields[idx].required = e.target.checked;
                    setConfig({ ...config, workflows: { ...config.workflows, [wf]: { ...wfConfig, exposed_fields: newFields } } });
                  }} />
                  <label style={{ fontSize: 12 }}>Required?</label>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
                  <input type="checkbox" checked={field.is_value_map} onChange={e => {
                    const newFields = [...(wfConfig?.exposed_fields || [])];
                    newFields[idx].is_value_map = e.target.checked;
                    setConfig({ ...config, workflows: { ...config.workflows, [wf]: { ...wfConfig, exposed_fields: newFields } } });
                  }} />
                  <label style={{ fontSize: 12 }}>Value Map?</label>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
                  <label style={{ fontSize: 12 }}>Input Target:</label>
                  <select 
                    value={field.input_target || 'text'} 
                    onChange={e => {
                      const newFields = [...(wfConfig?.exposed_fields || [])];
                      newFields[idx].input_target = e.target.value;
                      setConfig({ ...config, workflows: { ...config.workflows, [wf]: { ...wfConfig, exposed_fields: newFields } } });
                    }}
                    style={{ padding: '2px 5px', fontSize: 12, backgroundColor: '#15151e', border: '1px solid var(--border-color)', color: 'var(--text-main)', borderRadius: 4 }}
                  >
                    <option value="text">Text (Default)</option>
                    <option value="image_base64">Image (Base64 Node)</option>
                    <option value="image_url">Image (URL Node)</option>
                    <option value="comfy_upload">Image (Upload to ComfyUI)</option>
                  </select>
                </div>
                
                <button onClick={() => {
                  const newFields = [...(wfConfig?.exposed_fields || [])];
                  newFields.splice(idx, 1);
                  setConfig({ ...config, workflows: { ...config.workflows, [wf]: { ...wfConfig, exposed_fields: newFields } } });
                }} className="danger-btn" style={{ padding: '6px 10px', marginLeft: 'auto' }}>X</button>
              </div>
              {field.is_value_map && (
                <div className="form-group" style={{ flexDirection: 'row', alignItems: 'center', gap: 15, marginTop: 10, padding: 10, backgroundColor: '#15151e', borderRadius: 4 }}>
                  <div>
                    <label style={{ fontSize: 12, display: 'block', color: 'var(--text-muted)' }}>Incoming Values (comma separated)</label>
                    <input type="text" style={{ width: 180 }} placeholder="e.g. true,false" value={field.map_keys} onChange={e => {
                      const newFields = [...(wfConfig?.exposed_fields || [])];
                      newFields[idx].map_keys = e.target.value;
                      setConfig({ ...config, workflows: { ...config.workflows, [wf]: { ...wfConfig, exposed_fields: newFields } } });
                    }} />
                  </div>
                  <div>
                    <label style={{ fontSize: 12, display: 'block', color: 'var(--text-muted)' }}>Mapped Output (comma separated)</label>
                    <input type="text" style={{ width: 180 }} placeholder="e.g. 0,0.9" value={field.map_values} onChange={e => {
                      const newFields = [...(wfConfig?.exposed_fields || [])];
                      newFields[idx].map_values = e.target.value;
                      setConfig({ ...config, workflows: { ...config.workflows, [wf]: { ...wfConfig, exposed_fields: newFields } } });
                    }} />
                  </div>
                </div>
              )}
            </div>
          ))}
          {(!wfConfig || !wfConfig.exposed_fields || wfConfig.exposed_fields.length === 0) && <p style={{ color: 'var(--text-muted)' }}>No fields mapped yet.</p>}
        </div>
      </div>

      {/* Right pane: JSON Viewer/Editor */}
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', backgroundColor: '#1d1d26', borderRadius: 8, overflow: 'hidden', border: '1px solid var(--border-color)', resize: 'horizontal' }}>
        <div style={{ padding: '10px 15px', backgroundColor: 'var(--panel-bg)', borderBottom: '1px solid var(--border-color)', fontWeight: 600 }}>
          Workflow JSON (Reference)
        </div>
        <div style={{ flex: 1, overflowY: 'auto' }}>
          <pre style={{
            margin: 0,
            padding: 15,
            fontFamily: '"Fira code", "Fira Mono", monospace',
            fontSize: 12,
            backgroundColor: 'transparent',
            minHeight: '100%',
            whiteSpace: 'pre-wrap',
            wordWrap: 'break-word'
          }}>
            <code dangerouslySetInnerHTML={{
              __html: (Prism && Prism.languages && Prism.languages.json) 
                ? Prism.highlight(jsonText, Prism.languages.json, 'json')
                : jsonText
            }} />
          </pre>
        </div>
      </div>

      {/* Popup Modal */}
      {showPopup && (
        <div style={{ position: 'fixed', top: 0, left: 0, right: 0, bottom: 0, backgroundColor: 'rgba(0,0,0,0.7)', display: 'flex', justifyContent: 'center', alignItems: 'center', zIndex: 1000 }}>
          <div className="panel" style={{ width: 500 }}>
            <h2 style={{ marginBottom: 15 }}>Restructure (Assisted)</h2>
            <p style={{ color: 'var(--text-muted)', marginBottom: 15, fontSize: 14 }}>
              Provide a prompt to instruct the LLM on how to automatically detect and map the exposed fields. For example, specify if certain fields should be "value toggles" (like passing turbo=true sets strength to 0.9).
            </p>
            <div className="form-group" style={{ marginBottom: 15 }}>
              <label>Model Override (Optional)</label>
              <input type="text" value={llmModelOverride} onChange={e => setLlmModelOverride(e.target.value)} placeholder="e.g. gpt-4o (Leave blank to use default)" style={{ width: '100%' }} />
            </div>
            <textarea 
              value={llmPrompt}
              onChange={e => setLlmPrompt(e.target.value)}
              style={{ width: '100%', height: 120, padding: 10, backgroundColor: '#15151e', border: '1px solid var(--border-color)', color: 'var(--text-main)', borderRadius: 4, marginBottom: 15, fontFamily: 'inherit' }}
            />
            <div style={{ display: 'flex', gap: 10, justifyContent: 'flex-end' }}>
              <button className="secondary-btn" onClick={() => setShowPopup(false)}>Cancel</button>
              <button onClick={handleAssistedRestructure}>Submit to LLM</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function TestApiView({ config }: { config: any }) {
  const [selectedWf, setSelectedWf] = useState<string>('');
  const [params, setParams] = useState<Record<string, string>>({});
  const [activeTab, setActiveTab] = useState<'curl' | 'python'>('curl');
  const [isGenerating, setIsGenerating] = useState(false);
  const [resultImage, setResultImage] = useState<string | null>(null);
  const [resultError, setResultError] = useState<string | null>(null);
  const [testApiKey, setTestApiKey] = useState('');
  const [showApiKey, setShowApiKey] = useState(false);
  const [showSnippets, setShowSnippets] = useState(true);
  const [copied, setCopied] = useState(false);

  if (!config || !config.workflows) {
    return <div style={{ padding: 20 }}>No config loaded.</div>;
  }

  const activeWorkflows = Object.entries(config.workflows)
    .filter(([_, c]: any) => c.active)
    .map(([k, _]) => k);

  const wfConfig = selectedWf ? config.workflows[selectedWf] : null;

  // Auto-select first if none
  useEffect(() => {
    if (!selectedWf && activeWorkflows.length > 0) {
      setSelectedWf(activeWorkflows[0]);
    }
  }, [activeWorkflows, selectedWf]);

  const curlBody = JSON.stringify({
    workflow: selectedWf,
    params: params
  }, null, 4);
  const escapedCurlBody = curlBody.replace(/'/g, "'\\''");
  
  const authHeaderCurl = testApiKey ? ` \\\n  -H "Authorization: Bearer ${testApiKey}"` : "";
  const curlReq = `curl -X POST http://127.0.0.1:3000/api/generate \\
  -H "Content-Type: application/json"${authHeaderCurl} \\
  -d '${escapedCurlBody}'`;

  const authHeaderPython = testApiKey ? `, "Authorization": f"Bearer {api_key}"` : "";
  const pythonReq = `import requests

url = "http://127.0.0.1:3000/api/generate"
data = '''${JSON.stringify({
    workflow: selectedWf,
    params: params
}, null, 4)}'''
${testApiKey ? `\napi_key = "${testApiKey}"` : ""}
response = requests.post(url, data=data, headers={"Content-Type": "application/json"${authHeaderPython}})

with open("output.png", "wb") as f:
    f.write(response.content)
`;

  const displayApiKey = testApiKey ? (showApiKey ? testApiKey : "••••••••••••••••") : "";
  const authHeaderCurlDisplay = displayApiKey ? ` \\\n  -H "Authorization: Bearer ${displayApiKey}"` : "";
  const curlReqDisplay = `curl -X POST http://127.0.0.1:3000/api/generate \\
  -H "Content-Type: application/json"${authHeaderCurlDisplay} \\
  -d '${escapedCurlBody}'`;

  const authHeaderPythonDisplay = displayApiKey ? `, "Authorization": f"Bearer {api_key}"` : "";
  const pythonReqDisplay = `import requests

url = "http://127.0.0.1:3000/api/generate"
data = '''${JSON.stringify({
    workflow: selectedWf,
    params: params
}, null, 4)}'''
${displayApiKey ? `\napi_key = "${displayApiKey}"` : ""}
response = requests.post(url, data=data, headers={"Content-Type": "application/json"${authHeaderPythonDisplay}})

with open("output.png", "wb") as f:
    f.write(response.content)
`;

  const handleCopy = () => {
    const textToCopy = activeTab === 'curl' ? curlReq : pythonReq;
    navigator.clipboard.writeText(textToCopy);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleRun = async () => {
    setIsGenerating(true);
    setResultImage(null);
    setResultError(null);
    
    const headers: any = { 'Content-Type': 'application/json' };
    if (testApiKey) {
      headers['Authorization'] = `Bearer ${testApiKey}`;
    }

    try {
      const res = await fetch('/api/generate', {
        method: 'POST',
        headers,
        body: JSON.stringify({ workflow: selectedWf, params })
      });
      if (!res.ok) {
        const err = await res.text();
        throw new Error(err || 'Generation failed');
      }
      const blob = await res.blob();
      setResultImage(URL.createObjectURL(blob));
    } catch (e: any) {
      setResultError(e.message);
    } finally {
      setIsGenerating(false);
    }
  };

  return (
    <div style={{ padding: 40, display: 'flex', gap: 40, height: '100%', overflowY: 'auto', boxSizing: 'border-box' }}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <h2 style={{ marginBottom: 20 }}>Test API Endpoints</h2>
        
        <div className="form-group" style={{ marginBottom: 15 }}>
          <label>API Key (Optional)</label>
          <div style={{ display: 'flex', gap: 10 }}>
            <input 
              type={showApiKey ? "text" : "password"} 
              value={testApiKey} 
              onChange={e => setTestApiKey(e.target.value)}
              placeholder="sk-..."
              style={{ flex: 1, padding: 10, backgroundColor: '#15151e', border: '1px solid var(--border-color)', color: 'var(--text-main)', borderRadius: 4 }}
            />
            <button className="secondary-btn" onClick={() => setShowApiKey(!showApiKey)} title="Toggle API Key Visibility" style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', padding: '0 12px' }}>
              {showApiKey ? (
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"></path>
                  <line x1="1" y1="1" x2="23" y2="23"></line>
                </svg>
              ) : (
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"></path>
                  <circle cx="12" cy="12" r="3"></circle>
                </svg>
              )}
            </button>
          </div>
        </div>

        <div className="form-group" style={{ marginBottom: 30 }}>
          <label>Target Workflow</label>
          <select 
            value={selectedWf} 
            onChange={e => { setSelectedWf(e.target.value); setParams({}); }}
            style={{ width: '100%', padding: 10, backgroundColor: '#15151e', border: '1px solid var(--border-color)', color: 'var(--text-main)', borderRadius: 4 }}
          >
            <option value="" disabled>Select a workflow...</option>
            {activeWorkflows.map(wf => (
              <option key={wf} value={wf}>{wf}</option>
            ))}
          </select>
        </div>

        {wfConfig && wfConfig.exposed_fields && wfConfig.exposed_fields.length > 0 ? (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
            <h3 style={{ marginBottom: 15 }}>Parameters</h3>
            {wfConfig.exposed_fields.map((field: any, idx: number) => (
              <div key={idx} className="form-group" style={{ marginBottom: 15, padding: 15, backgroundColor: '#15151e', borderRadius: 6, border: '1px solid var(--border-color)' }}>
                <label style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                  <span>{field.exposed_as} {field.required && <span style={{ color: 'var(--danger)' }}>*</span>}</span>
                </label>
                <input 
                  type="text" 
                  value={params[field.exposed_as] || ''} 
                  onChange={e => setParams({ ...params, [field.exposed_as]: e.target.value })}
                  placeholder={field.is_value_map ? `Mapped Values: ${field.map_keys}` : "Value"}
                  style={{ width: '100%', marginTop: 8 }}
                />
              </div>
            ))}
            <button 
              onClick={handleRun} 
              disabled={isGenerating}
              className={isGenerating ? "secondary-btn" : "accent-btn"}
              style={{ 
                padding: '15px', 
                fontSize: 16, 
                fontWeight: 'bold', 
                cursor: isGenerating ? 'not-allowed' : 'pointer',
              }}
            >
              {isGenerating ? 'Generating...' : '🚀 Run Generation'}
            </button>
          </div>
        ) : (
          <p style={{ color: 'var(--text-muted)' }}>No exposed fields for this workflow.</p>
        )}
      </div>
      
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        {resultError && (
          <div style={{ padding: 15, backgroundColor: 'var(--danger)', color: 'white', borderRadius: 8, marginBottom: 20, fontSize: 14 }}>
            <strong>Error:</strong> {resultError}
          </div>
        )}
        {resultImage && (
          <div style={{ marginBottom: 20, textAlign: 'center' }}>
            <img src={resultImage} style={{ maxWidth: '100%', maxHeight: '400px', borderRadius: 8, border: '1px solid var(--border-color)', boxShadow: '0 4px 12px rgba(0,0,0,0.5)' }} alt="Generated Result" />
            <div style={{ marginTop: 10 }}>
              <a href={resultImage} download="result.png" style={{ color: 'var(--accent)', fontSize: 14 }}>Download Image</a>
            </div>
          </div>
        )}
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', borderBottom: '1px solid var(--border-color)', marginBottom: showSnippets ? 20 : 0 }}>
          <div style={{ display: 'flex' }}>
            <button 
              onClick={() => setActiveTab('curl')}
              className="flat-btn"
              style={{ color: activeTab === 'curl' ? 'var(--accent)' : 'var(--text-muted)', borderBottom: activeTab === 'curl' ? '2px solid var(--accent)' : '2px solid transparent', borderRadius: 0, padding: '10px 20px' }}>
              cURL
            </button>
            <button 
              onClick={() => setActiveTab('python')}
              className="flat-btn"
              style={{ color: activeTab === 'python' ? 'var(--accent)' : 'var(--text-muted)', borderBottom: activeTab === 'python' ? '2px solid var(--accent)' : '2px solid transparent', borderRadius: 0, padding: '10px 20px' }}>
              Python
            </button>
          </div>
          <div style={{ display: 'flex', gap: 10, paddingBottom: 10 }}>
            {showSnippets && <button onClick={handleCopy} className="secondary-btn" style={{ padding: '6px 12px', display: 'flex', alignItems: 'center', gap: 6 }}>
              {copied ? (
                <>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>
                  Copied!
                </>
              ) : (
                <>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>
                  Copy
                </>
              )}
            </button>}
            <button onClick={() => setShowSnippets(!showSnippets)} className="secondary-btn" style={{ padding: '6px 12px', display: 'flex', alignItems: 'center', gap: 6 }}>
              {showSnippets ? (
                <>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="18 15 12 9 6 15"></polyline></svg>
                  Hide
                </>
              ) : (
                <>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="6 9 12 15 18 9"></polyline></svg>
                  Show Snippets
                </>
              )}
            </button>
          </div>
        </div>

        {showSnippets && (
          <pre style={{ backgroundColor: '#1d1d26', padding: 20, borderRadius: 8, overflowX: 'auto', flex: 1, border: '1px solid var(--border-color)', margin: 0, fontSize: 13, lineHeight: '1.5', whiteSpace: 'pre-wrap', wordWrap: 'break-word', minHeight: 0 }}>
            <code>
              {activeTab === 'curl' ? curlReqDisplay : pythonReqDisplay}
            </code>
          </pre>
        )}
      </div>
    </div>
  );
}

export default function Root() {
  return <ErrorBoundary><App /></ErrorBoundary>;
}
