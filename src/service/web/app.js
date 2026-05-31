const API_URL = "";
let pollTimer = null;

// Form Submit for login
document.getElementById('login-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const u = document.getElementById('username').value;
    const p = document.getElementById('password').value;
    const errorToast = document.getElementById('login-error');
    errorToast.style.display = 'none';

    try {
        const res = await fetch(`${API_URL}/api/login`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ username: u, password: p })
        });

        if (res.ok) {
            const data = await res.json();
            localStorage.setItem('sip_session_token', data.token);
            initApp();
        } else {
            errorToast.style.display = 'block';
        }
    } catch (err) {
        console.error(err);
        errorToast.innerText = "Network error. Failed to reach web server.";
        errorToast.style.display = 'block';
    }
});

// Trigger Auto Answer IVR subfields visibility
document.getElementById('acc-auto-answer').addEventListener('change', (e) => {
    const ivrFields = document.getElementById('ivr-subfields');
    ivrFields.style.display = e.target.checked ? 'block' : 'none';
});

function getToken() {
    return localStorage.getItem('sip_session_token');
}

function getAuthHeaders() {
    return {
        'Authorization': `Bearer ${getToken()}`,
        'Content-Type': 'application/json'
    };
}

// Initialize application layout
function initApp() {
    const token = getToken();
    if (!token) {
        document.getElementById('login-screen').style.display = 'flex';
        document.getElementById('app-layout').style.display = 'none';
        if (pollTimer) clearInterval(pollTimer);
        return;
    }

    document.getElementById('login-screen').style.display = 'none';
    document.getElementById('app-layout').style.display = 'flex';

    // Start live status polling
    updateDashboard();
    pollTimer = setInterval(updateDashboard, 1000);
    
    // Read configured accounts
    loadAccountsConfig();
}

function logout() {
    localStorage.removeItem('sip_session_token');
    initApp();
}

// Manage navigation tabs
function switchTab(tabId) {
    document.querySelectorAll('.tab-content').forEach(el => el.classList.remove('active'));
    document.querySelectorAll('.tab-btn').forEach(el => el.classList.remove('active'));
    
    document.getElementById(`tab-${tabId}`).classList.add('active');
    event.target.classList.add('active');

    if (tabId === 'accounts') {
        loadAccountsConfig();
    } else if (tabId === 'settings') {
        loadGlobalSettings();
    }
}

// Format duration into hh:mm:ss
function formatDuration(sec) {
    const hrs = Math.floor(sec / 3600).toString().padStart(2, '0');
    const mins = Math.floor((sec % 3600) / 60).toString().padStart(2, '0');
    const secs = (sec % 60).toString().padStart(2, '0');
    return `${hrs}:${mins}:${secs}`;
}

// Audio stream state
let activeAudioSession = {
    accountName: null,
    ws: null,
    audioCtx: null,
    micStream: null,
    sampleQueue: [],
    playbackNode: null,
    captureNode: null,
    micSource: null
};

async function toggleJoinCall(accountName, codecRate) {
    if (activeAudioSession.accountName === accountName) {
        leaveCallAudio();
    } else {
        if (activeAudioSession.accountName) {
            leaveCallAudio();
        }
        await joinCallAudio(accountName, codecRate);
    }
}

async function joinCallAudio(accountName, codecRate) {
    try {
        // 1. Get microphone permission
        const micStream = await navigator.mediaDevices.getUserMedia({ audio: true });
        
        // 2. Open WebSocket connection
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const host = window.location.host;
        const token = getToken();
        const wsUrl = `${protocol}//${host}/api/accounts/${accountName}/audio-ws?token=${token}`;
        
        const ws = new WebSocket(wsUrl);
        ws.binaryType = 'arraybuffer';

        // 3. Setup AudioContext
        const audioCtx = new (window.AudioContext || window.webkitAudioContext)({ sampleRate: codecRate });
        const sampleQueue = [];

        // 4. Playback node (receiving PCM from WebSocket)
        const bufferSize = 2048;
        const playbackNode = audioCtx.createScriptProcessor(bufferSize, 0, 1);
        playbackNode.onaudioprocess = function(e) {
            const outputBuffer = e.outputBuffer.getChannelData(0);
            for (let i = 0; i < outputBuffer.length; i++) {
                outputBuffer[i] = sampleQueue.shift() || 0.0;
            }
        };
        playbackNode.connect(audioCtx.destination);

        // 5. Capture node (sending microphone audio over WS)
        const micSource = audioCtx.createMediaStreamSource(micStream);
        const captureNode = audioCtx.createScriptProcessor(bufferSize, 1, 0);
        captureNode.onaudioprocess = function(e) {
            const inputBuffer = e.inputBuffer.getChannelData(0);
            const i16Samples = new Int16Array(inputBuffer.length);
            for (let i = 0; i < inputBuffer.length; i++) {
                i16Samples[i] = Math.max(-32768, Math.min(32767, inputBuffer[i] * 32768));
            }
            if (ws.readyState === WebSocket.OPEN) {
                ws.send(i16Samples.buffer);
            }
        };
        micSource.connect(captureNode);
        captureNode.connect(audioCtx.destination);

        ws.onmessage = function(event) {
            const intData = new Int16Array(event.data);
            for (let i = 0; i < intData.length; i++) {
                sampleQueue.push(intData[i] / 32768.0);
            }
            if (sampleQueue.length > codecRate * 1.5) {
                sampleQueue.splice(0, sampleQueue.length - codecRate);
            }
        };

        ws.onclose = function() {
            console.log("Audio WebSocket closed.");
            if (activeAudioSession.accountName === accountName) {
                leaveCallAudio();
            }
        };

        ws.onerror = function(err) {
            console.error("Audio WebSocket error:", err);
        };

        activeAudioSession = {
            accountName,
            ws,
            audioCtx,
            micStream,
            sampleQueue,
            playbackNode,
            captureNode,
            micSource
        };

        document.getElementById('audio-session-account-name').innerText = accountName;
        document.getElementById('audio-session-banner').style.display = 'flex';
        
        updateDashboard();

    } catch (err) {
        console.error("Failed to join call audio:", err);
        alert("Could not access microphone or connect to audio service: " + err.message);
    }
}

function leaveCallAudio() {
    if (!activeAudioSession.accountName) return;

    console.log("Leaving call audio session for:", activeAudioSession.accountName);

    if (activeAudioSession.ws) {
        activeAudioSession.ws.close();
    }

    if (activeAudioSession.micStream) {
        activeAudioSession.micStream.getTracks().forEach(track => track.stop());
    }

    if (activeAudioSession.micSource && activeAudioSession.captureNode) {
        activeAudioSession.micSource.disconnect();
        activeAudioSession.captureNode.disconnect();
    }
    if (activeAudioSession.playbackNode) {
        activeAudioSession.playbackNode.disconnect();
    }

    if (activeAudioSession.audioCtx) {
        activeAudioSession.audioCtx.close();
    }

    activeAudioSession = {
        accountName: null,
        ws: null,
        audioCtx: null,
        micStream: null,
        sampleQueue: [],
        playbackNode: null,
        captureNode: null,
        micSource: null
    };

    document.getElementById('audio-session-banner').style.display = 'none';
    updateDashboard();
}

// Poll system statistics, active calls, and registration status
async function updateDashboard() {
    try {
        const res = await fetch(`${API_URL}/api/status`, { headers: getAuthHeaders() });
        if (res.status === 401) {
            logout();
            return;
        }
        if (!res.ok) return;

        const status = await res.json();

        // Update Dialer Dropdown
        const dialerSelect = document.getElementById('dialer-account');
        const previousSelection = dialerSelect.value;
        dialerSelect.innerHTML = '';
        status.accounts.forEach(acc => {
            const opt = document.createElement('option');
            opt.value = acc.name;
            opt.innerText = acc.name;
            dialerSelect.appendChild(opt);
        });
        if (previousSelection && Array.from(dialerSelect.options).some(o => o.value === previousSelection)) {
            dialerSelect.value = previousSelection;
        }

        // Set quick stats
        document.getElementById('stat-total-accounts').innerText = status.total_accounts;
        document.getElementById('stat-registered-accounts').innerText = status.registered_accounts;
        document.getElementById('stat-active-calls').innerText = status.active_calls;
        document.getElementById('stat-cpu-percent').innerText = `${status.cpu_percent.toFixed(1)} %`;

        // Set diagnostics panel
        document.getElementById('diag-os').innerText = status.os_name;
        document.getElementById('diag-mem').innerText = `${(status.memory_bytes / (1024 * 1024)).toFixed(1)} MB`;
        document.getElementById('diag-cpu').innerText = `${status.cpu_percent.toFixed(1)} %`;
        document.getElementById('uptime').innerText = `Uptime: ${formatDuration(status.uptime_secs)}`;

        // Build SIP Bindings Table (Dashboard tab)
        const bindingsBody = document.getElementById('bindings-monitor-body');
        bindingsBody.innerHTML = '';
        
        if (status.accounts.length === 0) {
            bindingsBody.innerHTML = `<tr><td colspan="5" style="text-align: center; color: var(--text-secondary);">No accounts configured.</td></tr>`;
        } else {
            status.accounts.forEach(acc => {
                const statusBadge = acc.registered 
                    ? `<span class="badge badge-success">Registered</span>` 
                    : `<span class="badge badge-warning">Unregistered</span>`;
                
                const actions = acc.registered
                    ? `<button class="btn btn-warning action-btn action-btn-sm" onclick="unregisterAccount('${acc.name}')">Deregister</button>`
                    : `<button class="btn btn-success action-btn action-btn-sm" onclick="registerAccount('${acc.name}')">Register</button>`;

                const tr = document.createElement('tr');
                tr.innerHTML = `
                    <td style="font-weight:600;">${acc.name}</td>
                    <td>sip:${acc.username}@${acc.domain}</td>
                    <td>${acc.sip_port}</td>
                    <td>${statusBadge}</td>
                    <td>${actions}</td>
                `;
                bindingsBody.appendChild(tr);
            });
        }

        // If we have an active audio session, but the account is no longer in a call, disconnect
        if (activeAudioSession.accountName) {
            const matched = status.accounts.find(a => a.name === activeAudioSession.accountName && a.in_call);
            if (!matched) {
                console.log("Active call ended, disconnecting audio session.");
                leaveCallAudio();
            }
        }

        // Build Active Calls Table
        const callsBody = document.getElementById('active-calls-body');
        callsBody.innerHTML = '';
        const activeCalls = status.accounts.filter(a => a.in_call);
        if (activeCalls.length === 0) {
            callsBody.innerHTML = `<tr><td colspan="5" style="text-align: center; color: var(--text-secondary);">No active calls ongoing.</td></tr>`;
        } else {
            activeCalls.forEach(call => {
                const tr = document.createElement('tr');
                const isJoined = activeAudioSession.accountName === call.name;
                const joinText = isJoined ? "Leave Audio" : "Join Audio";
                const joinClass = isJoined ? "btn-danger" : "btn-success";
                const stateBadge = call.held 
                    ? `<span class="badge badge-warning" style="animation: pulse 2s infinite;">HELD</span>` 
                    : `<span class="badge badge-success" style="animation: pulse 1.5s infinite;">IN CALL</span>`;
                tr.innerHTML = `
                    <td style="font-weight:600;">${call.name}</td>
                    <td>${call.server}</td>
                    <td style="font-family: var(--font-mono); font-size:0.8rem;">${call.call_id || '-'}</td>
                    <td>${stateBadge}</td>
                    <td style="display: flex; gap: 0.35rem; align-items: center; flex-wrap: wrap;">
                        <button class="btn ${joinClass} action-btn action-btn-sm" style="width:auto; padding: 0.35rem 0.6rem; font-size: 0.75rem;" onclick="toggleJoinCall('${call.name}', ${call.codec_rate})">${joinText}</button>
                        <button class="btn btn-warning action-btn action-btn-sm" style="width:auto; padding: 0.35rem 0.6rem; font-size: 0.75rem;" onclick="toggleHoldCall('${call.name}', ${call.held})">${call.held ? 'Resume' : 'Hold'}</button>
                        <button class="btn btn-danger action-btn action-btn-sm" style="width:auto; padding: 0.35rem 0.6rem; font-size: 0.75rem;" onclick="hangupCall('${call.name}')">Hangup</button>
                        
                        <div style="display: inline-flex; gap: 0.2rem; background: rgba(255,255,255,0.05); padding: 0.2rem; border-radius: var(--radius-sm); border: 1px solid var(--border-color);">
                            <input type="text" id="dtmf-${call.name}" placeholder="DTMF" style="width: 50px; background: transparent; border: none; color: #fff; font-size: 0.75rem; outline: none; text-align: center;">
                            <button class="btn btn-primary action-btn action-btn-sm" style="width:auto; padding: 0.2rem 0.4rem; font-size: 0.7rem; border-radius: 2px;" onclick="sendDtmfCall('${call.name}')">Send</button>
                        </div>
                        
                        <div style="display: inline-flex; gap: 0.2rem; background: rgba(255,255,255,0.05); padding: 0.2rem; border-radius: var(--radius-sm); border: 1px solid var(--border-color);">
                            <input type="text" id="refer-${call.name}" placeholder="Transfer URI" style="width: 100px; background: transparent; border: none; color: #fff; font-size: 0.75rem; outline: none; text-align: center;">
                            <button class="btn btn-primary action-btn action-btn-sm" style="width:auto; padding: 0.2rem 0.4rem; font-size: 0.7rem; border-radius: 2px;" onclick="transferCall('${call.name}')">Transfer</button>
                        </div>
                    </td>
                `;
                callsBody.appendChild(tr);
            });
        }

        // Poll Console Logs if console is visible
        if (document.getElementById('tab-logs').classList.contains('active')) {
            updateConsoleLogs();
        }

    } catch (err) {
        console.error("Poller error:", err);
    }
}

// Fetch logs and write to console div
let lastLogLength = 0;
async function updateConsoleLogs() {
    try {
        const res = await fetch(`${API_URL}/api/logs`, { headers: getAuthHeaders() });
        if (!res.ok) return;
        const logs = await res.json();
        
        const consoleDiv = document.getElementById('console-output');
        consoleDiv.innerHTML = '';
        
        logs.forEach(line => {
            const lineEl = document.createElement('div');
            lineEl.classList.add('log-entry');
            
            if (line.includes(' INFO ')) lineEl.classList.add('log-info');
            else if (line.includes(' WARN ')) lineEl.classList.add('log-warn');
            else if (line.includes('ERROR')) lineEl.classList.add('log-error');
            else if (line.includes('DEBUG')) lineEl.classList.add('log-debug');

            lineEl.innerText = line;
            consoleDiv.appendChild(lineEl);
        });

        if (document.getElementById('auto-scroll-check').checked) {
            consoleDiv.scrollTop = consoleDiv.scrollHeight;
        }
    } catch (err) {
        console.error("Logger polling error:", err);
    }
}

function clearConsole() {
    document.getElementById('console-output').innerHTML = '';
}

// Load account settings configurations
async function loadAccountsConfig() {
    try {
        const res = await fetch(`${API_URL}/api/accounts`, { headers: getAuthHeaders() });
        if (!res.ok) return;
        const accounts = await res.json();

        const configBody = document.getElementById('accounts-config-body');
        configBody.innerHTML = '';

        if (accounts.length === 0) {
            configBody.innerHTML = `<tr><td colspan="8" style="text-align: center; color: var(--text-secondary);">No accounts found. Create a new one.</td></tr>`;
        } else {
            accounts.forEach(acc => {
                const autoAns = acc.auto_answer ? 'Yes (IVR)' : 'No';
                const tr = document.createElement('tr');
                tr.innerHTML = `
                    <td style="font-weight:600;">${acc.name}</td>
                    <td>${acc.username}</td>
                    <td>${acc.server}</td>
                    <td style="text-transform: uppercase;">${acc.codec || 'pcmu'}</td>
                    <td>${acc.sip_port === 0 ? 'Auto' : acc.sip_port}</td>
                    <td>${acc.rtp_port_start}-${acc.rtp_port_end}</td>
                    <td>${autoAns}</td>
                    <td class="action-group">
                        <button class="action-btn" title="Edit account" onclick="openEditAccountModal('${acc.name}')">✎</button>
                        <button class="action-btn" title="Delete account" style="color:var(--accent-error);" onclick="deleteAccount('${acc.name}')">🗑</button>
                    </td>
                `;
                configBody.appendChild(tr);
            });
        }
    } catch (err) {
        console.error("Failed to load accounts:", err);
    }
}

async function loadGlobalSettings() {
    const token = getToken();
    if (!token) return;

    try {
        const res = await fetch(`${API_URL}/api/config`, {
            headers: { 'Authorization': `Bearer ${token}` }
        });
        if (res.status === 401) return logout();
        if (!res.ok) throw new Error("Load failed");

        const config = await res.json();

        // Fill form fields
        if (config.web) {
            document.getElementById('settings-web-port').value = config.web.port || 9090;
            document.getElementById('settings-web-user').value = config.web.username || 'admin';
            document.getElementById('settings-web-pass').value = config.web.password || 'admin';
        } else {
            document.getElementById('settings-web-port').value = 9090;
            document.getElementById('settings-web-user').value = 'admin';
            document.getElementById('settings-web-pass').value = 'admin';
        }

        if (config.commands_api) {
            document.getElementById('settings-cmd-port').value = config.commands_api.port || 9099;
            document.getElementById('settings-cmd-user').value = config.commands_api.username || '';
            document.getElementById('settings-cmd-pass').value = config.commands_api.password || '';
        } else {
            document.getElementById('settings-cmd-port').value = 9099;
            document.getElementById('settings-cmd-user').value = '';
            document.getElementById('settings-cmd-pass').value = '';
        }

        // Fill raw config text area
        document.getElementById('settings-raw-config').value = JSON.stringify(config, null, 4);
    } catch (err) {
        console.error("Failed to load settings:", err);
    }
}

async function saveGlobalSettings() {
    const token = getToken();
    if (!token) return;

    try {
        let updatedConfig = null;
        const rawText = document.getElementById('settings-raw-config').value;

        try {
            updatedConfig = JSON.parse(rawText);
        } catch (e) {
            alert("Invalid JSON format in the raw editor!");
            return;
        }

        // Also sync basic form values into updatedConfig in case they edited form fields
        if (!updatedConfig.web) updatedConfig.web = {};
        updatedConfig.web.port = parseInt(document.getElementById('settings-web-port').value) || 9090;
        updatedConfig.web.username = document.getElementById('settings-web-user').value || 'admin';
        updatedConfig.web.password = document.getElementById('settings-web-pass').value || 'admin';

        const cmdPort = parseInt(document.getElementById('settings-cmd-port').value);
        const cmdUser = document.getElementById('settings-cmd-user').value;
        const cmdPass = document.getElementById('settings-cmd-pass').value;

        if (cmdPort) {
            if (!updatedConfig.commands_api) updatedConfig.commands_api = {};
            updatedConfig.commands_api.port = cmdPort;
            updatedConfig.commands_api.username = cmdUser ? cmdUser : null;
            updatedConfig.commands_api.password = cmdPass ? cmdPass : null;
        } else {
            updatedConfig.commands_api = null;
        }

        const res = await fetch(`${API_URL}/api/config`, {
            method: 'PUT',
            headers: {
                'Authorization': `Bearer ${token}`,
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(updatedConfig)
        });

        if (res.status === 401) return logout();
        const data = await res.json();
        if (data.success) {
            alert("Settings updated and service clients reloaded successfully!");
            loadGlobalSettings();
        } else {
            alert("Failed to update settings: " + (data.msg || "Unknown error"));
        }
    } catch (err) {
        alert("Failed to save settings: " + err);
    }
}

// Trigger manual registration API calls
async function registerAccount(name) {
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/register`, {
            method: 'POST',
            headers: getAuthHeaders()
        });
        const data = await res.json();
        alert(data.msg);
        updateDashboard();
    } catch (err) {
        alert("Failed to send register command");
    }
}

async function unregisterAccount(name) {
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/unregister`, {
            method: 'POST',
            headers: getAuthHeaders()
        });
        const data = await res.json();
        alert(data.msg);
        updateDashboard();
    } catch (err) {
        alert("Failed to send unregister command");
    }
}

// Account addition and modification forms
function openAddAccountModal() {
    document.getElementById('account-form').reset();
    document.getElementById('edit-original-name').value = '';
    document.getElementById('modal-mode-title').innerText = 'Add SIP Account';
    document.getElementById('acc-name').disabled = false;
    document.getElementById('ivr-subfields').style.display = 'none';
    document.getElementById('account-modal').classList.add('active');
}

async function openEditAccountModal(name) {
    try {
        const res = await fetch(`${API_URL}/api/accounts`, { headers: getAuthHeaders() });
        const accounts = await res.json();
        const acc = accounts.find(a => a.name === name);
        if (!acc) return;

        document.getElementById('edit-original-name').value = acc.name;
        document.getElementById('acc-name').value = acc.name;
        document.getElementById('acc-name').disabled = true; // Cannot rename ID during edit
        document.getElementById('acc-username').value = acc.username;
        document.getElementById('acc-password').value = acc.password;
        document.getElementById('acc-server').value = acc.server;
        document.getElementById('acc-domain').value = acc.domain || '';
        document.getElementById('acc-sip-port').value = acc.sip_port;
        document.getElementById('acc-codec').value = acc.codec || 'pcmu';
        document.getElementById('acc-rtp-start').value = acc.rtp_port_start;
        document.getElementById('acc-rtp-end').value = acc.rtp_port_end;
        document.getElementById('acc-auto-answer').checked = acc.auto_answer || false;

        const ivrFields = document.getElementById('ivr-subfields');
        if (acc.auto_answer) {
            ivrFields.style.display = 'block';
            document.getElementById('acc-ivr-welcome').value = acc.ivr_welcome || '';
        } else {
            ivrFields.style.display = 'none';
        }

        document.getElementById('modal-mode-title').innerText = 'Edit SIP Account';
        document.getElementById('account-modal').classList.add('active');
    } catch (e) {
        console.error(e);
    }
}

function closeAccountModal() {
    document.getElementById('account-modal').classList.remove('active');
}

// Form Submit for Add/Edit
document.getElementById('account-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const originalName = document.getElementById('edit-original-name').value;
    const isEdit = originalName.length > 0;

    const name = document.getElementById('acc-name').value;
    const username = document.getElementById('acc-username').value;
    const password = document.getElementById('acc-password').value;
    const server = document.getElementById('acc-server').value;
    const domain = document.getElementById('acc-domain').value || undefined;
    const sip_port = parseInt(document.getElementById('acc-sip-port').value);
    const codec = document.getElementById('acc-codec').value;
    const rtp_port_start = parseInt(document.getElementById('acc-rtp-start').value);
    const rtp_port_end = parseInt(document.getElementById('acc-rtp-end').value);
    const auto_answer = document.getElementById('acc-auto-answer').checked;
    const ivr_welcome = auto_answer ? (document.getElementById('acc-ivr-welcome').value || undefined) : undefined;

    const accountData = {
        name, username, password, server, domain, sip_port, codec,
        rtp_port_start, rtp_port_end, auto_answer, ivr_welcome
    };

    const url = isEdit ? `${API_URL}/api/accounts/${originalName}` : `${API_URL}/api/accounts`;
    const method = isEdit ? 'PUT' : 'POST';

    try {
        const res = await fetch(url, {
            method: method,
            headers: getAuthHeaders(),
            body: JSON.stringify(accountData)
        });

        if (res.ok) {
            closeAccountModal();
            loadAccountsConfig();
            updateDashboard();
        } else {
            alert("Failed to save account. Check for duplicate names or values.");
        }
    } catch (err) {
        alert("Network error saving configuration.");
    }
});

// Delete account configuration
async function deleteAccount(name) {
    if (!confirm(`Are you sure you want to delete account "${name}"?`)) return;

    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}`, {
            method: 'DELETE',
            headers: getAuthHeaders()
        });

        if (res.ok) {
            loadAccountsConfig();
            updateDashboard();
        } else {
            alert("Failed to delete account.");
        }
    } catch (err) {
        alert("Network error deleting account.");
    }
}

async function placeOutboundCall() {
    const account = document.getElementById('dialer-account').value;
    const target = document.getElementById('dialer-target').value;
    if (!account || !target) {
        alert("Please select an account and enter a target URI.");
        return;
    }
    try {
        const res = await fetch(`${API_URL}/api/accounts/${account}/call`, {
            method: 'POST',
            headers: { ...getAuthHeaders(), 'Content-Type': 'application/json' },
            body: JSON.stringify({ target })
        });
        const data = await res.json();
        if (data.success) {
            document.getElementById('dialer-target').value = '';
            updateDashboard();
        } else {
            alert("Call failed: " + data.msg);
        }
    } catch (err) {
        alert("Error placing call: " + err);
    }
}

async function hangupCall(name) {
    if (!confirm(`Are you sure you want to end the call for '${name}'?`)) return;
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/hangup`, {
            method: 'POST',
            headers: getAuthHeaders()
        });
        const data = await res.json();
        if (data.success) {
            updateDashboard();
        } else {
            alert("Hangup failed: " + data.msg);
        }
    } catch (err) {
        alert("Error: " + err);
    }
}

async function toggleHoldCall(name, isHeld) {
    const endpoint = isHeld ? 'resume' : 'hold';
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/${endpoint}`, {
            method: 'POST',
            headers: getAuthHeaders()
        });
        const data = await res.json();
        if (data.success) {
            updateDashboard();
        } else {
            alert(`Hold/Resume failed: ` + data.msg);
        }
    } catch (err) {
        alert("Error: " + err);
    }
}

async function sendDtmfCall(name) {
    const input = document.getElementById(`dtmf-${name}`);
    const digits = input.value;
    if (!digits) return;
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/dtmf`, {
            method: 'POST',
            headers: { ...getAuthHeaders(), 'Content-Type': 'application/json' },
            body: JSON.stringify({ digits })
        });
        const data = await res.json();
        if (data.success) {
            input.value = '';
            console.log(`DTMF ${digits} sent successfully.`);
        } else {
            alert("Failed to send DTMF: " + data.msg);
        }
    } catch (err) {
        alert("Error: " + err);
    }
}

async function transferCall(name) {
    const input = document.getElementById(`refer-${name}`);
    const target = input.value;
    if (!target) return;
    if (!confirm(`Transfer call to ${target}?`)) return;
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/transfer`, {
            method: 'POST',
            headers: { ...getAuthHeaders(), 'Content-Type': 'application/json' },
            body: JSON.stringify({ target })
        });
        const data = await res.json();
        if (data.success) {
            input.value = '';
            updateDashboard();
        } else {
            alert("Transfer failed: " + data.msg);
        }
    } catch (err) {
        alert("Error: " + err);
    }
}

// Run application
initApp();
