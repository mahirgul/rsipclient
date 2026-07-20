let pollTimer = null;
let cpuHistory = [];
let ramHistory = [];
const maxHistoryLength = 30;

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
    } else if (tabId === 'audio') {
        loadAudioFiles();
    } else if (tabId === 'tracer') {
        loadCallHistory();
        loadSipTraces();
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
        window.latestStatus = status;

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
        const memMB = status.memory_bytes / (1024 * 1024);
        document.getElementById('diag-mem').innerText = `${memMB.toFixed(1)} MB`;
        document.getElementById('diag-cpu').innerText = `${status.cpu_percent.toFixed(1)} %`;
        document.getElementById('uptime').innerText = `Uptime: ${formatDuration(status.uptime_secs)}`;
        if (status.config_path) {
            document.getElementById('diag-config-path').innerText = status.config_path;
        }

        // Update control port in diagnostics
        const ctrlPort = status.config_path ? "5090" : "-";
        document.getElementById('diag-ctrl-port').innerText = ctrlPort;

        if (status.app_version) {
            document.getElementById('app-version').innerText = `v${status.app_version}`;
            const footerVer = document.getElementById('footer-version');
            if (footerVer) {
                footerVer.innerText = `v${status.app_version}`;
            }
        }

        // Update utilization charts data
        cpuHistory.push(status.cpu_percent);
        ramHistory.push(memMB);
        if (cpuHistory.length > maxHistoryLength) cpuHistory.shift();
        if (ramHistory.length > maxHistoryLength) ramHistory.shift();
        
        drawResourceChart(status.cpu_percent, memMB);

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

// Draw CPU and Memory sparkline in SVG
function drawResourceChart(currCpu, currRam) {
    document.getElementById('chart-cpu-val').innerText = `${currCpu.toFixed(1)}%`;
    document.getElementById('chart-ram-val').innerText = `${currRam.toFixed(1)}MB`;

    const svg = document.getElementById('resource-svg-chart');
    if (!svg) return;
    const width = svg.clientWidth || 300;
    const height = svg.clientHeight || 120;

    const buildPath = (history, maxVal) => {
        if (history.length < 2) return "";
        const dx = width / (maxHistoryLength - 1);
        let points = [];
        for (let i = 0; i < history.length; i++) {
            const x = i * dx;
            const normY = history[i] / (maxVal || 1);
            const y = height - (normY * (height - 15) + 5);
            points.push(`${x},${y}`);
        }
        
        // Form closed path for nice fill gradient
        const lastX = (history.length - 1) * dx;
        return `M 0,${height} L ${points.join(' L ')} L ${lastX},${height} Z`;
    };

    // CPU Max is 100%, Memory max let's assume 256MB dynamically scaled
    const maxRam = Math.max(...ramHistory, 64) * 1.2;
    
    document.getElementById('cpu-chart-path').setAttribute('d', buildPath(cpuHistory, 100));
    document.getElementById('ram-chart-path').setAttribute('d', buildPath(ramHistory, maxRam));
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

// --- AUDIO MANAGER LOGIC ---
let activePreviewAudio = null;

async function loadAudioFiles() {
    try {
        const res = await fetch(`${API_URL}/api/audio`, { headers: getAuthHeaders() });
        if (!res.ok) return;
        const files = await res.json();

        const body = document.getElementById('audio-files-body');
        body.innerHTML = '';

        if (files.length === 0) {
            body.innerHTML = `<tr><td colspan="6" style="text-align: center; color: var(--text-secondary);">No audio files uploaded yet.</td></tr>`;
            return;
        }

        files.forEach(file => {
            const sizeKB = (file.size / 1024).toFixed(1);
            const specs = `${file.sample_rate}Hz, ${file.channels === 1 ? 'Mono' : 'Stereo'}`;
            const tr = document.createElement('tr');
            tr.innerHTML = `
                <td style="font-weight:600;">${file.name}</td>
                <td>${sizeKB} KB</td>
                <td>${file.duration_secs.toFixed(1)} s</td>
                <td>${specs}</td>
                <td>
                    <button class="btn btn-outline" style="width:auto; padding: 0.25rem 0.6rem; font-size:0.75rem;" onclick="playAudioPreview('${file.name}', this)">▶ Play</button>
                </td>
                <td class="action-group">
                    <a class="action-btn" title="Download" href="${API_URL}/api/audio/${file.name}" target="_blank" style="text-decoration:none; display:inline-flex; align-items:center; justify-content:center;">📥</a>
                    <button class="action-btn" title="Delete" style="color:var(--accent-error);" onclick="deleteAudioFile('${file.name}')">🗑</button>
                </td>
            `;
            body.appendChild(tr);
        });
    } catch (err) {
        console.error("Failed to load audio files:", err);
    }
}

function playAudioPreview(filename, btnEl) {
    if (activePreviewAudio) {
        activePreviewAudio.pause();
        const activeBtns = document.querySelectorAll('#audio-files-body btn');
        activeBtns.forEach(b => { if (b.innerText === '⏸ Pause') b.innerText = '▶ Play'; });
        if (activePreviewAudio.src.endsWith(encodeURIComponent(filename))) {
            activePreviewAudio = null;
            btnEl.innerText = '▶ Play';
            return;
        }
    }

    const token = getToken();
    const url = `${API_URL}/api/audio/${filename}`;
    
    // We fetch with auth header, create an object URL to preview securely
    fetch(url, { headers: getAuthHeaders() })
        .then(res => res.blob())
        .then(blob => {
            const objUrl = URL.createObjectURL(blob);
            activePreviewAudio = new Audio(objUrl);
            activePreviewAudio.play();
            btnEl.innerText = '⏸ Pause';
            activePreviewAudio.onended = () => {
                btnEl.innerText = '▶ Play';
                activePreviewAudio = null;
            };
        })
        .catch(err => {
            showNotification("Failed to load audio clip: " + err, "error");
        });
}

async function deleteAudioFile(name) {
    if (!confirm(`Are you sure you want to delete '${name}'?`)) return;
    try {
        const res = await fetch(`${API_URL}/api/audio/${name}`, {
            method: 'DELETE',
            headers: getAuthHeaders()
        });
        if (res.ok) {
            showNotification("Audio file deleted.", "success");
            loadAudioFiles();
        } else {
            showNotification("Failed to delete audio file.", "error");
        }
    } catch (err) {
        showNotification("Failed to delete: " + err, "error");
    }
}

async function handleAudioUpload() {
    const input = document.getElementById('audio-upload-input');
    const file = input.files[0];
    if (!file) return;

    const name = file.name;
    try {
        const arrayBuffer = await file.arrayBuffer();
        const res = await fetch(`${API_URL}/api/audio/${name}`, {
            method: 'POST',
            headers: {
                ...getAuthHeaders(),
                'Content-Type': 'application/octet-stream'
            },
            body: arrayBuffer
        });
        if (res.ok) {
            showNotification(`Uploaded ${name} successfully!`, "success");
            input.value = '';
            loadAudioFiles();
        } else {
            showNotification("Failed to upload audio file. Ensure it is a valid .wav.", "error");
        }
    } catch (err) {
        showNotification("Upload error: " + err, "error");
    }
}

// Voice recorder state variables
let recorderAudioCtx = null;
let recorderMicStream = null;
let recorderStartTime = null;
let recorderTimerInterval = null;
let recorderProcessorNode = null;
let recorderMicSource = null;
let recorderSamplesBuffer = [];

async function toggleAudioRecording() {
    const btn = document.getElementById('record-btn');
    if (recorderMicStream) {
        // Stop recording
        stopRecordingAndSave();
    } else {
        // Start recording
        try {
            recorderMicStream = await navigator.mediaDevices.getUserMedia({ audio: true });
            recorderAudioCtx = new (window.AudioContext || window.webkitAudioContext)({ sampleRate: 8000 });
            
            recorderMicSource = recorderAudioCtx.createMediaStreamSource(recorderMicStream);
            const bufferSize = 4096;
            recorderProcessorNode = recorderAudioCtx.createScriptProcessor(bufferSize, 1, 1);
            
            recorderSamplesBuffer = [];
            recorderProcessorNode.onaudioprocess = (e) => {
                const inputData = e.inputBuffer.getChannelData(0);
                recorderSamplesBuffer.push(new Float32Array(inputData));
            };
            
            recorderMicSource.connect(recorderProcessorNode);
            recorderProcessorNode.connect(recorderAudioCtx.destination);
            
            recorderStartTime = Date.now();
            document.getElementById('recorder-status').style.display = 'flex';
            btn.innerText = 'Stop Recording';
            btn.classList.add('pulse');
            
            recorderTimerInterval = setInterval(() => {
                const elapsed = ((Date.now() - recorderStartTime) / 1000).toFixed(1);
                document.getElementById('recorder-timer').innerText = `Recording: ${elapsed}s (8kHz Mono PCM)`;
            }, 100);
        } catch (err) {
            showNotification("Microphone access failed: " + err.message, "error");
            console.error(err);
        }
    }
}

async function stopRecordingAndSave() {
    const btn = document.getElementById('record-btn');
    if (!recorderMicStream) return;

    clearInterval(recorderTimerInterval);
    document.getElementById('recorder-status').style.display = 'none';
    btn.innerText = 'Record WAV';
    btn.classList.remove('pulse');

    recorderProcessorNode.disconnect();
    recorderMicSource.disconnect();
    
    if (recorderAudioCtx.state !== 'closed') {
        await recorderAudioCtx.close();
    }
    
    recorderMicStream.getTracks().forEach(track => track.stop());

    // Encode to 8kHz Mono 16-bit WAV
    const mergedFloats = mergeAudioBuffers(recorderSamplesBuffer);
    const wavArrayBuffer = encodeFloatsToWav(mergedFloats, 8000);
    const wavBlob = new Blob([wavArrayBuffer], { type: 'audio/wav' });

    const filename = `recorded_${Date.now()}.wav`;
    
    try {
        const res = await fetch(`${API_URL}/api/audio/${filename}`, {
            method: 'POST',
            headers: {
                ...getAuthHeaders(),
                'Content-Type': 'application/octet-stream'
            },
            body: wavBlob
        });
        if (res.ok) {
            showNotification(`Saved voice clip as '${filename}'`, "success");
            loadAudioFiles();
        } else {
            showNotification("Failed to upload recorded audio.", "error");
        }
    } catch (e) {
        showNotification("Recording upload failed: " + e, "error");
    }

    recorderMicStream = null;
    recorderAudioCtx = null;
}

function mergeAudioBuffers(buffers) {
    let length = buffers.reduce((acc, b) => acc + b.length, 0);
    let result = new Float32Array(length);
    let offset = 0;
    for (let i = 0; i < buffers.length; i++) {
        result.set(buffers[i], offset);
        offset += buffers[i].length;
    }
    return result;
}

function encodeFloatsToWav(samples, sampleRate) {
    const buffer = new ArrayBuffer(44 + samples.length * 2);
    const view = new DataView(buffer);
    
    // RIFF identifier
    writeChars(view, 0, 'RIFF');
    // File length
    view.setUint32(4, 36 + samples.length * 2, true);
    // RIFF type
    writeChars(view, 8, 'WAVE');
    // Format chunk identifier
    writeChars(view, 12, 'fmt ');
    // Format chunk length
    view.setUint32(16, 16, true);
    // Sample format (1 = uncompressed PCM)
    view.setUint16(20, 1, true);
    // Channel count (1 = mono)
    view.setUint16(22, 1, true);
    // Sample rate
    view.setUint32(24, sampleRate, true);
    // Byte rate (sample rate * block align)
    view.setUint32(28, sampleRate * 2, true);
    // Block align (channel count * bytes per sample)
    view.setUint16(32, 2, true);
    // Bits per sample
    view.setUint16(34, 16, true);
    // Data chunk identifier
    writeChars(view, 36, 'data');
    // Data chunk length
    view.setUint32(40, samples.length * 2, true);
    
    // Float values to 16-bit PCM
    let offset = 44;
    for (let i = 0; i < samples.length; i++, offset += 2) {
        let s = Math.max(-1, Math.min(1, samples[i]));
        view.setInt16(offset, s < 0 ? s * 0x8000 : s * 0x7FFF, true);
    }
    
    return buffer;
}

function writeChars(view, offset, string) {
    for (let i = 0; i < string.length; i++) {
        view.setUint8(offset + i, string.charCodeAt(i));
    }
}

// --- CALL HISTORY & SIP PACKET TRACER LOGIC ---

async function loadCallHistory() {
    try {
        const res = await fetch(`${API_URL}/api/calls/history`, { headers: getAuthHeaders() });
        if (!res.ok) return;
        const history = await res.json();

        const body = document.getElementById('call-history-body');
        body.innerHTML = '';

        if (history.length === 0) {
            body.innerHTML = `<tr><td colspan="8" style="text-align: center; color: var(--text-secondary);">No calls recorded in history.</td></tr>`;
            return;
        }

        history.forEach(call => {
            const dirBadge = call.direction === 'IN' 
                ? `<span class="badge badge-success">Incoming</span>` 
                : `<span class="badge badge-primary">Outgoing</span>`;
            
            const dur = call.end_time ? `${call.duration_secs}s` : 'Active';
            const stateClass = call.state === 'Completed' || call.state === 'Connected' ? 'badge-success' : 'badge-warning';
            
            const tr = document.createElement('tr');
            tr.innerHTML = `
                <td style="font-family: var(--font-mono); font-size:0.75rem; word-break:break-all;">${call.id}</td>
                <td style="font-weight:600;">${call.account}</td>
                <td>${dirBadge}</td>
                <td style="font-family: var(--font-mono); font-size:0.8rem;">${call.remote_uri}</td>
                <td>${call.start_time}</td>
                <td>${dur}</td>
                <td><span class="badge ${stateClass}">${call.state}</span></td>
                <td style="font-family: var(--font-mono); font-weight:600; color:var(--accent-warning); letter-spacing:1px;">${call.dtmf_digits || '-'}</td>
            `;
            body.appendChild(tr);
        });
    } catch (err) {
        console.error("Failed to load call history:", err);
    }
}

let cachedSipTraces = [];

async function loadSipTraces() {
    try {
        const res = await fetch(`${API_URL}/api/sip/traces`, { headers: getAuthHeaders() });
        if (!res.ok) return;
        cachedSipTraces = await res.json();

        const diagram = document.getElementById('sip-sequence-diagram');
        diagram.innerHTML = '';

        if (cachedSipTraces.length === 0) {
            diagram.innerHTML = `<div style="text-align: center; padding: 2rem; color: var(--text-secondary);">No SIP messages captured yet. Place a call or register to trace.</div>`;
            return;
        }

        // Add headers
        const headerDiv = document.createElement('div');
        headerDiv.style.display = 'flex';
        headerDiv.style.justifyContent = 'space-between';
        headerDiv.style.fontWeight = 'bold';
        headerDiv.style.borderBottom = '1px solid rgba(255,255,255,0.1)';
        headerDiv.style.paddingBottom = '0.5rem';
        headerDiv.style.marginBottom = '0.5rem';
        headerDiv.innerHTML = `
            <span>SIP Server</span>
            <span>Signaling Flow</span>
            <span>Local Client</span>
        `;
        diagram.appendChild(headerDiv);

        cachedSipTraces.forEach((trace, idx) => {
            const firstLine = trace.message.split('\n')[0] || "SIP Message";
            const dirClass = trace.direction === 'IN' ? 'in' : 'out';
            const arrowClass = trace.direction === 'IN' ? 'in-arrow' : 'out-arrow';
            
            const div = document.createElement('div');
            div.className = `trace-item ${dirClass}`;
            div.onclick = () => selectSipTrace(idx);
            
            div.innerHTML = `
                <span style="font-size:0.7rem; opacity:0.6; width:60px;">${trace.timestamp}</span>
                <div class="trace-arrow ${arrowClass}">
                    <span class="trace-arrow-text">${firstLine} (${trace.transport})</span>
                </div>
                <span style="font-weight:600; font-size:0.75rem; text-align:right; width:60px;">${trace.account}</span>
            `;
            diagram.appendChild(div);
        });
    } catch (err) {
        console.error("Failed to load SIP traces:", err);
    }
}

function selectSipTrace(idx) {
    const trace = cachedSipTraces[idx];
    if (!trace) return;
    
    // Highlight selected item
    document.querySelectorAll('.trace-item').forEach((item, i) => {
        if (i === idx) {
            item.style.borderColor = 'var(--accent-primary)';
            item.style.background = 'rgba(59, 130, 246, 0.15)';
        } else {
            item.style.borderColor = '';
            item.style.background = '';
        }
    });

    const info = `// TIMESTAMP: ${trace.timestamp} | DIRECTION: ${trace.direction} | ACCOUNT: ${trace.account} | VIA: ${trace.transport}\n\n${trace.message}`;
    document.getElementById('sip-raw-message').value = info;
}


// --- SIP CALL CONTROL ---

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
