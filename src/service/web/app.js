let pollTimer = null;

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

        if (status.app_version) {
            document.getElementById('app-version').innerText = `v${status.app_version}`;
        }

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
                        
                        <div style="display: inline-flex; gap: 0.2rem; background: rgba(255,255,255,0.05); padding: 0.2rem; border-radius: var(--radius-sm); border: 1px solid var(--border-color);">
                            <input type="text" id="play-${call.name}" placeholder="WAV Path" style="width: 80px; background: transparent; border: none; color: #fff; font-size: 0.75rem; outline: none; text-align: center;">
                            <button class="btn btn-primary action-btn action-btn-sm" style="width:auto; padding: 0.2rem 0.4rem; font-size: 0.7rem; border-radius: 2px;" onclick="playWavCall('${call.name}')">Play</button>
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

async function placeOutboundCall() {
    const account = document.getElementById('dialer-account').value;
    const target = document.getElementById('dialer-target').value;
    if (!account || !target) {
        showNotification("Please select an account and enter a target URI.", "warning");
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
            showNotification(data.msg || "Call placed successfully", "success");
            document.getElementById('dialer-target').value = '';
            updateDashboard();
        } else {
            showNotification("Call failed: " + data.msg, "error");
        }
    } catch (err) {
        showNotification("Error placing call: " + err, "error");
    }
}

async function hangupCall(name) {
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/hangup`, {
            method: 'POST',
            headers: getAuthHeaders()
        });
        const data = await res.json();
        if (data.success) {
            showNotification(data.msg || "Call hung up", "success");
            updateDashboard();
        } else {
            showNotification("Hangup failed: " + data.msg, "error");
        }
    } catch (err) {
        showNotification("Error: " + err, "error");
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
            showNotification(data.msg || (isHeld ? "Call resumed" : "Call held"), "success");
            updateDashboard();
        } else {
            showNotification(`Hold/Resume failed: ` + data.msg, "error");
        }
    } catch (err) {
        showNotification("Error: " + err, "error");
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
            showNotification(`DTMF '${digits}' sent successfully.`, "success");
        } else {
            showNotification("Failed to send DTMF: " + data.msg, "error");
        }
    } catch (err) {
        showNotification("Error: " + err, "error");
    }
}

async function transferCall(name) {
    const input = document.getElementById(`refer-${name}`);
    const target = input.value;
    if (!target) return;
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/transfer`, {
            method: 'POST',
            headers: { ...getAuthHeaders(), 'Content-Type': 'application/json' },
            body: JSON.stringify({ target })
        });
        const data = await res.json();
        if (data.success) {
            showNotification(data.msg || "Transfer initiated successfully.", "success");
            input.value = '';
            updateDashboard();
        } else {
            showNotification("Transfer failed: " + data.msg, "error");
        }
    } catch (err) {
        showNotification("Error: " + err, "error");
    }
}

async function playWavCall(name) {
    const input = document.getElementById(`play-${name}`);
    const file = input.value;
    if (!file) return;
    try {
        const res = await fetch(`${API_URL}/api/accounts/${name}/play`, {
            method: 'POST',
            headers: { ...getAuthHeaders(), 'Content-Type': 'application/json' },
            body: JSON.stringify({ file })
        });
        const data = await res.json();
        if (data.success) {
            showNotification(data.msg, "success");
            input.value = '';
        } else {
            showNotification(data.msg, "error");
        }
    } catch (err) {
        showNotification("Error: " + err, "error");
    }
}

// Run application
initApp();
